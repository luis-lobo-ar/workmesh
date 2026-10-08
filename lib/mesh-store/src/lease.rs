use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseAcquireRequest {
    pub lease_name: String,
    pub owner_id: String,
    pub now_ms: u64,
    pub ttl_ms: u64,
}

impl LeaseAcquireRequest {
    pub fn new(
        lease_name: impl Into<String>,
        owner_id: impl Into<String>,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Self {
        Self {
            lease_name: lease_name.into(),
            owner_id: owner_id.into(),
            now_ms,
            ttl_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRenewRequest {
    pub lease_name: String,
    pub owner_id: String,
    pub lease_version: u64,
    pub fencing_token: u64,
    pub now_ms: u64,
    pub ttl_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseReleaseRequest {
    pub lease_name: String,
    pub owner_id: String,
    pub fencing_token: u64,
    pub now_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderElectionRequest {
    pub group: String,
    pub candidates: Vec<String>,
    pub now_ms: u64,
    pub ttl_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseStatus {
    Active,
    Released,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRecord {
    pub lease_name: String,
    pub owner_id: String,
    pub lease_version: u64,
    pub fencing_token: u64,
    pub acquired_at_ms: u64,
    pub renewed_at_ms: u64,
    pub expires_at_ms: u64,
    pub status: LeaseStatus,
}

impl LeaseRecord {
    pub fn is_active_at(&self, now_ms: u64) -> bool {
        self.status == LeaseStatus::Active && self.expires_at_ms > now_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LeaseAuditEventKind {
    Acquired,
    AcquireRejected,
    Renewed,
    Released,
    Expired,
    Takeover,
    StaleFencingRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseAuditEvent {
    pub lease_name: String,
    pub kind: LeaseAuditEventKind,
    pub owner_id: String,
    pub previous_owner_id: Option<String>,
    pub lease_version: u64,
    pub fencing_token: u64,
    pub at_ms: u64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseError {
    LeaseHeld {
        lease_name: String,
        owner_id: String,
        expires_at_ms: u64,
    },
    LeaseNotFound {
        lease_name: String,
    },
    StaleOwner {
        lease_name: String,
        expected_owner_id: String,
        actual_owner_id: Option<String>,
    },
    StaleFencingToken {
        lease_name: String,
        owner_id: String,
        expected_fencing_token: u64,
        actual_fencing_token: Option<u64>,
    },
    EmptyCandidateSet {
        group: String,
    },
    BackendUnavailable {
        message: String,
    },
}

impl LeaseError {
    pub fn is_lease_held(&self) -> bool {
        matches!(self, Self::LeaseHeld { .. })
    }

    pub fn is_stale_owner(&self) -> bool {
        matches!(self, Self::StaleOwner { .. })
    }

    pub fn is_stale_fencing_token(&self) -> bool {
        matches!(self, Self::StaleFencingToken { .. })
    }
}

impl std::fmt::Display for LeaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LeaseHeld {
                lease_name,
                owner_id,
                expires_at_ms,
            } => write!(
                f,
                "lease {lease_name} is held by {owner_id} until {expires_at_ms}"
            ),
            Self::LeaseNotFound { lease_name } => write!(f, "lease {lease_name} not found"),
            Self::StaleOwner {
                lease_name,
                expected_owner_id,
                actual_owner_id,
            } => write!(
                f,
                "stale owner for lease {lease_name}: expected {expected_owner_id}, actual {actual_owner_id:?}"
            ),
            Self::StaleFencingToken {
                lease_name,
                owner_id,
                expected_fencing_token,
                actual_fencing_token,
            } => write!(
                f,
                "stale fencing token for lease {lease_name} owner {owner_id}: expected {expected_fencing_token}, actual {actual_fencing_token:?}"
            ),
            Self::EmptyCandidateSet { group } => {
                write!(f, "leader election group {group} has no candidates")
            }
            Self::BackendUnavailable { message } => write!(f, "backend unavailable: {message}"),
        }
    }
}

impl std::error::Error for LeaseError {}

pub type LeaseResult<T> = Result<T, LeaseError>;

pub trait LeaseStore {
    fn acquire(&self, request: LeaseAcquireRequest) -> LeaseResult<LeaseRecord>;
    fn renew(&self, request: LeaseRenewRequest) -> LeaseResult<LeaseRecord>;
    fn release(&self, request: LeaseReleaseRequest) -> LeaseResult<LeaseRecord>;
    fn expire(&self, lease_name: &str, now_ms: u64) -> LeaseResult<LeaseRecord>;
    fn get(&self, lease_name: &str) -> Option<LeaseRecord>;
    fn verify_fencing(
        &self,
        lease_name: &str,
        owner_id: &str,
        fencing_token: u64,
    ) -> LeaseResult<LeaseRecord>;
    fn audit(&self, lease_name: &str) -> Vec<LeaseAuditEvent>;
    fn elect_leader(&self, request: LeaderElectionRequest) -> LeaseResult<LeaseRecord>;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryLeaseStore {
    inner: Arc<Mutex<LeaseState>>,
}

#[derive(Debug, Default)]
struct LeaseState {
    leases: BTreeMap<String, LeaseRecord>,
    audit: BTreeMap<String, Vec<LeaseAuditEvent>>,
}

impl LeaseStore for InMemoryLeaseStore {
    fn acquire(&self, request: LeaseAcquireRequest) -> LeaseResult<LeaseRecord> {
        let mut inner = self.lock()?;
        let existing = inner.leases.get(&request.lease_name).cloned();

        match existing {
            Some(current) if current.is_active_at(request.now_ms) => {
                if current.owner_id == request.owner_id {
                    return Ok(current);
                }
                append_audit(
                    &mut inner,
                    LeaseAuditEvent {
                        lease_name: request.lease_name.clone(),
                        kind: LeaseAuditEventKind::AcquireRejected,
                        owner_id: request.owner_id,
                        previous_owner_id: Some(current.owner_id.clone()),
                        lease_version: current.lease_version,
                        fencing_token: current.fencing_token,
                        at_ms: request.now_ms,
                        reason: Some("lease held by active owner".to_string()),
                    },
                );
                Err(LeaseError::LeaseHeld {
                    lease_name: current.lease_name,
                    owner_id: current.owner_id,
                    expires_at_ms: current.expires_at_ms,
                })
            }
            Some(current) => {
                let next_version = current.lease_version + 1;
                let next_token = current.fencing_token + 1;
                let previous_owner_id = Some(current.owner_id);
                let record = LeaseRecord {
                    lease_name: request.lease_name.clone(),
                    owner_id: request.owner_id.clone(),
                    lease_version: next_version,
                    fencing_token: next_token,
                    acquired_at_ms: request.now_ms,
                    renewed_at_ms: request.now_ms,
                    expires_at_ms: request.now_ms.saturating_add(request.ttl_ms),
                    status: LeaseStatus::Active,
                };
                inner
                    .leases
                    .insert(request.lease_name.clone(), record.clone());
                append_audit(
                    &mut inner,
                    LeaseAuditEvent {
                        lease_name: request.lease_name,
                        kind: LeaseAuditEventKind::Takeover,
                        owner_id: request.owner_id,
                        previous_owner_id,
                        lease_version: next_version,
                        fencing_token: next_token,
                        at_ms: record.acquired_at_ms,
                        reason: Some("previous lease inactive or expired".to_string()),
                    },
                );
                Ok(record)
            }
            None => {
                let record = LeaseRecord {
                    lease_name: request.lease_name.clone(),
                    owner_id: request.owner_id.clone(),
                    lease_version: 1,
                    fencing_token: 1,
                    acquired_at_ms: request.now_ms,
                    renewed_at_ms: request.now_ms,
                    expires_at_ms: request.now_ms.saturating_add(request.ttl_ms),
                    status: LeaseStatus::Active,
                };
                inner
                    .leases
                    .insert(request.lease_name.clone(), record.clone());
                append_audit(
                    &mut inner,
                    LeaseAuditEvent {
                        lease_name: request.lease_name,
                        kind: LeaseAuditEventKind::Acquired,
                        owner_id: request.owner_id,
                        previous_owner_id: None,
                        lease_version: record.lease_version,
                        fencing_token: record.fencing_token,
                        at_ms: record.acquired_at_ms,
                        reason: None,
                    },
                );
                Ok(record)
            }
        }
    }

    fn renew(&self, request: LeaseRenewRequest) -> LeaseResult<LeaseRecord> {
        let mut inner = self.lock()?;
        let current = inner
            .leases
            .get(&request.lease_name)
            .cloned()
            .ok_or_else(|| LeaseError::LeaseNotFound {
                lease_name: request.lease_name.clone(),
            })?;
        validate_owner_and_fencing(
            &current,
            &request.owner_id,
            request.lease_version,
            request.fencing_token,
        )?;
        if !current.is_active_at(request.now_ms) {
            return Err(LeaseError::StaleOwner {
                lease_name: request.lease_name,
                expected_owner_id: request.owner_id,
                actual_owner_id: Some(current.owner_id),
            });
        }

        let record = LeaseRecord {
            lease_name: request.lease_name.clone(),
            owner_id: request.owner_id.clone(),
            lease_version: current.lease_version + 1,
            fencing_token: current.fencing_token,
            acquired_at_ms: current.acquired_at_ms,
            renewed_at_ms: request.now_ms,
            expires_at_ms: request.now_ms.saturating_add(request.ttl_ms),
            status: LeaseStatus::Active,
        };
        inner
            .leases
            .insert(request.lease_name.clone(), record.clone());
        append_audit(
            &mut inner,
            LeaseAuditEvent {
                lease_name: request.lease_name,
                kind: LeaseAuditEventKind::Renewed,
                owner_id: request.owner_id,
                previous_owner_id: None,
                lease_version: record.lease_version,
                fencing_token: record.fencing_token,
                at_ms: record.renewed_at_ms,
                reason: None,
            },
        );
        Ok(record)
    }

    fn release(&self, request: LeaseReleaseRequest) -> LeaseResult<LeaseRecord> {
        let mut inner = self.lock()?;
        let current = inner
            .leases
            .get(&request.lease_name)
            .cloned()
            .ok_or_else(|| LeaseError::LeaseNotFound {
                lease_name: request.lease_name.clone(),
            })?;
        validate_owner(&current, &request.owner_id)?;
        if current.fencing_token != request.fencing_token {
            append_stale_fencing_audit(
                &mut inner,
                &current,
                &request.owner_id,
                request.fencing_token,
                request.now_ms,
            );
            return Err(LeaseError::StaleFencingToken {
                lease_name: request.lease_name,
                owner_id: request.owner_id,
                expected_fencing_token: request.fencing_token,
                actual_fencing_token: Some(current.fencing_token),
            });
        }

        let record = LeaseRecord {
            status: LeaseStatus::Released,
            expires_at_ms: request.now_ms,
            ..current
        };
        inner
            .leases
            .insert(request.lease_name.clone(), record.clone());
        append_audit(
            &mut inner,
            LeaseAuditEvent {
                lease_name: request.lease_name,
                kind: LeaseAuditEventKind::Released,
                owner_id: request.owner_id,
                previous_owner_id: None,
                lease_version: record.lease_version,
                fencing_token: record.fencing_token,
                at_ms: request.now_ms,
                reason: None,
            },
        );
        Ok(record)
    }

    fn expire(&self, lease_name: &str, now_ms: u64) -> LeaseResult<LeaseRecord> {
        let mut inner = self.lock()?;
        let current =
            inner
                .leases
                .get(lease_name)
                .cloned()
                .ok_or_else(|| LeaseError::LeaseNotFound {
                    lease_name: lease_name.to_string(),
                })?;
        if current.is_active_at(now_ms) {
            return Err(LeaseError::LeaseHeld {
                lease_name: current.lease_name,
                owner_id: current.owner_id,
                expires_at_ms: current.expires_at_ms,
            });
        }
        let record = LeaseRecord {
            status: LeaseStatus::Expired,
            ..current
        };
        inner.leases.insert(lease_name.to_string(), record.clone());
        append_audit(
            &mut inner,
            LeaseAuditEvent {
                lease_name: lease_name.to_string(),
                kind: LeaseAuditEventKind::Expired,
                owner_id: record.owner_id.clone(),
                previous_owner_id: None,
                lease_version: record.lease_version,
                fencing_token: record.fencing_token,
                at_ms: now_ms,
                reason: Some("lease ttl elapsed".to_string()),
            },
        );
        Ok(record)
    }

    fn get(&self, lease_name: &str) -> Option<LeaseRecord> {
        self.inner.lock().ok()?.leases.get(lease_name).cloned()
    }

    fn verify_fencing(
        &self,
        lease_name: &str,
        owner_id: &str,
        fencing_token: u64,
    ) -> LeaseResult<LeaseRecord> {
        let mut inner = self.lock()?;
        let current =
            inner
                .leases
                .get(lease_name)
                .cloned()
                .ok_or_else(|| LeaseError::LeaseNotFound {
                    lease_name: lease_name.to_string(),
                })?;
        validate_owner(&current, owner_id)?;
        if current.status != LeaseStatus::Active || current.fencing_token != fencing_token {
            append_stale_fencing_audit(&mut inner, &current, owner_id, fencing_token, 0);
            return Err(LeaseError::StaleFencingToken {
                lease_name: lease_name.to_string(),
                owner_id: owner_id.to_string(),
                expected_fencing_token: fencing_token,
                actual_fencing_token: Some(current.fencing_token),
            });
        }
        Ok(current)
    }

    fn audit(&self, lease_name: &str) -> Vec<LeaseAuditEvent> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.audit.get(lease_name).cloned())
            .unwrap_or_default()
    }

    fn elect_leader(&self, request: LeaderElectionRequest) -> LeaseResult<LeaseRecord> {
        let mut candidates = request.candidates;
        candidates.sort();
        candidates.dedup();
        let Some(candidate) = candidates.first().cloned() else {
            return Err(LeaseError::EmptyCandidateSet {
                group: request.group,
            });
        };
        let lease_name = format!("leader:{}", request.group);
        match self.acquire(LeaseAcquireRequest::new(
            &lease_name,
            candidate,
            request.now_ms,
            request.ttl_ms,
        )) {
            Ok(record) => Ok(record),
            Err(LeaseError::LeaseHeld { .. }) => self
                .get(&lease_name)
                .ok_or(LeaseError::LeaseNotFound { lease_name }),
            Err(err) => Err(err),
        }
    }
}

impl InMemoryLeaseStore {
    fn lock(&self) -> LeaseResult<std::sync::MutexGuard<'_, LeaseState>> {
        self.inner
            .lock()
            .map_err(|_| LeaseError::BackendUnavailable {
                message: "in-memory lease mutex poisoned".to_string(),
            })
    }
}

fn validate_owner(current: &LeaseRecord, owner_id: &str) -> LeaseResult<()> {
    if current.owner_id == owner_id {
        return Ok(());
    }
    Err(LeaseError::StaleOwner {
        lease_name: current.lease_name.clone(),
        expected_owner_id: owner_id.to_string(),
        actual_owner_id: Some(current.owner_id.clone()),
    })
}

fn validate_owner_and_fencing(
    current: &LeaseRecord,
    owner_id: &str,
    lease_version: u64,
    fencing_token: u64,
) -> LeaseResult<()> {
    validate_owner(current, owner_id)?;
    if current.lease_version != lease_version {
        return Err(LeaseError::StaleOwner {
            lease_name: current.lease_name.clone(),
            expected_owner_id: owner_id.to_string(),
            actual_owner_id: Some(current.owner_id.clone()),
        });
    }
    if current.fencing_token != fencing_token {
        return Err(LeaseError::StaleFencingToken {
            lease_name: current.lease_name.clone(),
            owner_id: owner_id.to_string(),
            expected_fencing_token: fencing_token,
            actual_fencing_token: Some(current.fencing_token),
        });
    }
    Ok(())
}

fn append_audit(inner: &mut LeaseState, event: LeaseAuditEvent) {
    inner
        .audit
        .entry(event.lease_name.clone())
        .or_default()
        .push(event);
}

fn append_stale_fencing_audit(
    inner: &mut LeaseState,
    current: &LeaseRecord,
    owner_id: &str,
    attempted_token: u64,
    at_ms: u64,
) {
    append_audit(
        inner,
        LeaseAuditEvent {
            lease_name: current.lease_name.clone(),
            kind: LeaseAuditEventKind::StaleFencingRejected,
            owner_id: owner_id.to_string(),
            previous_owner_id: Some(current.owner_id.clone()),
            lease_version: current.lease_version,
            fencing_token: attempted_token,
            at_ms,
            reason: Some(format!(
                "current fencing token is {} with status {:?}",
                current.fencing_token, current.status
            )),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_leader_candidate_set_is_rejected() {
        let store = InMemoryLeaseStore::default();
        let err = store
            .elect_leader(LeaderElectionRequest {
                group: "workers".to_string(),
                candidates: vec![],
                now_ms: 0,
                ttl_ms: 10,
            })
            .expect_err("empty candidates");

        assert!(matches!(err, LeaseError::EmptyCandidateSet { .. }));
    }
}
