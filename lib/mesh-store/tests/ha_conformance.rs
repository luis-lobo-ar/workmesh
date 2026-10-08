use mesh_store::ha::{
    CommandResultEvidence, CommandResultWrite, CommitEnvelope, ExpectedVersion,
    ExternalEffectIntent, ExternalEffectState, HaEventAppend, HaResourceRef, HaStorage,
    InMemoryHaStorage, NamespaceRegistration, StateWrite, TransactionReservation,
    command_result_resource, conformance, external_effect_resource, external_effect_stream_id,
};
use serde_json::json;

fn registered_store() -> InMemoryHaStorage {
    let store = InMemoryHaStorage::default();
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

fn reserve(store: &InMemoryHaStorage, tx: &str) -> u64 {
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

#[test]
fn commit_is_atomic_when_any_cas_check_fails() {
    let store = registered_store();
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
                    resource: existing.clone(),
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

#[test]
fn commits_assign_versions_events_and_finality_watermark() {
    let store = registered_store();
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

#[test]
fn commit_envelope_atomically_records_command_result_evidence() {
    let store = registered_store();
    let token = store
        .reserve_transaction(TransactionReservation {
            scope: "workspace:mesh".to_string(),
            namespace: "guildmesh".to_string(),
            transaction_id: "tx-result".to_string(),
            idempotency_key: Some("key-result".to_string()),
            fingerprint: "fingerprint-result".to_string(),
        })
        .expect("reserve transaction")
        .reservation_token;

    let outcome = store
        .commit(CommitEnvelope {
            scope: "workspace:mesh".to_string(),
            namespace: "guildmesh".to_string(),
            transaction_id: "tx-result".to_string(),
            reservation_token: token,
            required_schema_version: 1,
            required_finality_watermark: 0,
            state_writes: vec![StateWrite {
                resource: resource("room-result"),
                expected_version: ExpectedVersion::Absent,
                payload: json!({"name": "result"}),
            }],
            events: vec![HaEventAppend {
                stream_id: "guildmesh.room.room-result".to_string(),
                kind: "room.created".to_string(),
                payload: json!({"room_id": "room-result"}),
            }],
            external_effect_intents: vec![],
            command_result: Some(CommandResultWrite {
                idempotency_key: "key-result".to_string(),
                fingerprint: "fingerprint-result".to_string(),
                result: json!({"ok": true, "room_id": "room-result"}),
            }),
        })
        .expect("commit with command result");

    let result_ref = command_result_resource("workspace:mesh", "guildmesh", "key-result");
    assert_eq!(outcome.command_result_ref, Some(result_ref.clone()));
    let evidence: CommandResultEvidence = serde_json::from_value(
        store
            .load_state(&result_ref)
            .expect("load command result")
            .expect("command result evidence")
            .payload,
    )
    .expect("parse command result evidence");

    assert_eq!(evidence.transaction_id, "tx-result");
    assert_eq!(evidence.idempotency_key, "key-result");
    assert_eq!(evidence.fingerprint, "fingerprint-result");
    assert_eq!(evidence.finality_watermark, outcome.finality_watermark);
    assert_eq!(
        evidence.result,
        json!({"ok": true, "room_id": "room-result"})
    );
}

#[test]
fn commit_envelope_atomically_creates_external_effect_intent_and_genesis_event() {
    let store = registered_store();
    let token = reserve(&store, "tx-effect");

    let outcome = store
        .commit(CommitEnvelope {
            scope: "workspace:mesh".to_string(),
            namespace: "guildmesh".to_string(),
            transaction_id: "tx-effect".to_string(),
            reservation_token: token,
            required_schema_version: 1,
            required_finality_watermark: 0,
            state_writes: vec![StateWrite {
                resource: resource("room-effect"),
                expected_version: ExpectedVersion::Absent,
                payload: json!({"effect": "accepted"}),
            }],
            events: vec![HaEventAppend {
                stream_id: "guildmesh.room.room-effect".to_string(),
                kind: "room.effect_accepted".to_string(),
                payload: json!({"room_id": "room-effect"}),
            }],
            external_effect_intents: vec![sample_effect_intent()],
            command_result: None,
        })
        .expect("commit with effect");

    let effect_ref = external_effect_resource("workspace:mesh", "guildmesh", "effect-1");
    let effect = store
        .load_state(&effect_ref)
        .expect("load effect")
        .expect("effect state");
    let events = store
        .read_events(
            &external_effect_stream_id("workspace:mesh", "guildmesh", "effect-1"),
            0,
        )
        .expect("effect events");

    assert_eq!(outcome.effect_intent_ids, vec!["effect-1"]);
    assert_eq!(effect.version, 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "effect_pending");
}

#[test]
fn recovery_finalizes_reserved_transactions_without_state_or_events() {
    let store = registered_store();
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

#[test]
fn namespace_schema_migrations_are_checked_at_commit_time() {
    let store = registered_store();
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

fn sample_effect_intent() -> ExternalEffectIntent {
    ExternalEffectIntent {
        scope: "workspace:mesh".to_string(),
        namespace: "guildmesh".to_string(),
        effect_id: "effect-1".to_string(),
        version: 0,
        effect_group_id: Some("room-effect".to_string()),
        source_transaction_id: "tx-effect".to_string(),
        provider: "demo-provider".to_string(),
        operation: "room.external_notify".to_string(),
        target_ref: Some(resource("room-effect")),
        provider_idempotency_key: Some("provider-key-1".to_string()),
        request_hash: "sha256:req".to_string(),
        state: ExternalEffectState::Pending,
        attempt: None,
        retry_count: 0,
        compensation_count: 0,
        provider_operation_ref: None,
        observed_provider_state: None,
        reconciliation_evidence_ref: None,
        reconciliation_required_by_ms: None,
        unknown_outcome_reason: None,
        terminal_reason: None,
        recovery_incident_refs: Vec::new(),
        metadata: Default::default(),
    }
}

#[test]
fn in_memory_store_passes_the_reusable_backend_conformance_harness() {
    conformance::run_ha_storage_conformance(InMemoryHaStorage::default);
}
