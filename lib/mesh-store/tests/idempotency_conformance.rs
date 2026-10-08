use mesh_store::idempotency::{
    CommandCompletion, CommandResult, IdempotencyRegistry, IdempotencyReservationRequest,
    IdempotencyStatus, InMemoryIdempotencyRegistry, ReserveOutcome,
};
use serde_json::json;

fn reserve_request(
    key: &str,
    fingerprint: &str,
    command_id: &str,
    owner_id: &str,
    now_ms: u64,
) -> IdempotencyReservationRequest {
    IdempotencyReservationRequest {
        scope: "workspace:mesh".to_string(),
        namespace: "guildmesh".to_string(),
        key: key.to_string(),
        fingerprint: fingerprint.to_string(),
        command_id: command_id.to_string(),
        owner_id: owner_id.to_string(),
        now_ms,
        in_flight_ttl_ms: 100,
        retention_ttl_ms: 1_000,
    }
}

#[test]
fn first_reservation_records_in_flight_command() {
    let registry = InMemoryIdempotencyRegistry::default();

    let outcome = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-1",
            "replica-a",
            10,
        ))
        .expect("reserve");

    let ReserveOutcome::Reserved(record) = outcome else {
        panic!("expected new reservation");
    };
    assert_eq!(record.status, IdempotencyStatus::InFlight);
    assert_eq!(record.command_id, "cmd-1");
    assert_eq!(record.owner_id, "replica-a");
    assert_eq!(record.in_flight_expires_at_ms, 110);
    assert_eq!(record.retention_expires_at_ms, 1010);
}

#[test]
fn duplicate_in_flight_command_reports_existing_owner_without_new_reservation() {
    let registry = InMemoryIdempotencyRegistry::default();
    registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-1",
            "replica-a",
            10,
        ))
        .expect("reserve");

    let outcome = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-2",
            "replica-b",
            20,
        ))
        .expect("duplicate");

    let ReserveOutcome::InFlight(record) = outcome else {
        panic!("expected in-flight replay");
    };
    assert_eq!(record.command_id, "cmd-1");
    assert_eq!(record.owner_id, "replica-a");
}

#[test]
fn completed_command_replays_original_result_for_duplicate_retry() {
    let registry = InMemoryIdempotencyRegistry::default();
    let reserved = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-1",
            "replica-a",
            10,
        ))
        .expect("reserve")
        .record()
        .clone();

    registry
        .complete(CommandCompletion {
            scope: reserved.scope.clone(),
            namespace: reserved.namespace.clone(),
            key: reserved.key.clone(),
            reservation_token: reserved.reservation_token,
            now_ms: 30,
            result: CommandResult::succeeded(json!({"handoff_id": "handoff-1"})),
        })
        .expect("complete");

    let outcome = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-2",
            "replica-b",
            40,
        ))
        .expect("duplicate");

    let ReserveOutcome::ReplayedCompleted(record) = outcome else {
        panic!("expected completed replay");
    };
    assert_eq!(record.command_id, "cmd-1");
    assert_eq!(
        record.result.expect("result").payload,
        json!({"handoff_id": "handoff-1"})
    );
}

#[test]
fn conflicting_payload_reuse_is_rejected() {
    let registry = InMemoryIdempotencyRegistry::default();
    registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-1",
            "replica-a",
            10,
        ))
        .expect("reserve");

    let err = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-b",
            "cmd-2",
            "replica-b",
            20,
        ))
        .expect_err("fingerprint conflict");

    assert!(err.is_idempotency_conflict());
}

#[test]
fn expired_in_flight_same_fingerprint_can_be_reclaimed() {
    let registry = InMemoryIdempotencyRegistry::default();
    registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-1",
            "replica-a",
            10,
        ))
        .expect("reserve");

    let outcome = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-2",
            "replica-b",
            111,
        ))
        .expect("reclaim");

    let ReserveOutcome::Reclaimed(record) = outcome else {
        panic!("expected reclaimed reservation");
    };
    assert_eq!(record.command_id, "cmd-2");
    assert_eq!(record.owner_id, "replica-b");
    assert_eq!(record.attempt, 2);
}

#[test]
fn completed_records_replay_until_pruned_by_retention() {
    let registry = InMemoryIdempotencyRegistry::default();
    let reserved = registry
        .reserve(reserve_request(
            "key-1",
            "fingerprint-a",
            "cmd-1",
            "replica-a",
            10,
        ))
        .expect("reserve")
        .record()
        .clone();
    registry
        .complete(CommandCompletion {
            scope: reserved.scope.clone(),
            namespace: reserved.namespace.clone(),
            key: reserved.key.clone(),
            reservation_token: reserved.reservation_token,
            now_ms: 30,
            result: CommandResult::succeeded(json!({"deployment_id": "deploy-1"})),
        })
        .expect("complete");

    assert!(matches!(
        registry
            .reserve(reserve_request(
                "key-1",
                "fingerprint-a",
                "cmd-2",
                "replica-b",
                900
            ))
            .expect("replay before retention expiry"),
        ReserveOutcome::ReplayedCompleted(_)
    ));

    let pruned = registry.prune_expired(1_011);
    assert_eq!(pruned, 1);

    assert!(matches!(
        registry
            .reserve(reserve_request(
                "key-1",
                "fingerprint-a",
                "cmd-3",
                "replica-c",
                1_012
            ))
            .expect("new reservation after prune"),
        ReserveOutcome::Reserved(_)
    ));
}
