//! Shared idempotency registry primitives for HA-safe mutation APIs and workers.
//!
//! Retention behavior is explicit:
//! - an `InFlight` record suppresses duplicate execution until its
//!   `in_flight_expires_at_ms`;
//! - a same-fingerprint retry after in-flight expiry may reclaim the reservation
//!   with a new owner/command id and incremented attempt;
//! - a `Completed` record replays the original result until
//!   `retention_expires_at_ms`;
//! - completed records may be removed only through `prune_expired`, after which
//!   the same key can reserve new work;
//! - any same-key/different-fingerprint request is an idempotency conflict while
//!   a record is retained.
//!
//! The registry intentionally exposes completion of a reserved command result,
//! not a standalone "fail a reserved transaction" operation. No-commit failure
//! finalization belongs to the HA transaction/finality path.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandResultStatus {
    Succeeded,
    RejectedTerminal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandResult {
    pub status: CommandResultStatus,
    pub payload: Value,
}

impl CommandResult {
    pub fn succeeded(payload: Value) -> Self {
        Self {
            status: CommandResultStatus::Succeeded,
            payload,
        }
    }

    pub fn rejected_terminal(payload: Value) -> Self {
        Self {
            status: CommandResultStatus::RejectedTerminal,
            payload,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdempotencyStatus {
    InFlight,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdempotencyRecord {
    pub scope: String,
    pub namespace: String,
    pub key: String,
    pub fingerprint: String,
    pub command_id: String,
    pub owner_id: String,
    pub attempt: u64,
    pub reservation_token: u64,
    pub status: IdempotencyStatus,
    pub reserved_at_ms: u64,
    pub in_flight_expires_at_ms: u64,
    pub retention_expires_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub result: Option<CommandResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdempotencyReservationRequest {
    pub scope: String,
    pub namespace: String,
    pub key: String,
    pub fingerprint: String,
    pub command_id: String,
    pub owner_id: String,
    pub now_ms: u64,
    pub in_flight_ttl_ms: u64,
    pub retention_ttl_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReserveOutcome {
    Reserved(IdempotencyRecord),
    InFlight(IdempotencyRecord),
    ReplayedCompleted(IdempotencyRecord),
    Reclaimed(IdempotencyRecord),
}

impl ReserveOutcome {
    pub fn record(&self) -> &IdempotencyRecord {
        match self {
            Self::Reserved(record)
            | Self::InFlight(record)
            | Self::ReplayedCompleted(record)
            | Self::Reclaimed(record) => record,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandCompletion {
    pub scope: String,
    pub namespace: String,
    pub key: String,
    pub reservation_token: u64,
    pub now_ms: u64,
    pub result: CommandResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyError {
    Conflict {
        scope: String,
        namespace: String,
        key: String,
    },
    NotFound {
        scope: String,
        namespace: String,
        key: String,
    },
    StaleReservation {
        scope: String,
        namespace: String,
        key: String,
    },
    AlreadyCompleted {
        scope: String,
        namespace: String,
        key: String,
    },
    BackendUnavailable {
        message: String,
    },
}

impl IdempotencyError {
    pub fn is_idempotency_conflict(&self) -> bool {
        matches!(self, Self::Conflict { .. })
    }
}

impl std::fmt::Display for IdempotencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict {
                scope,
                namespace,
                key,
            } => write!(f, "idempotency conflict for {scope}/{namespace} key {key}"),
            Self::NotFound {
                scope,
                namespace,
                key,
            } => write!(
                f,
                "idempotency record not found for {scope}/{namespace} key {key}"
            ),
            Self::StaleReservation {
                scope,
                namespace,
                key,
            } => write!(
                f,
                "stale idempotency reservation for {scope}/{namespace} key {key}"
            ),
            Self::AlreadyCompleted {
                scope,
                namespace,
                key,
            } => write!(
                f,
                "idempotency record already completed for {scope}/{namespace} key {key}"
            ),
            Self::BackendUnavailable { message } => write!(f, "backend unavailable: {message}"),
        }
    }
}

impl std::error::Error for IdempotencyError {}

pub type IdempotencyResult<T> = Result<T, IdempotencyError>;

pub trait IdempotencyRegistry {
    fn reserve(&self, request: IdempotencyReservationRequest) -> IdempotencyResult<ReserveOutcome>;
    fn complete(&self, completion: CommandCompletion) -> IdempotencyResult<IdempotencyRecord>;
    fn get(&self, scope: &str, namespace: &str, key: &str) -> Option<IdempotencyRecord>;
    fn prune_expired(&self, now_ms: u64) -> usize;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryIdempotencyRegistry {
    inner: Arc<Mutex<IdempotencyState>>,
}

#[derive(Debug, Default)]
struct IdempotencyState {
    records: BTreeMap<(String, String, String), IdempotencyRecord>,
    next_reservation_token: u64,
}

impl IdempotencyRegistry for InMemoryIdempotencyRegistry {
    fn reserve(&self, request: IdempotencyReservationRequest) -> IdempotencyResult<ReserveOutcome> {
        let mut inner = self.lock()?;
        let key = record_key(&request.scope, &request.namespace, &request.key);
        if let Some(existing) = inner.records.get(&key).cloned() {
            if existing.fingerprint != request.fingerprint {
                return Err(IdempotencyError::Conflict {
                    scope: request.scope,
                    namespace: request.namespace,
                    key: request.key,
                });
            }
            return match existing.status {
                IdempotencyStatus::Completed
                    if existing.retention_expires_at_ms >= request.now_ms =>
                {
                    Ok(ReserveOutcome::ReplayedCompleted(existing))
                }
                IdempotencyStatus::Completed => {
                    inner.records.remove(&key);
                    reserve_new(&mut inner, key, request, 1).map(ReserveOutcome::Reserved)
                }
                IdempotencyStatus::InFlight
                    if existing.in_flight_expires_at_ms >= request.now_ms =>
                {
                    Ok(ReserveOutcome::InFlight(existing))
                }
                IdempotencyStatus::InFlight => {
                    let attempt = existing.attempt + 1;
                    reserve_new(&mut inner, key, request, attempt).map(ReserveOutcome::Reclaimed)
                }
            };
        }
        reserve_new(&mut inner, key, request, 1).map(ReserveOutcome::Reserved)
    }

    fn complete(&self, completion: CommandCompletion) -> IdempotencyResult<IdempotencyRecord> {
        let mut inner = self.lock()?;
        let key = record_key(&completion.scope, &completion.namespace, &completion.key);
        let record = inner
            .records
            .get_mut(&key)
            .ok_or_else(|| IdempotencyError::NotFound {
                scope: completion.scope.clone(),
                namespace: completion.namespace.clone(),
                key: completion.key.clone(),
            })?;
        if record.reservation_token != completion.reservation_token {
            return Err(IdempotencyError::StaleReservation {
                scope: completion.scope,
                namespace: completion.namespace,
                key: completion.key,
            });
        }
        if record.status == IdempotencyStatus::Completed {
            return Err(IdempotencyError::AlreadyCompleted {
                scope: completion.scope,
                namespace: completion.namespace,
                key: completion.key,
            });
        }
        record.status = IdempotencyStatus::Completed;
        record.completed_at_ms = Some(completion.now_ms);
        record.result = Some(completion.result);
        Ok(record.clone())
    }

    fn get(&self, scope: &str, namespace: &str, key: &str) -> Option<IdempotencyRecord> {
        self.inner
            .lock()
            .ok()?
            .records
            .get(&record_key(scope, namespace, key))
            .cloned()
    }

    fn prune_expired(&self, now_ms: u64) -> usize {
        let Ok(mut inner) = self.inner.lock() else {
            return 0;
        };
        let before = inner.records.len();
        inner.records.retain(|_, record| match record.status {
            IdempotencyStatus::Completed => record.retention_expires_at_ms >= now_ms,
            IdempotencyStatus::InFlight => true,
        });
        before - inner.records.len()
    }
}

impl InMemoryIdempotencyRegistry {
    fn lock(&self) -> IdempotencyResult<std::sync::MutexGuard<'_, IdempotencyState>> {
        self.inner
            .lock()
            .map_err(|_| IdempotencyError::BackendUnavailable {
                message: "in-memory idempotency registry mutex poisoned".to_string(),
            })
    }
}

fn reserve_new(
    inner: &mut IdempotencyState,
    key: (String, String, String),
    request: IdempotencyReservationRequest,
    attempt: u64,
) -> IdempotencyResult<IdempotencyRecord> {
    inner.next_reservation_token += 1;
    let record = IdempotencyRecord {
        scope: request.scope,
        namespace: request.namespace,
        key: request.key,
        fingerprint: request.fingerprint,
        command_id: request.command_id,
        owner_id: request.owner_id,
        attempt,
        reservation_token: inner.next_reservation_token,
        status: IdempotencyStatus::InFlight,
        reserved_at_ms: request.now_ms,
        in_flight_expires_at_ms: request.now_ms.saturating_add(request.in_flight_ttl_ms),
        retention_expires_at_ms: request.now_ms.saturating_add(request.retention_ttl_ms),
        completed_at_ms: None,
        result: None,
    };
    inner.records.insert(key, record.clone());
    Ok(record)
}

fn record_key(scope: &str, namespace: &str, key: &str) -> (String, String, String) {
    (scope.to_string(), namespace.to_string(), key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn completing_with_stale_token_is_rejected() {
        let registry = InMemoryIdempotencyRegistry::default();
        registry
            .reserve(IdempotencyReservationRequest {
                scope: "workspace:mesh".to_string(),
                namespace: "agentmesh".to_string(),
                key: "key-1".to_string(),
                fingerprint: "fingerprint-a".to_string(),
                command_id: "cmd-1".to_string(),
                owner_id: "replica-a".to_string(),
                now_ms: 0,
                in_flight_ttl_ms: 10,
                retention_ttl_ms: 100,
            })
            .expect("reserve");

        let err = registry
            .complete(CommandCompletion {
                scope: "workspace:mesh".to_string(),
                namespace: "agentmesh".to_string(),
                key: "key-1".to_string(),
                reservation_token: 999,
                now_ms: 1,
                result: CommandResult::succeeded(json!({})),
            })
            .expect_err("stale token");

        assert!(matches!(err, IdempotencyError::StaleReservation { .. }));
    }
}
