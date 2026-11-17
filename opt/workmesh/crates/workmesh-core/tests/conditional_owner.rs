use chrono::{Duration, Local};
use workmesh_core::conditional_owner::{conditional_owner, ContractPin, OwnerAction};
use workmesh_core::task::parse_task_file;
use workmesh_core::task_ops::{update_task_field, FieldValue};

#[test]
fn conditional_owner_never_overwrites_foreign_or_expired_lease_and_pins_contract() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("task.md");
    std::fs::write(&path, "---\nid: task-001\nuid: stable\nstatus: To Do\n---\nDescription:\nfix bug\nAcceptance Criteria:\ntests green\nDefinition of Done:\nbehavior proven\n").unwrap();
    let pin = ContractPin::from_task(&parse_task_file(&path).unwrap()).unwrap();
    let now = Local::now().naive_local();
    conditional_owner(&path, &pin, "campaign-a", now, OwnerAction::Claim).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(conditional_owner(&path, &pin, "campaign-b", now, OwnerAction::Claim).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    for action in [
        OwnerAction::Renew,
        OwnerAction::Note("accepted".into()),
        OwnerAction::Complete,
    ] {
        update_task_field(
            &path,
            "lease_owner",
            Some(FieldValue::Scalar("foreign".into())),
        )
        .unwrap();
        let foreign = std::fs::read_to_string(&path).unwrap();
        assert!(conditional_owner(&path, &pin, "campaign-a", now, action).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), foreign);
    }
    update_task_field(
        &path,
        "lease_owner",
        Some(FieldValue::Scalar("campaign-a".into())),
    )
    .unwrap();
    assert!(conditional_owner(
        &path,
        &pin,
        "campaign-a",
        now + Duration::minutes(11),
        OwnerAction::Renew
    )
    .is_err());
    std::fs::write(
        &path,
        std::fs::read_to_string(&path)
            .unwrap()
            .replace("fix bug", "weakened scope"),
    )
    .unwrap();
    assert!(conditional_owner(&path, &pin, "campaign-a", now, OwnerAction::Complete).is_err());
}

#[test]
fn completion_atomically_closes_same_live_owner_without_later_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("task.md");
    std::fs::write(&path,"---\nid: task-001\nuid: stable\nstatus: To Do\n---\nDescription:\nfix\nAcceptance Criteria:\nworks\nDefinition of Done:\nverified\n").unwrap();
    let pin = ContractPin::from_task(&parse_task_file(&path).unwrap()).unwrap();
    let now = Local::now().naive_local();
    conditional_owner(&path, &pin, "a", now, OwnerAction::Claim).unwrap();
    let acquired = parse_task_file(&path).unwrap().lease.unwrap().acquired_at;
    conditional_owner(
        &path,
        &pin,
        "a",
        now + Duration::minutes(1),
        OwnerAction::Renew,
    )
    .unwrap();
    assert_eq!(
        parse_task_file(&path).unwrap().lease.unwrap().acquired_at,
        acquired
    );
    conditional_owner(
        &path,
        &pin,
        "a",
        now,
        OwnerAction::Note("exact evidence".into()),
    )
    .unwrap();
    conditional_owner(&path, &pin, "a", now, OwnerAction::Complete).unwrap();
    let done = parse_task_file(&path).unwrap();
    assert_eq!(done.status, "Done");
    assert!(done.lease.is_none());
    assert!(conditional_owner(&path, &pin, "a", now, OwnerAction::Complete).is_err());
}

#[test]
fn malformed_indefinite_and_ownerless_leases_cannot_be_claimed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("task.md");
    let base="---\nid: task-001\nuid: stable\nstatus: To Do\n---\nDescription:\nfix\nAcceptance Criteria:\nworks\nDefinition of Done:\nverified\n";
    std::fs::write(&path, base).unwrap();
    let pin = ContractPin::from_task(&parse_task_file(&path).unwrap()).unwrap();
    for fields in [
        "lease_owner: foreign\n",
        "lease_owner: foreign\nlease_expires_at: nonsense\n",
        "lease_expires_at: 2099-01-01 00:00\n",
        "lease_owner: foreign\nlease_acquired_at: malformed\nlease_expires_at: 2000-01-01 00:00\n",
        "lease_owner: foreign\nlease_owner: other\nlease_acquired_at: 2000-01-01 00:00\nlease_expires_at: 2000-01-01 00:01\n",
    ] {
        let text = base.replacen("status: To Do\n", &format!("status: To Do\n{fields}"), 1);
        std::fs::write(&path, &text).unwrap();
        assert!(conditional_owner(
            &path,
            &pin,
            "campaign-a",
            Local::now().naive_local(),
            OwnerAction::Claim
        )
        .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }
}
