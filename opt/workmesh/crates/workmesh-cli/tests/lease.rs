use std::process::Command;

use tempfile::TempDir;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_workmesh"))
}

fn write_task(tasks_dir: &std::path::Path) {
    let content = "---\n".to_string()
        + "id: task-001\n"
        + "title: Lease Test\n"
        + "status: To Do\n"
        + "priority: P2\n"
        + "phase: Phase1\n"
        + "dependencies: []\n"
        + "labels: []\n"
        + "assignee: []\n"
        + "---\n";
    std::fs::write(tasks_dir.join("task-001 - lease.md"), content).expect("write");
}

fn write_active_lease_task(tasks_dir: &std::path::Path, id: &str, title: &str, status: &str) {
    let content = format!(
        "---\n\
id: {id}\n\
title: {title}\n\
kind: task\n\
status: {status}\n\
priority: P1\n\
phase: Phase1\n\
dependencies: []\n\
labels: []\n\
assignee: []\n\
lease_owner: session-xyz\n\
lease_acquired_at: 2099-01-01 10:00\n\
lease_expires_at: 2099-01-01 11:00\n\
---\n\
\n\
Description:\n\
--------------------------------------------------\n\
- Seed\n\
\n\
Acceptance Criteria:\n\
--------------------------------------------------\n\
- Seed\n\
\n\
Definition of Done:\n\
--------------------------------------------------\n\
- Seed\n",
        id = id,
        title = title,
        status = status
    );
    std::fs::write(tasks_dir.join(format!("{id} - {title}.md")), content).expect("write");
}

#[test]
fn claim_and_release_updates_lease_fields() {
    let temp = TempDir::new().expect("tempdir");
    let backlog_dir = temp.path().join("backlog");
    let tasks_dir = backlog_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    write_task(&tasks_dir);

    let claim = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("claim")
        .arg("task-001")
        .arg("agent-1")
        .arg("--minutes")
        .arg("30")
        .output()
        .expect("claim");
    assert!(claim.status.success());

    let content = std::fs::read_to_string(tasks_dir.join("task-001 - lease.md")).expect("read");
    assert!(content.contains("lease_owner: agent-1"));
    assert!(content.contains("lease_expires_at:"));

    let release = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("release")
        .arg("task-001")
        .output()
        .expect("release");
    assert!(release.status.success());

    let content = std::fs::read_to_string(tasks_dir.join("task-001 - lease.md")).expect("read");
    assert!(!content.contains("lease_owner: agent-1"));
}

#[test]
fn show_and_next_surface_active_lease_owner() {
    let temp = TempDir::new().expect("tempdir");
    let backlog_dir = temp.path().join("backlog");
    let tasks_dir = backlog_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    write_active_lease_task(&tasks_dir, "task-001", "Lease Visible", "In Progress");
    write_active_lease_task(&tasks_dir, "task-002", "Other", "To Do");

    let show = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("show")
        .arg("task-001")
        .output()
        .expect("show");
    assert!(show.status.success());
    let show_stdout = String::from_utf8_lossy(&show.stdout);
    assert!(show_stdout.contains("lease: session-xyz"));

    let next = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("next")
        .output()
        .expect("next");
    assert!(next.status.success());
    let next_stdout = String::from_utf8_lossy(&next.stdout);
    assert!(next_stdout.contains("lease: session-xyz"));
}

#[test]
fn next_ignores_done_tasks_with_stale_leases() {
    let temp = TempDir::new().expect("tempdir");
    let backlog_dir = temp.path().join("backlog");
    let tasks_dir = backlog_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    write_active_lease_task(&tasks_dir, "task-001", "Done Leased", "Done");
    write_active_lease_task(&tasks_dir, "task-002", "Ready Work", "To Do");

    let next = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("next")
        .output()
        .expect("next");
    assert!(next.status.success());
    let next_stdout = String::from_utf8_lossy(&next.stdout);
    assert!(next_stdout.contains("task-002"));
    assert!(!next_stdout.contains("task-001"));
}

#[test]
fn set_status_done_clears_active_lease_fields() {
    let temp = TempDir::new().expect("tempdir");
    let backlog_dir = temp.path().join("backlog");
    let tasks_dir = backlog_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");
    write_active_lease_task(&tasks_dir, "task-001", "Completing Leased", "In Progress");

    let done = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("set-status")
        .arg("task-001")
        .arg("Done")
        .output()
        .expect("set-status");
    assert!(done.status.success());

    let content =
        std::fs::read_to_string(tasks_dir.join("task-001 - Completing Leased.md")).expect("read");
    assert!(!content.contains("lease_owner:"));
    assert!(!content.contains("lease_acquired_at:"));
    assert!(!content.contains("lease_expires_at:"));
}
