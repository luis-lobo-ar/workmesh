use std::fs;
use std::process::Command;

use tempfile::TempDir;
use workmesh_core::task::load_tasks;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_workmesh"))
}

fn write_item(tasks_dir: &std::path::Path, id: &str, title: &str, kind: &str) {
    let content = format!(
        "---\n\
id: {id}\n\
title: {title}\n\
kind: {kind}\n\
status: To Do\n\
priority: P2\n\
phase: Phase1\n\
dependencies: []\n\
labels: []\n\
---\n",
        id = id,
        title = title
    );
    let filename = format!("{id} - {title}.md", id = id, title = title);
    fs::write(tasks_dir.join(filename), content).expect("write task");
}

fn write_task(tasks_dir: &std::path::Path, id: &str, title: &str) {
    write_item(tasks_dir, id, title, "task");
}

fn write_epic(tasks_dir: &std::path::Path, id: &str, title: &str) {
    write_item(tasks_dir, id, title, "epic");
}

#[test]
fn add_discovered_sets_relationship() {
    let temp = TempDir::new().expect("tempdir");
    let backlog_dir = temp.path().join("backlog");
    let tasks_dir = backlog_dir.join("tasks");
    fs::create_dir_all(&tasks_dir).expect("tasks dir");

    write_task(&tasks_dir, "task-001", "Alpha");

    let output = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("add-discovered")
        .arg("--from")
        .arg("task-001")
        .arg("--title")
        .arg("Found bug")
        .arg("--description")
        .arg("Investigate the discovered bug.")
        .arg("--acceptance-criteria")
        .arg("The bug is reproduced and the required outcome is defined.")
        .arg("--definition-of-done")
        .arg("The bug is triaged with a clear resolution path.\nDocs updated if needed.")
        .output()
        .expect("add discovered");
    assert!(output.status.success());

    let tasks = load_tasks(&backlog_dir);
    let created = tasks
        .iter()
        .find(|task| task.relationships.discovered_from == ["task-001"])
        .expect("created task");
    assert_eq!(created.title, "Found bug");
    assert!(created.relationships.parent.is_empty());
}

#[test]
fn add_discovered_from_epic_sets_parent_and_discovered_from() {
    let temp = TempDir::new().expect("tempdir");
    let backlog_dir = temp.path().join("backlog");
    let tasks_dir = backlog_dir.join("tasks");
    fs::create_dir_all(&tasks_dir).expect("tasks dir");

    write_epic(&tasks_dir, "epic-001", "Platform");

    let output = bin()
        .arg("--root")
        .arg(&backlog_dir)
        .arg("add-discovered")
        .arg("--from")
        .arg("epic-001")
        .arg("--title")
        .arg("Define profile registry")
        .arg("--description")
        .arg("Capture the missing design slice.")
        .arg("--acceptance-criteria")
        .arg("The design scope is documented.")
        .arg("--definition-of-done")
        .arg("The design task is ready for review.")
        .output()
        .expect("add discovered from epic");
    assert!(output.status.success());

    let tasks = load_tasks(&backlog_dir);
    let created = tasks
        .iter()
        .find(|task| task.relationships.discovered_from == ["epic-001"])
        .expect("created task");
    assert_eq!(created.title, "Define profile registry");
    assert_eq!(created.relationships.parent, vec!["epic-001".to_string()]);
}
