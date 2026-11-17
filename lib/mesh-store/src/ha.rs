use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod scope;
pub use scope::{AuthorityScope, AuthorityScopeError};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HaResourceRef {
    pub scope: String,
    pub namespace: String,
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespaceRegistration {
    pub scope: String,
    pub namespace: String,
    pub schema_version: u32,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespaceRecord {
    pub scope: String,
    pub namespace: String,
    pub schema_version: u32,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationRecord {
    pub scope: String,
    pub namespace: String,
    pub from_schema_version: u32,
    pub to_schema_version: u32,
    pub migration_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpectedVersion {
    Absent,
    Exact(u64),
    Any,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateWrite {
    pub resource: HaResourceRef,
    pub expected_version: ExpectedVersion,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateRecord {
    pub resource: HaResourceRef,
    pub version: u64,
    pub payload: Value,
    pub transaction_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HaEventAppend {
    pub stream_id: String,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HaEventRecord {
    pub event_id: String,
    pub stream_id: String,
    pub cursor: u64,
    pub kind: String,
    pub payload: Value,
    pub transaction_id: String,
    pub transaction_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalEffectState {
    Pending,
    Dispatching,
    ProviderCallStarted,
    Acknowledged,
    UnknownOutcome,
    Compensating,
    FailedTerminal,
    Completed,
    CompletedFollowupFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectAttemptRecord {
    pub attempt_id: String,
    pub dispatcher_owner_id: String,
    pub lease_name: String,
    pub fencing_token: u64,
    pub started_at_ms: u64,
    pub provider_call_started_at_ms: Option<u64>,
    pub provider_idempotency_key: Option<String>,
    pub request_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalEffectIntent {
    pub scope: String,
    pub namespace: String,
    pub effect_id: String,
    #[serde(default)]
    pub version: u64,
    pub effect_group_id: Option<String>,
    pub source_transaction_id: String,
    pub provider: String,
    pub operation: String,
    pub target_ref: Option<HaResourceRef>,
    pub provider_idempotency_key: Option<String>,
    pub request_hash: String,
    pub state: ExternalEffectState,
    pub attempt: Option<EffectAttemptRecord>,
    pub retry_count: u64,
    pub compensation_count: u64,
    pub provider_operation_ref: Option<String>,
    pub observed_provider_state: Option<String>,
    pub reconciliation_evidence_ref: Option<String>,
    pub reconciliation_required_by_ms: Option<u64>,
    pub unknown_outcome_reason: Option<String>,
    pub terminal_reason: Option<String>,
    pub recovery_incident_refs: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

impl ExternalEffectIntent {
    pub fn current_attempt(&self) -> Option<&EffectAttemptRecord> {
        self.attempt.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionReservation {
    pub scope: String,
    pub namespace: String,
    pub transaction_id: String,
    pub idempotency_key: Option<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    Reserved,
    Committed,
    Aborted,
    InDoubt,
    Corrupt,
}

impl TransactionStatus {
    pub fn is_aborted(self) -> bool {
        self == Self::Aborted
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub scope: String,
    pub namespace: String,
    pub transaction_id: String,
    pub sequence: u64,
    pub reservation_token: u64,
    pub idempotency_key: Option<String>,
    pub fingerprint: String,
    pub status: TransactionStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandResultWrite {
    pub idempotency_key: String,
    pub fingerprint: String,
    pub result: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandResultEvidence {
    pub scope: String,
    pub namespace: String,
    pub transaction_id: String,
    pub transaction_sequence: u64,
    pub finality_watermark: u64,
    pub idempotency_key: String,
    pub fingerprint: String,
    pub result: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitEnvelope {
    pub scope: String,
    pub namespace: String,
    pub transaction_id: String,
    pub reservation_token: u64,
    pub required_schema_version: u32,
    pub required_finality_watermark: u64,
    pub state_writes: Vec<StateWrite>,
    pub events: Vec<HaEventAppend>,
    #[serde(default)]
    pub external_effect_intents: Vec<ExternalEffectIntent>,
    #[serde(default)]
    pub command_result: Option<CommandResultWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitOutcome {
    pub transaction_id: String,
    pub transaction_sequence: u64,
    pub finality_watermark: u64,
    pub state_versions: Vec<(HaResourceRef, u64)>,
    pub event_cursors: Vec<(String, u64)>,
    pub effect_intent_ids: Vec<String>,
    pub command_result_ref: Option<HaResourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionConflictError {
    pub resource: HaResourceRef,
    pub expected: ExpectedVersion,
    pub actual: Option<u64>,
}

impl std::fmt::Display for VersionConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "version conflict for {:?}: expected {:?}, actual {:?}",
            self.resource, self.expected, self.actual
        )
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum HaStorageError {
    #[error("namespace {scope}/{namespace} is unavailable")]
    NamespaceUnavailable { scope: String, namespace: String },
    #[error(
        "namespace {scope}/{namespace} schema incompatible: required {required}, actual {actual}"
    )]
    SchemaIncompatible {
        scope: String,
        namespace: String,
        required: u32,
        actual: u32,
    },
    #[error("migration conflict for {scope}/{namespace}: expected {expected}, actual {actual}")]
    MigrationConflict {
        scope: String,
        namespace: String,
        expected: u32,
        actual: u32,
    },
    #[error("{0}")]
    VersionConflict(Box<VersionConflictError>),
    #[error("transaction {scope}/{namespace}/{transaction_id} not found")]
    TransactionNotFound {
        scope: String,
        namespace: String,
        transaction_id: String,
    },
    #[error("transaction {transaction_id} has status {status:?}")]
    TransactionConflict {
        transaction_id: String,
        status: TransactionStatus,
    },
    #[error("stale reservation token for transaction {transaction_id}")]
    StaleReservation { transaction_id: String },
    #[error("idempotency conflict for key {idempotency_key}")]
    IdempotencyConflict { idempotency_key: String },
    #[error(
        "finality watermark too low for {scope}/{namespace}: required {required}, actual {actual}"
    )]
    StaleFinalityWatermark {
        scope: String,
        namespace: String,
        required: u64,
        actual: u64,
    },
    #[error("backend unavailable: {message}")]
    BackendUnavailable { message: String },
    #[error("commit corrupt: {message}")]
    CommitCorrupt { message: String },
}

impl HaStorageError {
    pub fn is_version_conflict(&self) -> bool {
        matches!(self, Self::VersionConflict { .. })
    }

    pub fn is_schema_incompatible(&self) -> bool {
        matches!(self, Self::SchemaIncompatible { .. })
    }
}

pub type HaStorageResult<T> = Result<T, HaStorageError>;

pub trait HaStorage {
    fn register_namespace(
        &self,
        registration: NamespaceRegistration,
    ) -> HaStorageResult<NamespaceRecord>;
    fn namespace_record(
        &self,
        scope: &str,
        namespace: &str,
    ) -> HaStorageResult<Option<NamespaceRecord>>;
    fn record_migration(
        &self,
        scope: &str,
        namespace: &str,
        from_schema_version: u32,
        to_schema_version: u32,
        migration_id: &str,
    ) -> HaStorageResult<MigrationRecord>;
    fn reserve_transaction(
        &self,
        reservation: TransactionReservation,
    ) -> HaStorageResult<TransactionRecord>;
    fn commit(&self, envelope: CommitEnvelope) -> HaStorageResult<CommitOutcome>;
    fn finalize_no_commit(
        &self,
        scope: &str,
        namespace: &str,
        transaction_id: &str,
        reservation_token: u64,
    ) -> HaStorageResult<TransactionRecord>;
    fn transaction_record(
        &self,
        scope: &str,
        namespace: &str,
        transaction_id: &str,
    ) -> HaStorageResult<Option<TransactionRecord>>;
    fn finality_watermark(&self, scope: &str, namespace: &str) -> HaStorageResult<u64>;
    fn load_state(&self, resource: &HaResourceRef) -> HaStorageResult<Option<StateRecord>>;
    fn read_events(
        &self,
        stream_id: &str,
        after_cursor: u64,
    ) -> HaStorageResult<Vec<HaEventRecord>>;
}

#[derive(Debug, Default, Clone)]
pub struct InMemoryHaStorage {
    inner: Arc<Mutex<InMemoryHaState>>,
}

#[derive(Debug, Default)]
struct InMemoryHaState {
    namespaces: BTreeMap<(String, String), NamespaceRecord>,
    migrations: Vec<MigrationRecord>,
    transactions: BTreeMap<(String, String, String), TransactionRecord>,
    idempotency: BTreeMap<(String, String, String), (String, String)>,
    records: BTreeMap<HaResourceRef, StateRecord>,
    events: BTreeMap<String, Vec<HaEventRecord>>,
    finality_watermarks: BTreeMap<(String, String), u64>,
    next_transaction_sequence: u64,
}

impl HaStorage for InMemoryHaStorage {
    fn register_namespace(
        &self,
        registration: NamespaceRegistration,
    ) -> HaStorageResult<NamespaceRecord> {
        let record = NamespaceRecord {
            scope: registration.scope,
            namespace: registration.namespace,
            schema_version: registration.schema_version,
            active: registration.active,
        };
        let key = (record.scope.clone(), record.namespace.clone());
        let mut inner = self.lock()?;
        inner.namespaces.insert(key.clone(), record.clone());
        inner.finality_watermarks.entry(key).or_insert(0);
        Ok(record)
    }

    fn namespace_record(
        &self,
        scope: &str,
        namespace: &str,
    ) -> HaStorageResult<Option<NamespaceRecord>> {
        Ok(self
            .lock()?
            .namespaces
            .get(&(scope.to_string(), namespace.to_string()))
            .cloned())
    }

    fn record_migration(
        &self,
        scope: &str,
        namespace: &str,
        from_schema_version: u32,
        to_schema_version: u32,
        migration_id: &str,
    ) -> HaStorageResult<MigrationRecord> {
        let mut inner = self.lock()?;
        let namespace_key = (scope.to_string(), namespace.to_string());
        let Some(namespace_record) = inner.namespaces.get_mut(&namespace_key) else {
            return Err(HaStorageError::NamespaceUnavailable {
                scope: scope.to_string(),
                namespace: namespace.to_string(),
            });
        };
        if namespace_record.schema_version != from_schema_version {
            return Err(HaStorageError::MigrationConflict {
                scope: scope.to_string(),
                namespace: namespace.to_string(),
                expected: from_schema_version,
                actual: namespace_record.schema_version,
            });
        }

        namespace_record.schema_version = to_schema_version;
        let record = MigrationRecord {
            scope: scope.to_string(),
            namespace: namespace.to_string(),
            from_schema_version,
            to_schema_version,
            migration_id: migration_id.to_string(),
        };
        inner.migrations.push(record.clone());
        Ok(record)
    }

    fn reserve_transaction(
        &self,
        reservation: TransactionReservation,
    ) -> HaStorageResult<TransactionRecord> {
        let mut inner = self.lock()?;
        ensure_namespace_active(&inner, &reservation.scope, &reservation.namespace)?;

        if let Some(idempotency_key) = &reservation.idempotency_key {
            let key = (
                reservation.scope.clone(),
                reservation.namespace.clone(),
                idempotency_key.clone(),
            );
            if let Some((existing_transaction_id, existing_fingerprint)) =
                inner.idempotency.get(&key)
            {
                if existing_fingerprint != &reservation.fingerprint {
                    return Err(HaStorageError::IdempotencyConflict {
                        idempotency_key: idempotency_key.clone(),
                    });
                }
                let tx_key = (
                    reservation.scope,
                    reservation.namespace,
                    existing_transaction_id.clone(),
                );
                return inner.transactions.get(&tx_key).cloned().ok_or_else(|| {
                    HaStorageError::CommitCorrupt {
                        message: format!(
                            "idempotency key {idempotency_key} points to missing transaction {existing_transaction_id}"
                        ),
                    }
                });
            }
            inner.idempotency.insert(
                key,
                (
                    reservation.transaction_id.clone(),
                    reservation.fingerprint.clone(),
                ),
            );
        }

        let transaction_key = (
            reservation.scope.clone(),
            reservation.namespace.clone(),
            reservation.transaction_id.clone(),
        );
        if let Some(existing) = inner.transactions.get(&transaction_key) {
            return Ok(existing.clone());
        }

        inner.next_transaction_sequence += 1;
        let sequence = inner.next_transaction_sequence;
        let record = TransactionRecord {
            scope: reservation.scope,
            namespace: reservation.namespace,
            transaction_id: reservation.transaction_id,
            sequence,
            reservation_token: sequence.saturating_mul(2).saturating_add(1),
            idempotency_key: reservation.idempotency_key,
            fingerprint: reservation.fingerprint,
            status: TransactionStatus::Reserved,
        };
        inner.transactions.insert(transaction_key, record.clone());
        Ok(record)
    }

    fn commit(&self, envelope: CommitEnvelope) -> HaStorageResult<CommitOutcome> {
        let mut inner = self.lock()?;
        let namespace = ensure_namespace_active(&inner, &envelope.scope, &envelope.namespace)?;
        if namespace.schema_version != envelope.required_schema_version {
            return Err(HaStorageError::SchemaIncompatible {
                scope: envelope.scope,
                namespace: envelope.namespace,
                required: envelope.required_schema_version,
                actual: namespace.schema_version,
            });
        }

        let finality_key = (envelope.scope.clone(), envelope.namespace.clone());
        let current_finality = inner
            .finality_watermarks
            .get(&finality_key)
            .copied()
            .unwrap_or(0);
        if current_finality < envelope.required_finality_watermark {
            return Err(HaStorageError::StaleFinalityWatermark {
                scope: envelope.scope,
                namespace: envelope.namespace,
                required: envelope.required_finality_watermark,
                actual: current_finality,
            });
        }

        let tx_key = (
            envelope.scope.clone(),
            envelope.namespace.clone(),
            envelope.transaction_id.clone(),
        );
        let tx = inner.transactions.get(&tx_key).cloned().ok_or_else(|| {
            HaStorageError::TransactionNotFound {
                scope: envelope.scope.clone(),
                namespace: envelope.namespace.clone(),
                transaction_id: envelope.transaction_id.clone(),
            }
        })?;
        if tx.reservation_token != envelope.reservation_token {
            return Err(HaStorageError::StaleReservation {
                transaction_id: envelope.transaction_id,
            });
        }
        if tx.status != TransactionStatus::Reserved {
            return Err(HaStorageError::TransactionConflict {
                transaction_id: envelope.transaction_id,
                status: tx.status,
            });
        }

        let mut proposed_records = inner.records.clone();
        let mut state_versions = Vec::with_capacity(envelope.state_writes.len());
        for write in &envelope.state_writes {
            let actual = proposed_records
                .get(&write.resource)
                .map(|record| record.version);
            validate_expected_version(&write.resource, &write.expected_version, actual)?;
            let next_version = actual.unwrap_or(0) + 1;
            proposed_records.insert(
                write.resource.clone(),
                StateRecord {
                    resource: write.resource.clone(),
                    version: next_version,
                    payload: write.payload.clone(),
                    transaction_id: envelope.transaction_id.clone(),
                },
            );
            state_versions.push((write.resource.clone(), next_version));
        }

        let mut effect_intent_ids = Vec::with_capacity(envelope.external_effect_intents.len());
        for intent in &envelope.external_effect_intents {
            if intent.scope != envelope.scope || intent.namespace != envelope.namespace {
                return Err(HaStorageError::CommitCorrupt {
                    message: format!(
                        "external effect intent {}/{} is outside commit authority {}/{}",
                        intent.scope, intent.namespace, envelope.scope, envelope.namespace
                    ),
                });
            }
            if intent.state != ExternalEffectState::Pending {
                return Err(HaStorageError::CommitCorrupt {
                    message: format!(
                        "external effect intent {} must start in pending state",
                        intent.effect_id
                    ),
                });
            }
            let resource =
                external_effect_resource(&envelope.scope, &envelope.namespace, &intent.effect_id);
            let actual = proposed_records.get(&resource).map(|record| record.version);
            validate_expected_version(&resource, &ExpectedVersion::Absent, actual)?;
            let mut stored_intent = intent.clone();
            stored_intent.version = 1;
            proposed_records.insert(
                resource.clone(),
                StateRecord {
                    resource: resource.clone(),
                    version: 1,
                    payload: serde_json::to_value(&stored_intent).map_err(|error| {
                        HaStorageError::BackendUnavailable {
                            message: format!(
                                "serialize external effect intent {}: {error}",
                                intent.effect_id
                            ),
                        }
                    })?,
                    transaction_id: envelope.transaction_id.clone(),
                },
            );
            state_versions.push((resource, 1));
            effect_intent_ids.push(intent.effect_id.clone());
        }

        let mut event_cursors =
            Vec::with_capacity(envelope.events.len() + envelope.external_effect_intents.len());
        let mut proposed_events = inner.events.clone();
        for event in &envelope.events {
            let stream = proposed_events.entry(event.stream_id.clone()).or_default();
            let cursor = stream.len() as u64 + 1;
            stream.push(HaEventRecord {
                event_id: format!("{}:{}", event.stream_id, cursor),
                stream_id: event.stream_id.clone(),
                cursor,
                kind: event.kind.clone(),
                payload: event.payload.clone(),
                transaction_id: envelope.transaction_id.clone(),
                transaction_sequence: tx.sequence,
            });
            event_cursors.push((event.stream_id.clone(), cursor));
        }
        for intent in &envelope.external_effect_intents {
            let stream_id =
                external_effect_stream_id(&envelope.scope, &envelope.namespace, &intent.effect_id);
            let stream = proposed_events.entry(stream_id.clone()).or_default();
            let cursor = stream.len() as u64 + 1;
            let mut stored_intent = intent.clone();
            stored_intent.version = 1;
            stream.push(HaEventRecord {
                event_id: format!("{stream_id}:{cursor}"),
                stream_id: stream_id.clone(),
                cursor,
                kind: "effect_pending".to_string(),
                payload: serde_json::to_value(&stored_intent).map_err(|error| {
                    HaStorageError::BackendUnavailable {
                        message: format!(
                            "serialize external effect pending event {}: {error}",
                            intent.effect_id
                        ),
                    }
                })?,
                transaction_id: envelope.transaction_id.clone(),
                transaction_sequence: tx.sequence,
            });
            event_cursors.push((stream_id, cursor));
        }

        let finality_watermark = current_finality.max(tx.sequence);
        let command_result_ref = if let Some(command_result) = &envelope.command_result {
            if tx.idempotency_key.as_deref() != Some(command_result.idempotency_key.as_str()) {
                return Err(HaStorageError::CommitCorrupt {
                    message: format!(
                        "command result idempotency key {} does not match transaction idempotency key {:?}",
                        command_result.idempotency_key, tx.idempotency_key
                    ),
                });
            }
            if tx.fingerprint != command_result.fingerprint {
                return Err(HaStorageError::CommitCorrupt {
                    message: "command result fingerprint does not match transaction fingerprint"
                        .to_string(),
                });
            }
            let resource = command_result_resource(
                &envelope.scope,
                &envelope.namespace,
                &command_result.idempotency_key,
            );
            let actual = proposed_records.get(&resource).map(|record| record.version);
            validate_expected_version(&resource, &ExpectedVersion::Absent, actual)?;
            let evidence = CommandResultEvidence {
                scope: envelope.scope.clone(),
                namespace: envelope.namespace.clone(),
                transaction_id: envelope.transaction_id.clone(),
                transaction_sequence: tx.sequence,
                finality_watermark,
                idempotency_key: command_result.idempotency_key.clone(),
                fingerprint: command_result.fingerprint.clone(),
                result: command_result.result.clone(),
            };
            proposed_records.insert(
                resource.clone(),
                StateRecord {
                    resource: resource.clone(),
                    version: 1,
                    payload: serde_json::to_value(&evidence).map_err(|error| {
                        HaStorageError::BackendUnavailable {
                            message: format!(
                                "serialize command result evidence {}: {error}",
                                evidence.idempotency_key
                            ),
                        }
                    })?,
                    transaction_id: envelope.transaction_id.clone(),
                },
            );
            state_versions.push((resource.clone(), 1));
            Some(resource)
        } else {
            None
        };

        inner.records = proposed_records;
        inner.events = proposed_events;
        inner
            .finality_watermarks
            .insert(finality_key, finality_watermark);
        let committed_tx =
            inner
                .transactions
                .get_mut(&tx_key)
                .ok_or_else(|| HaStorageError::CommitCorrupt {
                    message: format!(
                        "transaction disappeared before commit: {}",
                        tx.transaction_id
                    ),
                })?;
        committed_tx.status = TransactionStatus::Committed;

        Ok(CommitOutcome {
            transaction_id: tx.transaction_id,
            transaction_sequence: tx.sequence,
            finality_watermark,
            state_versions,
            event_cursors,
            effect_intent_ids,
            command_result_ref,
        })
    }

    fn finalize_no_commit(
        &self,
        scope: &str,
        namespace: &str,
        transaction_id: &str,
        reservation_token: u64,
    ) -> HaStorageResult<TransactionRecord> {
        let mut inner = self.lock()?;
        let tx_key = (
            scope.to_string(),
            namespace.to_string(),
            transaction_id.to_string(),
        );
        let tx = inner.transactions.get_mut(&tx_key).ok_or_else(|| {
            HaStorageError::TransactionNotFound {
                scope: scope.to_string(),
                namespace: namespace.to_string(),
                transaction_id: transaction_id.to_string(),
            }
        })?;
        if tx.reservation_token != reservation_token {
            return Err(HaStorageError::StaleReservation {
                transaction_id: transaction_id.to_string(),
            });
        }
        if tx.status != TransactionStatus::Reserved {
            return Err(HaStorageError::TransactionConflict {
                transaction_id: transaction_id.to_string(),
                status: tx.status,
            });
        }
        tx.status = TransactionStatus::Aborted;
        let record = tx.clone();
        let finality_key = (scope.to_string(), namespace.to_string());
        let current_finality = inner
            .finality_watermarks
            .get(&finality_key)
            .copied()
            .unwrap_or(0);
        inner
            .finality_watermarks
            .insert(finality_key, current_finality.max(record.sequence));
        Ok(record)
    }

    fn transaction_record(
        &self,
        scope: &str,
        namespace: &str,
        transaction_id: &str,
    ) -> HaStorageResult<Option<TransactionRecord>> {
        Ok(self
            .lock()?
            .transactions
            .get(&(
                scope.to_string(),
                namespace.to_string(),
                transaction_id.to_string(),
            ))
            .cloned())
    }

    fn finality_watermark(&self, scope: &str, namespace: &str) -> HaStorageResult<u64> {
        Ok(self
            .lock()?
            .finality_watermarks
            .get(&(scope.to_string(), namespace.to_string()))
            .copied()
            .unwrap_or(0))
    }

    fn load_state(&self, resource: &HaResourceRef) -> HaStorageResult<Option<StateRecord>> {
        Ok(self.lock()?.records.get(resource).cloned())
    }

    fn read_events(
        &self,
        stream_id: &str,
        after_cursor: u64,
    ) -> HaStorageResult<Vec<HaEventRecord>> {
        Ok(self
            .lock()?
            .events
            .get(stream_id)
            .map(|records| {
                records
                    .iter()
                    .filter(|record| record.cursor > after_cursor)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl InMemoryHaStorage {
    fn lock(&self) -> HaStorageResult<std::sync::MutexGuard<'_, InMemoryHaState>> {
        self.inner
            .lock()
            .map_err(|_| HaStorageError::BackendUnavailable {
                message: "in-memory HA storage mutex poisoned".to_string(),
            })
    }
}

fn ensure_namespace_active<'a>(
    inner: &'a InMemoryHaState,
    scope: &str,
    namespace: &str,
) -> HaStorageResult<&'a NamespaceRecord> {
    let record = inner
        .namespaces
        .get(&(scope.to_string(), namespace.to_string()))
        .ok_or_else(|| HaStorageError::NamespaceUnavailable {
            scope: scope.to_string(),
            namespace: namespace.to_string(),
        })?;
    if !record.active {
        return Err(HaStorageError::NamespaceUnavailable {
            scope: scope.to_string(),
            namespace: namespace.to_string(),
        });
    }
    Ok(record)
}

fn validate_expected_version(
    resource: &HaResourceRef,
    expected: &ExpectedVersion,
    actual: Option<u64>,
) -> HaStorageResult<()> {
    let matches = match expected {
        ExpectedVersion::Absent => actual.is_none(),
        ExpectedVersion::Exact(expected) => actual == Some(*expected),
        ExpectedVersion::Any => true,
    };
    if matches {
        return Ok(());
    }
    Err(HaStorageError::VersionConflict(Box::new(
        VersionConflictError {
            resource: resource.clone(),
            expected: expected.clone(),
            actual,
        },
    )))
}

pub fn command_result_resource(
    scope: &str,
    namespace: &str,
    idempotency_key: &str,
) -> HaResourceRef {
    HaResourceRef {
        scope: scope.to_string(),
        namespace: namespace.to_string(),
        kind: "command_result".to_string(),
        id: idempotency_key.to_string(),
    }
}

pub fn external_effect_resource(scope: &str, namespace: &str, effect_id: &str) -> HaResourceRef {
    HaResourceRef {
        scope: scope.to_string(),
        namespace: namespace.to_string(),
        kind: "external_effect_intent".to_string(),
        id: effect_id.to_string(),
    }
}

pub fn external_effect_stream_id(scope: &str, namespace: &str, effect_id: &str) -> String {
    format!("mesh-effect:{scope}:{namespace}:{effect_id}")
}

pub mod conformance {
    use super::{
        CommitEnvelope, ExpectedVersion, HaEventAppend, HaResourceRef, HaStorage,
        NamespaceRegistration, StateWrite, TransactionReservation,
    };
    use serde_json::json;

    pub fn run_ha_storage_conformance<S, F>(factory: F)
    where
        S: HaStorage,
        F: Fn() -> S,
    {
        commit_is_atomic_when_any_cas_check_fails(factory());
        commits_assign_versions_events_and_finality_watermark(factory());
        recovery_finalizes_reserved_transactions_without_state_or_events(factory());
        namespace_schema_migrations_are_checked_at_commit_time(factory());
    }

    fn registered_store<S: HaStorage>(store: S) -> S {
        store
            .register_namespace(NamespaceRegistration {
                scope: "workspace:mesh".to_string(),
                namespace: "guildmesh".to_string(),
                schema_version: 1,
                active: true,
            })
            .expect("register namespace");
        store
    }

    fn resource(id: &str) -> HaResourceRef {
        HaResourceRef {
            scope: "workspace:mesh".to_string(),
            namespace: "guildmesh".to_string(),
            kind: "room".to_string(),
            id: id.to_string(),
        }
    }

    fn reserve<S: HaStorage>(store: &S, tx: &str) -> u64 {
        store
            .reserve_transaction(TransactionReservation {
                scope: "workspace:mesh".to_string(),
                namespace: "guildmesh".to_string(),
                transaction_id: tx.to_string(),
                idempotency_key: Some(tx.to_string()),
                fingerprint: format!("fingerprint:{tx}"),
            })
            .expect("reserve transaction")
            .reservation_token
    }

    fn commit_is_atomic_when_any_cas_check_fails<S: HaStorage>(store: S) {
        let store = registered_store(store);
        let existing = resource("room-1");
        let missing = resource("room-2");

        let token = reserve(&store, "tx-seed");
        store
            .commit(CommitEnvelope {
                scope: "workspace:mesh".to_string(),
                namespace: "guildmesh".to_string(),
                transaction_id: "tx-seed".to_string(),
                reservation_token: token,
                required_schema_version: 1,
                required_finality_watermark: 0,
                state_writes: vec![StateWrite {
                    resource: existing.clone(),
                    expected_version: ExpectedVersion::Absent,
                    payload: json!({"name": "alpha"}),
                }],
                events: vec![],
                external_effect_intents: vec![],
                command_result: None,
            })
            .expect("seed commit");

        let token = reserve(&store, "tx-conflict");
        let err = store
            .commit(CommitEnvelope {
                scope: "workspace:mesh".to_string(),
                namespace: "guildmesh".to_string(),
                transaction_id: "tx-conflict".to_string(),
                reservation_token: token,
                required_schema_version: 1,
                required_finality_watermark: 0,
                state_writes: vec![
                    StateWrite {
                        resource: missing.clone(),
                        expected_version: ExpectedVersion::Absent,
                        payload: json!({"name": "beta"}),
                    },
                    StateWrite {
                        resource: existing,
                        expected_version: ExpectedVersion::Absent,
                        payload: json!({"name": "should-not-write"}),
                    },
                ],
                events: vec![HaEventAppend {
                    stream_id: "guildmesh.room.room-2".to_string(),
                    kind: "room.created".to_string(),
                    payload: json!({"room_id": "room-2"}),
                }],
                external_effect_intents: vec![],
                command_result: None,
            })
            .expect_err("CAS conflict");

        assert!(err.is_version_conflict());
        assert!(store.load_state(&missing).expect("load").is_none());
        assert!(
            store
                .read_events("guildmesh.room.room-2", 0)
                .expect("events")
                .is_empty()
        );
    }

    fn commits_assign_versions_events_and_finality_watermark<S: HaStorage>(store: S) {
        let store = registered_store(store);
        let room = resource("room-1");
        let token = reserve(&store, "tx-create");

        let outcome = store
            .commit(CommitEnvelope {
                scope: "workspace:mesh".to_string(),
                namespace: "guildmesh".to_string(),
                transaction_id: "tx-create".to_string(),
                reservation_token: token,
                required_schema_version: 1,
                required_finality_watermark: 0,
                state_writes: vec![StateWrite {
                    resource: room.clone(),
                    expected_version: ExpectedVersion::Absent,
                    payload: json!({"name": "alpha"}),
                }],
                events: vec![HaEventAppend {
                    stream_id: "guildmesh.room.room-1".to_string(),
                    kind: "room.created".to_string(),
                    payload: json!({"room_id": "room-1"}),
                }],
                external_effect_intents: vec![],
                command_result: None,
            })
            .expect("commit");

        let state = store.load_state(&room).expect("load").expect("state");
        let events = store
            .read_events("guildmesh.room.room-1", 0)
            .expect("read events");
        let tx = store
            .transaction_record("workspace:mesh", "guildmesh", "tx-create")
            .expect("tx")
            .expect("record");

        assert_eq!(state.version, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].cursor, 1);
        assert_eq!(outcome.finality_watermark, tx.sequence);
        assert_eq!(
            store
                .finality_watermark("workspace:mesh", "guildmesh")
                .expect("watermark"),
            tx.sequence
        );
    }

    fn recovery_finalizes_reserved_transactions_without_state_or_events<S: HaStorage>(store: S) {
        let store = registered_store(store);
        let token = reserve(&store, "tx-abort");

        store
            .finalize_no_commit("workspace:mesh", "guildmesh", "tx-abort", token)
            .expect("finalize");

        let tx = store
            .transaction_record("workspace:mesh", "guildmesh", "tx-abort")
            .expect("tx")
            .expect("record");

        assert!(tx.status.is_aborted());
        assert_eq!(
            store
                .finality_watermark("workspace:mesh", "guildmesh")
                .expect("watermark"),
            tx.sequence
        );
        assert!(store.read_events("any", 0).expect("events").is_empty());
    }

    fn namespace_schema_migrations_are_checked_at_commit_time<S: HaStorage>(store: S) {
        let store = registered_store(store);
        store
            .record_migration("workspace:mesh", "guildmesh", 1, 2, "guildmesh-v2")
            .expect("migration");

        let token = reserve(&store, "tx-old-schema");
        let err = store
            .commit(CommitEnvelope {
                scope: "workspace:mesh".to_string(),
                namespace: "guildmesh".to_string(),
                transaction_id: "tx-old-schema".to_string(),
                reservation_token: token,
                required_schema_version: 1,
                required_finality_watermark: 0,
                state_writes: vec![StateWrite {
                    resource: resource("room-1"),
                    expected_version: ExpectedVersion::Absent,
                    payload: json!({"name": "alpha"}),
                }],
                events: vec![],
                external_effect_intents: vec![],
                command_result: None,
            })
            .expect_err("schema conflict");

        assert!(err.is_schema_incompatible());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn idempotency_key_reuse_with_different_fingerprint_is_rejected() {
        let store = InMemoryHaStorage::default();
        store
            .register_namespace(NamespaceRegistration {
                scope: "workspace:mesh".to_string(),
                namespace: "agentmesh".to_string(),
                schema_version: 1,
                active: true,
            })
            .expect("namespace");

        store
            .reserve_transaction(TransactionReservation {
                scope: "workspace:mesh".to_string(),
                namespace: "agentmesh".to_string(),
                transaction_id: "tx-1".to_string(),
                idempotency_key: Some("key-1".to_string()),
                fingerprint: "a".to_string(),
            })
            .expect("first reservation");

        let err = store
            .reserve_transaction(TransactionReservation {
                scope: "workspace:mesh".to_string(),
                namespace: "agentmesh".to_string(),
                transaction_id: "tx-2".to_string(),
                idempotency_key: Some("key-1".to_string()),
                fingerprint: "b".to_string(),
            })
            .expect_err("idempotency conflict");

        assert!(matches!(err, HaStorageError::IdempotencyConflict { .. }));
    }

    #[test]
    fn exact_version_updates_increment_from_existing_version() {
        let store = InMemoryHaStorage::default();
        store
            .register_namespace(NamespaceRegistration {
                scope: "workspace:mesh".to_string(),
                namespace: "shipmesh".to_string(),
                schema_version: 1,
                active: true,
            })
            .expect("namespace");
        let resource = HaResourceRef {
            scope: "workspace:mesh".to_string(),
            namespace: "shipmesh".to_string(),
            kind: "deployment".to_string(),
            id: "deploy-1".to_string(),
        };

        for (idx, expected_version) in [ExpectedVersion::Absent, ExpectedVersion::Exact(1)]
            .into_iter()
            .enumerate()
        {
            let tx_id = format!("tx-{idx}");
            let reservation = store
                .reserve_transaction(TransactionReservation {
                    scope: "workspace:mesh".to_string(),
                    namespace: "shipmesh".to_string(),
                    transaction_id: tx_id.clone(),
                    idempotency_key: Some(tx_id.clone()),
                    fingerprint: tx_id.clone(),
                })
                .expect("reservation");
            store
                .commit(CommitEnvelope {
                    scope: "workspace:mesh".to_string(),
                    namespace: "shipmesh".to_string(),
                    transaction_id: tx_id,
                    reservation_token: reservation.reservation_token,
                    required_schema_version: 1,
                    required_finality_watermark: 0,
                    state_writes: vec![StateWrite {
                        resource: resource.clone(),
                        expected_version,
                        payload: json!({"idx": idx}),
                    }],
                    events: vec![],
                    external_effect_intents: vec![],
                    command_result: None,
                })
                .expect("commit");
        }

        assert_eq!(
            store.load_state(&resource).expect("load").unwrap().version,
            2
        );
    }

    #[test]
    fn any_expected_version_allows_insert_without_cas() {
        let store = InMemoryHaStorage::default();
        store
            .register_namespace(NamespaceRegistration {
                scope: "workspace:mesh".to_string(),
                namespace: "workmesh".to_string(),
                schema_version: 1,
                active: true,
            })
            .expect("namespace");
        let resource = HaResourceRef {
            scope: "workspace:mesh".to_string(),
            namespace: "workmesh".to_string(),
            kind: "task".to_string(),
            id: "task-1".to_string(),
        };
        let reservation = store
            .reserve_transaction(TransactionReservation {
                scope: "workspace:mesh".to_string(),
                namespace: "workmesh".to_string(),
                transaction_id: "tx-any".to_string(),
                idempotency_key: Some("tx-any".to_string()),
                fingerprint: "tx-any".to_string(),
            })
            .expect("reservation");

        store
            .commit(CommitEnvelope {
                scope: "workspace:mesh".to_string(),
                namespace: "workmesh".to_string(),
                transaction_id: "tx-any".to_string(),
                reservation_token: reservation.reservation_token,
                required_schema_version: 1,
                required_finality_watermark: 0,
                state_writes: vec![StateWrite {
                    resource: resource.clone(),
                    expected_version: ExpectedVersion::Any,
                    payload: json!({"title": "alpha"}),
                }],
                events: vec![],
                external_effect_intents: vec![],
                command_result: None,
            })
            .expect("commit");

        assert_eq!(
            store.load_state(&resource).expect("load").unwrap().version,
            1
        );
    }
}
