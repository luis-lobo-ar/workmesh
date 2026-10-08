use mesh_store::lease::{
    InMemoryLeaseStore, LeaderElectionRequest, LeaseAcquireRequest, LeaseAuditEventKind,
    LeaseReleaseRequest, LeaseRenewRequest, LeaseStatus, LeaseStore,
};
use std::sync::{Arc, Barrier};
use std::thread;

#[test]
fn acquire_rejects_competing_owner_until_expiry_then_takeover_increments_fencing() {
    let store = InMemoryLeaseStore::default();
    let first = store
        .acquire(LeaseAcquireRequest::new(
            "guildmesh:room:room-1",
            "replica-a",
            1_000,
            100,
        ))
        .expect("acquire");

    let conflict = store
        .acquire(LeaseAcquireRequest::new(
            "guildmesh:room:room-1",
            "replica-b",
            1_050,
            100,
        ))
        .expect_err("held by replica-a");

    assert!(conflict.is_lease_held());

    let takeover = store
        .acquire(LeaseAcquireRequest::new(
            "guildmesh:room:room-1",
            "replica-b",
            1_101,
            100,
        ))
        .expect("takeover after expiry");

    assert_eq!(first.fencing_token, 1);
    assert_eq!(takeover.owner_id, "replica-b");
    assert_eq!(takeover.fencing_token, 2);
    assert_eq!(takeover.lease_version, 2);
}

#[test]
fn renew_requires_current_owner_version_and_fencing_token() {
    let store = InMemoryLeaseStore::default();
    let acquired = store
        .acquire(LeaseAcquireRequest::new(
            "agentmesh:run:run-1",
            "executor-a",
            10,
            50,
        ))
        .expect("acquire");

    let renewed = store
        .renew(LeaseRenewRequest {
            lease_name: "agentmesh:run:run-1".to_string(),
            owner_id: "executor-a".to_string(),
            lease_version: acquired.lease_version,
            fencing_token: acquired.fencing_token,
            now_ms: 40,
            ttl_ms: 75,
        })
        .expect("renew");

    assert_eq!(renewed.owner_id, "executor-a");
    assert_eq!(renewed.fencing_token, acquired.fencing_token);
    assert_eq!(renewed.lease_version, acquired.lease_version + 1);
    assert_eq!(renewed.expires_at_ms, 115);

    let stale = store
        .renew(LeaseRenewRequest {
            lease_name: "agentmesh:run:run-1".to_string(),
            owner_id: "executor-a".to_string(),
            lease_version: acquired.lease_version,
            fencing_token: acquired.fencing_token,
            now_ms: 45,
            ttl_ms: 75,
        })
        .expect_err("stale version");

    assert!(stale.is_stale_owner());
}

#[test]
fn release_allows_reacquire_and_rejects_stale_fencing_token() {
    let store = InMemoryLeaseStore::default();
    let acquired = store
        .acquire(LeaseAcquireRequest::new(
            "shipmesh:env:beta",
            "worker-a",
            10,
            100,
        ))
        .expect("acquire");

    store
        .release(LeaseReleaseRequest {
            lease_name: "shipmesh:env:beta".to_string(),
            owner_id: "worker-a".to_string(),
            fencing_token: acquired.fencing_token,
            now_ms: 20,
        })
        .expect("release");

    let reacquired = store
        .acquire(LeaseAcquireRequest::new(
            "shipmesh:env:beta",
            "worker-b",
            25,
            100,
        ))
        .expect("reacquire");

    assert_eq!(reacquired.owner_id, "worker-b");
    assert_eq!(reacquired.fencing_token, acquired.fencing_token + 1);

    let stale = store.verify_fencing("shipmesh:env:beta", "worker-b", acquired.fencing_token);
    assert!(stale.expect_err("stale token").is_stale_fencing_token());
}

#[test]
fn audit_records_acquire_reject_renew_release_and_takeover() {
    let store = InMemoryLeaseStore::default();
    let acquired = store
        .acquire(LeaseAcquireRequest::new(
            "testmesh:worker:queue",
            "worker-a",
            0,
            10,
        ))
        .expect("acquire");
    let _ = store.acquire(LeaseAcquireRequest::new(
        "testmesh:worker:queue",
        "worker-b",
        1,
        10,
    ));
    let renewed = store
        .renew(LeaseRenewRequest {
            lease_name: "testmesh:worker:queue".to_string(),
            owner_id: "worker-a".to_string(),
            lease_version: acquired.lease_version,
            fencing_token: acquired.fencing_token,
            now_ms: 2,
            ttl_ms: 10,
        })
        .expect("renew");
    store
        .release(LeaseReleaseRequest {
            lease_name: "testmesh:worker:queue".to_string(),
            owner_id: "worker-a".to_string(),
            fencing_token: renewed.fencing_token,
            now_ms: 3,
        })
        .expect("release");
    store
        .acquire(LeaseAcquireRequest::new(
            "testmesh:worker:queue",
            "worker-b",
            4,
            10,
        ))
        .expect("takeover from released");

    let kinds = store
        .audit("testmesh:worker:queue")
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        [
            LeaseAuditEventKind::Acquired,
            LeaseAuditEventKind::AcquireRejected,
            LeaseAuditEventKind::Renewed,
            LeaseAuditEventKind::Released,
            LeaseAuditEventKind::Takeover,
        ]
    );
}

#[test]
fn expire_marks_inactive_and_allows_takeover_with_new_fencing_token() {
    let store = InMemoryLeaseStore::default();
    let acquired = store
        .acquire(LeaseAcquireRequest::new(
            "boardmesh:session:1",
            "replica-a",
            0,
            10,
        ))
        .expect("acquire");

    let expired = store.expire("boardmesh:session:1", 11).expect("expire");

    assert_eq!(expired.status, LeaseStatus::Expired);
    assert_eq!(expired.expires_at_ms, 10);

    let takeover = store
        .acquire(LeaseAcquireRequest::new(
            "boardmesh:session:1",
            "replica-b",
            12,
            10,
        ))
        .expect("takeover");

    assert_eq!(takeover.fencing_token, acquired.fencing_token + 1);
    assert_eq!(
        store
            .audit("boardmesh:session:1")
            .into_iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [
            LeaseAuditEventKind::Acquired,
            LeaseAuditEventKind::Expired,
            LeaseAuditEventKind::Takeover,
        ]
    );
}

#[test]
fn leader_election_uses_named_lease_and_returns_current_leader() {
    let store = InMemoryLeaseStore::default();
    let leader = store
        .elect_leader(LeaderElectionRequest {
            group: "guildmesh:sse-fanout".to_string(),
            candidates: vec!["replica-b".to_string(), "replica-a".to_string()],
            now_ms: 100,
            ttl_ms: 50,
        })
        .expect("elect leader");

    assert_eq!(leader.lease_name, "leader:guildmesh:sse-fanout");
    assert_eq!(leader.owner_id, "replica-a");

    let observed = store
        .elect_leader(LeaderElectionRequest {
            group: "guildmesh:sse-fanout".to_string(),
            candidates: vec!["replica-c".to_string(), "replica-b".to_string()],
            now_ms: 110,
            ttl_ms: 50,
        })
        .expect("observe current leader");

    assert_eq!(observed.owner_id, "replica-a");
}

#[test]
fn concurrent_acquire_allows_one_owner_and_audits_rejections() {
    let store = Arc::new(InMemoryLeaseStore::default());
    let barrier = Arc::new(Barrier::new(6));
    let mut handles = Vec::new();

    for idx in 0..6 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            store.acquire(LeaseAcquireRequest::new(
                "vaultmesh:lease:secret-ref",
                format!("worker-{idx}"),
                0,
                100,
            ))
        }));
    }

    let mut acquired = 0;
    let mut rejected = 0;
    for handle in handles {
        match handle.join().expect("join") {
            Ok(_) => acquired += 1,
            Err(err) if err.is_lease_held() => rejected += 1,
            Err(err) => panic!("unexpected lease error: {err}"),
        }
    }

    assert_eq!(acquired, 1);
    assert_eq!(rejected, 5);

    let audit = store.audit("vaultmesh:lease:secret-ref");
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == LeaseAuditEventKind::Acquired)
            .count(),
        1
    );
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.kind == LeaseAuditEventKind::AcquireRejected)
            .count(),
        5
    );
}
