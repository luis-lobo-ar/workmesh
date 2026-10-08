use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Serialize;
use serde_yaml::Value;
use thiserror::Error;

use crate::backlog::resolve_tasks_dir;

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub uid: Option<String>,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub phase: String,
    pub dependencies: Vec<String>,
    pub labels: Vec<String>,
    pub assignee: Vec<String>,
    pub relationships: Relationships,
    pub lease: Option<Lease>,
    pub project: Option<String>,
    pub initiative: Option<String>,
    pub created_date: Option<String>,
    pub updated_date: Option<String>,
    pub extra: HashMap<String, Value>,
    pub file_path: Option<PathBuf>,
    pub body: String,
}

impl Task {
    pub fn id_num(&self) -> i32 {
        let re = Regex::new(r"(\d+)").expect("regex");
        re.captures(&self.id)
            .and_then(|cap| cap.get(1))
            .and_then(|m| m.as_str().parse::<i32>().ok())
            .unwrap_or(999_999)
    }

    pub fn mesh_refs(&self) -> MeshRefs {
        parse_mesh_refs(self.extra.get("mesh_refs"))
    }
}

#[derive(Debug, Error)]
pub enum TaskParseError {
    #[error("Missing front matter delimiter")]
    MissingFrontMatter,
    #[error("Missing closing --- for front matter")]
    MissingFrontMatterEnd,
    #[error("Invalid task file: {0}")]
    Invalid(String),
}

impl From<std::io::Error> for TaskParseError {
    fn from(err: std::io::Error) -> Self {
        TaskParseError::Invalid(err.to_string())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Relationships {
    pub blocked_by: Vec<String>,
    pub parent: Vec<String>,
    pub child: Vec<String>,
    pub discovered_from: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Lease {
    pub owner: String,
    pub acquired_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MeshRefs {
    pub boardmesh: BoardmeshRefs,
    pub guildmesh: GuildmeshRefs,
    pub shipmesh: ShipmeshRefs,
    pub testmesh: TestmeshRefs,
    pub timemesh: TimemeshRefs,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BoardmeshRefs {
    pub sessions: Vec<String>,
    pub canvases: Vec<String>,
    pub objects: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GuildmeshRefs {
    pub rooms: Vec<String>,
    pub threads: Vec<String>,
    pub handoffs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ShipmeshRefs {
    pub deployments: Vec<String>,
    pub environments: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TestmeshRefs {
    pub runs: Vec<String>,
    pub suites: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TimemeshRefs {
    pub workspaces: Vec<String>,
    pub entries: Vec<String>,
}

pub fn split_front_matter(text: &str) -> Result<(String, String), TaskParseError> {
    if !text.starts_with("---") {
        return Err(TaskParseError::MissingFrontMatter);
    }
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return Err(TaskParseError::MissingFrontMatter);
    }
    let mut end_idx = None;
    for (idx, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            end_idx = Some(idx);
            break;
        }
    }
    let end_idx = end_idx.ok_or(TaskParseError::MissingFrontMatterEnd)?;
    let front = lines[1..end_idx].join("\n");
    let body = lines[end_idx + 1..].join("\n");
    Ok((front, body))
}

pub fn parse_list_value(value: Option<&Value>) -> Vec<String> {
    match value {
        None => Vec::new(),
        Some(Value::Null) => Vec::new(),
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(value_to_string)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => parse_list_string(s),
        Some(other) => value_to_string(other)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|s| vec![s])
            .unwrap_or_default(),
    }
}

pub fn parse_task_file(path: &Path) -> Result<Task, TaskParseError> {
    let text = fs::read_to_string(path).map_err(|err| TaskParseError::Invalid(err.to_string()))?;
    let (front, body) = split_front_matter(&text)?;

    let data = parse_front_matter(&front);

    let task_id = data
        .get("id")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id_from_filename(path));
    let uid = data
        .get("uid")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let kind = data
        .get("kind")
        .and_then(value_to_string)
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "task".to_string());

    let title = data
        .get("title")
        .and_then(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
    let status = data
        .get("status")
        .and_then(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
    let priority = data
        .get("priority")
        .and_then(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();
    let phase = data
        .get("phase")
        .and_then(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string();

    let dependencies = parse_list_value(data.get("dependencies"));
    let labels = parse_list_value(data.get("labels"));
    let assignee = parse_list_value(data.get("assignee"));
    let relationships = parse_relationships(&data);
    let lease = parse_lease(&data);
    let project = data
        .get("project")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let initiative = data
        .get("initiative")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let created_date = data
        .get("created_date")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let updated_date = data
        .get("updated_date")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let known_keys = [
        "id",
        "uid",
        "kind",
        "title",
        "status",
        "priority",
        "phase",
        "dependencies",
        "labels",
        "assignee",
        "blocked_by",
        "parent",
        "child",
        "discovered_from",
        "relationships",
        "lease",
        "lease_owner",
        "lease_acquired_at",
        "lease_expires_at",
        "project",
        "initiative",
        "created_date",
        "updated_date",
    ];
    let mut extra = HashMap::new();
    for (key, value) in data {
        if !known_keys.contains(&key.as_str()) {
            extra.insert(key, value);
        }
    }

    Ok(Task {
        id: task_id,
        uid,
        kind,
        title,
        status,
        priority,
        phase,
        dependencies,
        labels,
        assignee,
        relationships,
        lease,
        project,
        initiative,
        created_date,
        updated_date,
        extra,
        file_path: Some(path.to_path_buf()),
        body,
    })
}

pub fn load_tasks(backlog_dir: &Path) -> Vec<Task> {
    let tasks_dir = tasks_dir_for_root(backlog_dir);
    load_tasks_from_dir_recursive(&tasks_dir)
}

/// Load tasks from `tasks/` and, optionally, from `archive/` (recursively).
///
/// Archived tasks are typically moved out of `tasks/` by the `archive` command, so this is
/// useful for listing historical Done work without unarchiving files.
pub fn load_tasks_with_archive(backlog_dir: &Path) -> Vec<Task> {
    let mut tasks = load_tasks(backlog_dir);
    let archive_root = archive_root_for_root(backlog_dir);
    if archive_root.is_dir() {
        tasks.extend(load_tasks_from_dir_recursive(&archive_root));
    }
    tasks
}

pub fn tasks_dir_for_root(root: &Path) -> PathBuf {
    resolve_tasks_dir(root).unwrap_or_else(|_| {
        if root
            .file_name()
            .map(|name| name == "tasks")
            .unwrap_or(false)
        {
            root.to_path_buf()
        } else {
            root.join("tasks")
        }
    })
}

pub fn archive_root_for_root(root: &Path) -> PathBuf {
    let tasks_root = tasks_dir_for_root(root);
    tasks_root
        .parent()
        .unwrap_or(tasks_root.as_path())
        .join("archive")
}

fn load_tasks_from_dir_recursive(root: &Path) -> Vec<Task> {
    let mut md_files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().map(|ext| ext == "md").unwrap_or(false) {
                md_files.push(path);
            }
        }
    }
    md_files.sort();

    let mut tasks = Vec::new();
    for path in md_files {
        if let Ok(task) = parse_task_file(&path) {
            tasks.push(task);
        }
    }
    tasks
}

fn parse_front_matter(front: &str) -> HashMap<String, Value> {
    if let Ok(Value::Mapping(map)) = serde_yaml::from_str::<Value>(front) {
        let mut data = HashMap::new();
        for (key, value) in map {
            if let Some(key_str) = value_to_string(&key) {
                data.insert(key_str, value);
            }
        }
        if !data.is_empty() {
            return data;
        }
    }
    parse_front_matter_loose(front)
}

fn parse_front_matter_loose(front: &str) -> HashMap<String, Value> {
    let mut data = HashMap::new();
    let lines: Vec<&str> = front.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            i += 1;
            continue;
        };
        let key = key.trim().to_string();
        let value = rest.trim();
        if value == ">-" || value == "|" {
            let mut block: Vec<String> = Vec::new();
            i += 1;
            while i < lines.len() {
                let next = lines[i];
                if next.starts_with(' ') || next.starts_with('\t') {
                    block.push(next.trim().to_string());
                    i += 1;
                    continue;
                }
                break;
            }
            let joined = if value == ">-" {
                block.join(" ").trim().to_string()
            } else {
                block.join("\n")
            };
            data.insert(key, Value::String(joined));
            continue;
        }
        if value.is_empty() {
            let mut items: Vec<Value> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let next = lines[j];
                if next.trim_start().starts_with("- ") {
                    let item = next.trim_start()[2..].trim();
                    if !item.is_empty() {
                        items.push(Value::String(item.to_string()));
                    }
                    j += 1;
                    continue;
                }
                if next.starts_with(' ') || next.starts_with('\t') {
                    j += 1;
                    continue;
                }
                break;
            }
            if !items.is_empty() {
                data.insert(key, Value::Sequence(items));
                i = j;
                continue;
            }
        }
        if value.starts_with('[') && value.ends_with(']') {
            let items = parse_list_string(value)
                .into_iter()
                .map(Value::String)
                .collect();
            data.insert(key, Value::Sequence(items));
            i += 1;
            continue;
        }
        data.insert(key, Value::String(value.to_string()));
        i += 1;
    }
    data
}

fn parse_list_string(value: &str) -> Vec<String> {
    let raw = value.trim();
    if raw.is_empty() || raw == "[]" {
        return Vec::new();
    }
    let inner = if raw.starts_with('[') && raw.ends_with(']') {
        raw[1..raw.len() - 1].trim()
    } else {
        raw
    };
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn id_from_filename(path: &Path) -> String {
    let file_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let re = Regex::new(r"(task-\d+)").expect("regex");
    re.captures(file_name)
        .and_then(|cap| cap.get(1))
        .map(|m| m.as_str().to_lowercase())
        .unwrap_or_else(|| file_name.to_string())
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(val) => Some(val.clone()),
        Value::Number(num) => Some(num.to_string()),
        Value::Bool(val) => Some(val.to_string()),
        Value::Null => None,
        _ => serde_yaml::to_string(value)
            .ok()
            .map(|s| s.trim().to_string()),
    }
}

fn parse_relationships(data: &HashMap<String, Value>) -> Relationships {
    if let Some(Value::Mapping(map)) = data.get("relationships") {
        let blocked_by = map
            .get(Value::String("blocked_by".to_string()))
            .and_then(value_to_list);
        let parent = map
            .get(Value::String("parent".to_string()))
            .and_then(value_to_list);
        let child = map
            .get(Value::String("child".to_string()))
            .and_then(value_to_list);
        let discovered_from = map
            .get(Value::String("discovered_from".to_string()))
            .and_then(value_to_list);

        if blocked_by.is_some() || parent.is_some() || child.is_some() || discovered_from.is_some()
        {
            return Relationships {
                blocked_by: blocked_by.unwrap_or_default(),
                parent: parent.unwrap_or_default(),
                child: child.unwrap_or_default(),
                discovered_from: discovered_from.unwrap_or_default(),
            };
        }
    }

    Relationships {
        blocked_by: parse_list_value(data.get("blocked_by")),
        parent: parse_list_value(data.get("parent")),
        child: parse_list_value(data.get("child")),
        discovered_from: parse_list_value(data.get("discovered_from")),
    }
}

fn parse_mesh_refs(value: Option<&Value>) -> MeshRefs {
    let Some(Value::Mapping(root)) = value else {
        return MeshRefs::default();
    };

    MeshRefs {
        boardmesh: parse_boardmesh_refs(root.get(Value::String("boardmesh".to_string()))),
        guildmesh: parse_guildmesh_refs(root.get(Value::String("guildmesh".to_string()))),
        shipmesh: parse_shipmesh_refs(root.get(Value::String("shipmesh".to_string()))),
        testmesh: parse_testmesh_refs(root.get(Value::String("testmesh".to_string()))),
        timemesh: parse_timemesh_refs(root.get(Value::String("timemesh".to_string()))),
    }
}

fn parse_boardmesh_refs(value: Option<&Value>) -> BoardmeshRefs {
    let Some(Value::Mapping(map)) = value else {
        return BoardmeshRefs::default();
    };

    BoardmeshRefs {
        sessions: parse_list_value(map.get(Value::String("sessions".to_string()))),
        canvases: parse_list_value(map.get(Value::String("canvases".to_string()))),
        objects: parse_list_value(map.get(Value::String("objects".to_string()))),
    }
}

fn parse_guildmesh_refs(value: Option<&Value>) -> GuildmeshRefs {
    let Some(Value::Mapping(map)) = value else {
        return GuildmeshRefs::default();
    };

    GuildmeshRefs {
        rooms: parse_list_value(map.get(Value::String("rooms".to_string()))),
        threads: parse_list_value(map.get(Value::String("threads".to_string()))),
        handoffs: parse_list_value(map.get(Value::String("handoffs".to_string()))),
    }
}

fn parse_shipmesh_refs(value: Option<&Value>) -> ShipmeshRefs {
    let Some(Value::Mapping(map)) = value else {
        return ShipmeshRefs::default();
    };

    ShipmeshRefs {
        deployments: parse_list_value(map.get(Value::String("deployments".to_string()))),
        environments: parse_list_value(map.get(Value::String("environments".to_string()))),
    }
}

fn parse_testmesh_refs(value: Option<&Value>) -> TestmeshRefs {
    let Some(Value::Mapping(map)) = value else {
        return TestmeshRefs::default();
    };

    TestmeshRefs {
        runs: parse_list_value(map.get(Value::String("runs".to_string()))),
        suites: parse_list_value(map.get(Value::String("suites".to_string()))),
    }
}

fn parse_timemesh_refs(value: Option<&Value>) -> TimemeshRefs {
    let Some(Value::Mapping(map)) = value else {
        return TimemeshRefs::default();
    };

    TimemeshRefs {
        workspaces: parse_list_value(map.get(Value::String("workspaces".to_string()))),
        entries: parse_list_value(map.get(Value::String("entries".to_string()))),
    }
}

fn value_to_list(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Sequence(_) => Some(parse_list_value(Some(value))),
        Value::String(_) => Some(parse_list_value(Some(value))),
        _ => None,
    }
}

fn parse_lease(data: &HashMap<String, Value>) -> Option<Lease> {
    let owner = data
        .get("lease_owner")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let acquired_at = data
        .get("lease_acquired_at")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let expires_at = data
        .get("lease_expires_at")
        .and_then(value_to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if let Some(owner) = owner {
        return Some(Lease {
            owner,
            acquired_at,
            expires_at,
        });
    }

    if let Some(Value::Mapping(map)) = data.get("lease") {
        let owner = map
            .get(Value::String("owner".to_string()))
            .and_then(value_to_string)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let acquired_at = map
            .get(Value::String("acquired_at".to_string()))
            .and_then(value_to_string)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let expires_at = map
            .get(Value::String("expires_at".to_string()))
            .and_then(value_to_string)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(owner) = owner {
            return Some(Lease {
                owner,
                acquired_at,
                expires_at,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_list_value_handles_string_lists() {
        let value = Value::String("[a, b, c]".to_string());
        assert_eq!(parse_list_value(Some(&value)), vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_task_file_reads_yaml_front_matter() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-001 - a.md");
        let content = "---\n".to_string()
            + "id: task-001\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "dependencies: [task-000]\n"
            + "labels: [ops]\n"
            + "---\n\n"
            + "Description:\n"
            + "--------------------------------------------------\n"
            + "- Example\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        assert_eq!(task.id, "task-001");
        assert_eq!(task.dependencies, vec!["task-000"]);
        assert_eq!(task.labels, vec!["ops"]);
    }

    #[test]
    fn parse_task_file_reads_uid() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-006 - uid.md");
        let content = "---\n".to_string()
            + "id: task-006\n"
            + "uid: 01J2R0QZ6QX9V0000000000000\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "---\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        assert_eq!(task.uid.as_deref(), Some("01J2R0QZ6QX9V0000000000000"));
    }

    #[test]
    fn parse_task_file_reads_relationships_mapping() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-002 - rel.md");
        let content = "---\n".to_string()
            + "id: task-002\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "relationships:\n"
            + "  blocked_by: [task-001]\n"
            + "  parent: [task-000]\n"
            + "  child: [task-003]\n"
            + "  discovered_from: [task-004]\n"
            + "---\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        assert_eq!(task.relationships.blocked_by, vec!["task-001"]);
        assert_eq!(task.relationships.parent, vec!["task-000"]);
        assert_eq!(task.relationships.child, vec!["task-003"]);
        assert_eq!(task.relationships.discovered_from, vec!["task-004"]);
    }

    #[test]
    fn parse_task_file_reads_flat_relationships() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-003 - rel-flat.md");
        let content = "---\n".to_string()
            + "id: task-003\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "blocked_by: [task-001]\n"
            + "parent: [task-000]\n"
            + "child: [task-004]\n"
            + "discovered_from: [task-005]\n"
            + "---\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        assert_eq!(task.relationships.blocked_by, vec!["task-001"]);
        assert_eq!(task.relationships.parent, vec!["task-000"]);
        assert_eq!(task.relationships.child, vec!["task-004"]);
        assert_eq!(task.relationships.discovered_from, vec!["task-005"]);
    }

    #[test]
    fn parse_task_file_reads_mesh_refs_mapping() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-003 - mesh-refs.md");
        let content = "---\n".to_string()
            + "id: task-003\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "mesh_refs:\n"
            + "  boardmesh:\n"
            + "    sessions: [board-main]\n"
            + "  guildmesh:\n"
            + "    rooms: [room-beta]\n"
            + "    threads: [thread-release]\n"
            + "  timemesh:\n"
            + "    entries: [entry-1]\n"
            + "---\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        let mesh_refs = task.mesh_refs();
        assert_eq!(mesh_refs.boardmesh.sessions, vec!["board-main"]);
        assert_eq!(mesh_refs.guildmesh.rooms, vec!["room-beta"]);
        assert_eq!(mesh_refs.guildmesh.threads, vec!["thread-release"]);
        assert_eq!(mesh_refs.timemesh.entries, vec!["entry-1"]);
    }

    #[test]
    fn parse_task_file_reads_lease_mapping() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-004 - lease.md");
        let content = "---\n".to_string()
            + "id: task-004\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "lease:\n"
            + "  owner: agent-1\n"
            + "  acquired_at: 2026-02-03 10:00\n"
            + "  expires_at: 2026-02-03 11:00\n"
            + "---\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        let lease = task.lease.expect("lease");
        assert_eq!(lease.owner, "agent-1");
        assert_eq!(lease.acquired_at.as_deref(), Some("2026-02-03 10:00"));
        assert_eq!(lease.expires_at.as_deref(), Some("2026-02-03 11:00"));
    }

    #[test]
    fn parse_task_file_reads_flat_lease() {
        let temp = TempDir::new().expect("tempdir");
        let file_path = temp.path().join("task-005 - lease-flat.md");
        let content = "---\n".to_string()
            + "id: task-005\n"
            + "title: Example\n"
            + "status: To Do\n"
            + "priority: P2\n"
            + "phase: Phase1\n"
            + "lease_owner: agent-2\n"
            + "lease_acquired_at: 2026-02-03 12:00\n"
            + "lease_expires_at: 2026-02-03 13:00\n"
            + "---\n";
        fs::write(&file_path, content).expect("write");

        let task = parse_task_file(&file_path).expect("parse");
        let lease = task.lease.expect("lease");
        assert_eq!(lease.owner, "agent-2");
        assert_eq!(lease.acquired_at.as_deref(), Some("2026-02-03 12:00"));
        assert_eq!(lease.expires_at.as_deref(), Some("2026-02-03 13:00"));
    }

    #[test]
    fn split_front_matter_errors_when_missing() {
        let err = split_front_matter("no front matter");
        assert!(matches!(err, Err(TaskParseError::MissingFrontMatter)));
    }

    #[test]
    fn parse_front_matter_loose_handles_block_scalars_and_lists() {
        let front = r#"title: >-
  hello
  world
labels:
  - a
  - b
notes: |
  line1
  line2
"#;
        // Call the loose parser directly to exercise the permissive mode.
        let data = parse_front_matter_loose(front);
        assert_eq!(
            data.get("title").and_then(value_to_string).as_deref(),
            Some("hello world")
        );
        let labels = parse_list_value(data.get("labels"));
        assert_eq!(labels, vec!["a", "b"]);
        assert_eq!(
            data.get("notes").and_then(value_to_string).as_deref(),
            Some("line1\nline2")
        );
    }

    #[test]
    fn parse_front_matter_prefers_yaml_parser_when_valid() {
        let front = "\
title: hello\n\
labels: [a, b]\n\
";
        let data = parse_front_matter(front);
        assert_eq!(
            data.get("title").and_then(value_to_string).as_deref(),
            Some("hello")
        );
        assert_eq!(parse_list_value(data.get("labels")), vec!["a", "b"]);
    }

    #[test]
    fn id_from_filename_finds_task_id_or_falls_back_to_stem() {
        let temp = TempDir::new().expect("tempdir");
        let p = temp.path().join("task-123 - hello.md");
        fs::write(&p, "---\n---\n").expect("write");
        assert_eq!(id_from_filename(&p), "task-123");

        let p2 = temp.path().join("weird-name.md");
        fs::write(&p2, "---\n---\n").expect("write");
        assert_eq!(id_from_filename(&p2), "weird-name");
    }

    #[test]
    fn load_tasks_reads_recursive_active_task_and_epic_files() {
        let temp = TempDir::new().expect("tempdir");
        let backlog = temp.path().join("workmesh");
        let tasks_dir = backlog.join("tasks");
        let nested_feature_dir = tasks_dir
            .join("customer-a")
            .join("app-one")
            .join("feature-x");
        fs::create_dir_all(&nested_feature_dir).expect("nested tasks");

        fs::write(
            nested_feature_dir.join("epic-nested-001 - feature container.md"),
            "---\nid: epic-nested-001\nkind: epic\ntitle: Feature container\nstatus: To Do\npriority: P2\nphase: Phase1\n---\n",
        )
        .expect("write epic");
        fs::write(
            nested_feature_dir.join("task-nested-001 - implement feature.md"),
            "---\nid: task-nested-001\nkind: task\ntitle: Implement feature\nstatus: To Do\npriority: P2\nphase: Phase1\nrelationships:\n  parent: [epic-nested-001]\n---\n",
        )
        .expect("write task");

        let tasks = load_tasks(&backlog);
        let ids: Vec<String> = tasks.into_iter().map(|task| task.id).collect();
        assert!(ids.contains(&"epic-nested-001".to_string()));
        assert!(ids.contains(&"task-nested-001".to_string()));
    }

    #[test]
    fn load_tasks_with_archive_reads_recursive_archive_and_skips_invalid() {
        let temp = TempDir::new().expect("tempdir");
        let backlog = temp.path().join("workmesh");
        let tasks_dir = backlog.join("tasks");
        let archive_dir = backlog.join("archive").join("2026-02");
        fs::create_dir_all(&tasks_dir).expect("tasks");
        fs::create_dir_all(&archive_dir).expect("archive");

        // Valid task.
        fs::write(
            tasks_dir.join("task-001 - a.md"),
            "---\nid: task-001\ntitle: A\nstatus: To Do\npriority: P2\nphase: Phase1\n---\n",
        )
        .expect("write");

        // Archived valid task in nested folder.
        fs::write(
            archive_dir.join("task-010 - done.md"),
            "---\nid: task-010\ntitle: Done\nstatus: Done\npriority: P2\nphase: Phase1\n---\n",
        )
        .expect("write");

        // Invalid markdown file should be ignored.
        fs::write(archive_dir.join("task-bad.md"), "no front matter").expect("write");

        let tasks = load_tasks_with_archive(&backlog);
        let ids: Vec<String> = tasks.into_iter().map(|t| t.id).collect();
        assert!(ids.contains(&"task-001".to_string()));
        assert!(ids.contains(&"task-010".to_string()));
        assert!(!ids.contains(&"task-bad".to_string()));
    }
}
