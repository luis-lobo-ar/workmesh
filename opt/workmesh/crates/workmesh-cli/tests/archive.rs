use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_workmesh"))
}

fn write_task(dir: &std::path::Path, id: &str, title: &str, status: &str) {
    let filename = format!("{} - {}.md", id, title.to_lowercase());
    let path = dir.join(filename);
    let content = format!(
        "---\n\
id: {id}\n\
title: {title}\n\
kind: task\n\
status: {status}\n\
priority: P2\n\
phase: Phase1\n\
dependencies: []\n\
labels: []\n\
assignee: []\n\
updated_date: 2099-01-15 10:00\n\
---\n\n\
Body\n",
        id = id,
        title = title,
        status = status
    );
    std::fs::write(path, content).expect("write task");
}

#[test]
fn archive_accepts_explicit_task_ids() {
    let repo = TempDir::new().expect("repo");
    let backlog_dir = repo.path().join("workmesh");
    let tasks_dir = backlog_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir).expect("tasks dir");

    write_task(&tasks_dir, "task-001", "Done", "Done");
    write_task(&tasks_dir, "task-002", "Todo", "To Do");

    let output = bin()
        .arg("--root")
        .arg(repo.path())
        .arg("archive")
        .arg("task-002")
        .arg("--task-id")
        .arg("task-missing")
        .arg("--before")
        .arg("2024-01-01")
        .arg("--json")
        .output()
        .expect("archive");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(payload["archived"], serde_json::json!(["task-002"]));
    assert_eq!(payload["skipped"], serde_json::json!(["task-missing"]));
    assert_eq!(
        payload["task_ids"],
        serde_json::json!(["task-missing", "task-002"])
    );
    assert!(tasks_dir.join("task-001 - done.md").is_file());
    assert!(!tasks_dir.join("task-002 - todo.md").is_file());
    assert!(backlog_dir
        .join("archive")
        .join("2099")
        .join("01")
        .join("to-do")
        .join("task-002 - todo.md")
        .is_file());
}

#[test]
fn archive_preserves_nested_task_path() {
    let repo = TempDir::new().expect("repo");
    let backlog_dir = repo.path().join("workmesh");
    let nested_dir = backlog_dir
        .join("tasks")
        .join("customer")
        .join("project")
        .join("service")
        .join("feature");
    std::fs::create_dir_all(&nested_dir).expect("nested tasks dir");

    write_task(&nested_dir, "task-003", "NestedDone", "Done");

    let output = bin()
        .arg("--root")
        .arg(repo.path())
        .arg("archive")
        .arg("--before")
        .arg("2999-01-01")
        .arg("--json")
        .output()
        .expect("archive");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(backlog_dir
        .join("archive")
        .join("2099")
        .join("01")
        .join("done")
        .join("customer")
        .join("project")
        .join("service")
        .join("feature")
        .join("task-003 - nesteddone.md")
        .is_file());
}
