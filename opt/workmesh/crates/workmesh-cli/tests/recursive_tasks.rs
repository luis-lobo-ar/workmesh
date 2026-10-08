use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_workmesh"))
}

fn write_record(dir: &std::path::Path, id: &str, kind: &str, title: &str, status: &str) {
    std::fs::create_dir_all(dir).expect("task dir");
    let filename = format!("{} - {}.md", id, title.to_lowercase());
    let path = dir.join(filename);
    let content = format!(
        "---\n\
id: {id}\n\
title: {title}\n\
kind: {kind}\n\
status: {status}\n\
priority: P2\n\
phase: Phase1\n\
dependencies: []\n\
labels: []\n\
assignee: []\n\
---\n\n\
Description:\n\
--------------------------------------------------\n\
- Deliver {title}.\n\n\
Acceptance Criteria:\n\
--------------------------------------------------\n\
- Expected behavior is validated.\n\n\
Definition of Done:\n\
--------------------------------------------------\n\
- {title} behavior matches the expected outcome.\n\
- Code/config committed.\n\
- Docs updated if needed.\n",
        id = id,
        title = title,
        kind = kind,
        status = status
    );
    std::fs::write(path, content).expect("write record");
}

#[test]
fn ready_reads_nested_task_files_but_keeps_epics_as_containers() {
    let repo = TempDir::new().expect("repo");
    let nested = repo
        .path()
        .join("workmesh")
        .join("tasks")
        .join("customer-a")
        .join("application-one")
        .join("feature-x");
    write_record(&nested, "epic-nested-001", "epic", "Feature X", "To Do");
    write_record(
        &nested,
        "task-nested-001",
        "task",
        "Implement Feature X",
        "To Do",
    );

    let output = bin()
        .arg("--root")
        .arg(repo.path())
        .arg("ready")
        .arg("--json")
        .output()
        .expect("ready");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ready: Value = serde_json::from_slice(&output.stdout).expect("json");
    let ids: Vec<String> = ready
        .as_array()
        .expect("ready array")
        .iter()
        .map(|value| value.get("id").unwrap().as_str().unwrap().to_string())
        .collect();
    assert!(ids.contains(&"task-nested-001".to_string()));
    assert!(!ids.contains(&"epic-nested-001".to_string()));
}
