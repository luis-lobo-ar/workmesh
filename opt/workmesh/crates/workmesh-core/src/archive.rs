use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime};
use thiserror::Error;

use crate::task::{archive_root_for_root, tasks_dir_for_root, Task};

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("Missing task path for {0}")]
    MissingPath(String),
    #[error("Failed to move task: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct ArchiveOptions {
    pub before: NaiveDate,
    /// Explicit statuses to archive. When empty, terminal defaults are used.
    pub statuses: Vec<String>,
    /// Explicit task ids to archive. When present, ids bypass status and before filters.
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ArchiveResult {
    pub archived: Vec<String>,
    pub skipped: Vec<String>,
    pub archive_dir: PathBuf,
}

pub fn archive_tasks(
    backlog_dir: &Path,
    tasks: &[Task],
    options: &ArchiveOptions,
) -> Result<ArchiveResult, ArchiveError> {
    let archive_root = archive_root_for_root(backlog_dir);
    let mut archived = Vec::new();
    let mut skipped = Vec::new();
    let requested_task_ids = options
        .task_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let allowed_statuses = if options.statuses.is_empty() {
        default_archive_statuses()
            .iter()
            .map(|value| normalize_status(value))
            .collect::<std::collections::HashSet<_>>()
    } else {
        options
            .statuses
            .iter()
            .map(|value| normalize_status(value))
            .collect::<std::collections::HashSet<_>>()
    };

    for task in tasks {
        let archive_requested_id = !requested_task_ids.is_empty();
        if archive_requested_id {
            if !requested_task_ids.contains(&task.id) {
                continue;
            }
        } else {
            if !allowed_statuses.contains(&normalize_status(&task.status)) {
                continue;
            }
            let task_date = task_date(task).unwrap_or_else(|| Local::now().date_naive());
            if task_date > options.before {
                continue;
            }
        }
        let task_date = task_date(task).unwrap_or_else(|| Local::now().date_naive());
        let path = task
            .file_path
            .as_ref()
            .ok_or_else(|| ArchiveError::MissingPath(task.id.clone()))?;
        let tasks_root = tasks_dir_for_root(backlog_dir);
        let relative_path = path.strip_prefix(&tasks_root).unwrap_or(path.as_path());
        let status_dir = archive_status_dir(&task.status);
        let target = archive_root
            .join(format!("{:04}", task_date.year()))
            .join(format!("{:02}", task_date.month()))
            .join(status_dir)
            .join(relative_path);
        if let Some(target_dir) = target.parent() {
            fs::create_dir_all(target_dir)?;
        }
        fs::rename(path, &target)?;
        archived.push(task.id.clone());
    }

    for task_id in requested_task_ids {
        if !archived.iter().any(|archived_id| archived_id == &task_id) {
            skipped.push(task_id);
        }
    }

    Ok(ArchiveResult {
        archived,
        skipped,
        archive_dir: archive_root,
    })
}

pub fn default_archive_statuses() -> &'static [&'static str] {
    &["Done", "Cancelled", "Canceled", "Won't Do", "Wont Do"]
}

fn normalize_status(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn archive_status_dir(status: &str) -> &'static str {
    match normalize_status(status).as_str() {
        "done" => "done",
        "cancelled" | "canceled" => "cancelled",
        "wontdo" => "wont-do",
        "todo" => "to-do",
        _ => "other",
    }
}

fn parse_task_date(value: &str) -> Option<NaiveDate> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(date);
    }
    if let Ok(date_time) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M") {
        return Some(date_time.date());
    }
    None
}

fn task_date(task: &Task) -> Option<NaiveDate> {
    if let Some(value) = task.updated_date.as_deref().and_then(parse_task_date) {
        return Some(value);
    }
    if let Some(value) = task.created_date.as_deref().and_then(parse_task_date) {
        return Some(value);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::load_tasks;
    use crate::task_ops::create_task_file;
    use tempfile::TempDir;

    fn dir_contains_file_starting_with(dir: &Path, prefix: &str) -> bool {
        fs::read_dir(dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
    }

    #[test]
    fn archive_moves_done_tasks_by_month() {
        let temp = TempDir::new().expect("tempdir");
        let backlog_dir = temp.path().join("workmesh");
        let tasks_dir = backlog_dir.join("tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");

        let _ = create_task_file(
            &tasks_dir,
            "task-001",
            "Done Task",
            "Done",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create");

        let mut tasks = load_tasks(&backlog_dir);
        for task in &mut tasks {
            task.updated_date = Some("2024-01-15 10:00".to_string());
        }

        let result = archive_tasks(
            &backlog_dir,
            &tasks,
            &ArchiveOptions {
                before: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
                statuses: vec!["Done".to_string()],
                task_ids: Vec::new(),
            },
        )
        .expect("archive");

        assert_eq!(result.archived, vec!["task-001".to_string()]);
        let archive_dir = backlog_dir
            .join("archive")
            .join("2024")
            .join("01")
            .join("done");
        assert!(dir_contains_file_starting_with(
            &archive_dir,
            "task-001 - done task"
        ));
    }

    #[test]
    fn archive_preserves_nested_relative_task_path_under_date_and_status() {
        let temp = TempDir::new().expect("tempdir");
        let backlog_dir = temp.path().join("workmesh");
        let nested_dir = backlog_dir
            .join("tasks")
            .join("customer-a")
            .join("project-b")
            .join("app-c")
            .join("feature-d");
        fs::create_dir_all(&nested_dir).expect("nested tasks dir");

        let _ = create_task_file(
            &nested_dir,
            "task-archive-001",
            "Nested Done",
            "Done",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create nested done");

        let mut tasks = load_tasks(&backlog_dir);
        for task in &mut tasks {
            task.updated_date = Some("2024-02-20 10:00".to_string());
        }

        archive_tasks(
            &backlog_dir,
            &tasks,
            &ArchiveOptions {
                before: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
                statuses: vec!["Done".to_string()],
                task_ids: Vec::new(),
            },
        )
        .expect("archive");

        assert!(!dir_contains_file_starting_with(
            &nested_dir,
            "task-archive-001 - nested done"
        ));
        let archive_dir = backlog_dir
            .join("archive")
            .join("2024")
            .join("02")
            .join("done")
            .join("customer-a")
            .join("project-b")
            .join("app-c")
            .join("feature-d");
        assert!(dir_contains_file_starting_with(
            &archive_dir,
            "task-archive-001 - nested done"
        ));
    }

    #[test]
    fn archive_normalizes_terminal_status_directories() {
        let temp = TempDir::new().expect("tempdir");
        let backlog_dir = temp.path().join("workmesh");
        let tasks_dir = backlog_dir.join("tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");

        let _ = create_task_file(
            &tasks_dir,
            "task-cancelled-001",
            "Cancelled Task",
            "Canceled",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create canceled");
        let _ = create_task_file(
            &tasks_dir,
            "task-wontdo-001",
            "Wontdo Task",
            "Won't Do",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create wont do");

        let mut tasks = load_tasks(&backlog_dir);
        for task in &mut tasks {
            task.updated_date = Some("2024-03-15 10:00".to_string());
        }

        archive_tasks(
            &backlog_dir,
            &tasks,
            &ArchiveOptions {
                before: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
                statuses: Vec::new(),
                task_ids: Vec::new(),
            },
        )
        .expect("archive");

        let cancelled_dir = backlog_dir
            .join("archive")
            .join("2024")
            .join("03")
            .join("cancelled");
        let wont_do_dir = backlog_dir
            .join("archive")
            .join("2024")
            .join("03")
            .join("wont-do");
        assert!(dir_contains_file_starting_with(
            &cancelled_dir,
            "task-cancelled-001 - cancelled task"
        ));
        assert!(dir_contains_file_starting_with(
            &wont_do_dir,
            "task-wontdo-001 - wontdo task"
        ));
    }

    #[test]
    fn archive_defaults_to_terminal_statuses_only() {
        let temp = TempDir::new().expect("tempdir");
        let backlog_dir = temp.path().join("workmesh");
        let tasks_dir = backlog_dir.join("tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");

        let _ = create_task_file(
            &tasks_dir,
            "task-001",
            "Done Task",
            "Done",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create done");
        let _ = create_task_file(
            &tasks_dir,
            "task-002",
            "Cancelled Task",
            "Cancelled",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create cancelled");
        let _ = create_task_file(
            &tasks_dir,
            "task-003",
            "Todo Task",
            "To Do",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create todo");

        let mut tasks = load_tasks(&backlog_dir);
        for task in &mut tasks {
            task.updated_date = Some("2024-01-15 10:00".to_string());
        }

        let result = archive_tasks(
            &backlog_dir,
            &tasks,
            &ArchiveOptions {
                before: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
                statuses: Vec::new(),
                task_ids: Vec::new(),
            },
        )
        .expect("archive");

        assert_eq!(
            result.archived,
            vec!["task-001".to_string(), "task-002".to_string()]
        );
        let todo_still_present = fs::read_dir(&tasks_dir)
            .expect("read tasks")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with("task-003")
            });
        assert!(todo_still_present);
    }

    #[test]
    fn archive_explicit_task_ids_bypass_status_and_before_filters() {
        let temp = TempDir::new().expect("tempdir");
        let backlog_dir = temp.path().join("workmesh");
        let tasks_dir = backlog_dir.join("tasks");
        fs::create_dir_all(&tasks_dir).expect("tasks dir");

        let _ = create_task_file(
            &tasks_dir,
            "task-001",
            "Done Task",
            "Done",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create done");
        let _ = create_task_file(
            &tasks_dir,
            "task-002",
            "Todo Task",
            "To Do",
            "P2",
            "Phase1",
            &[],
            &[],
            &[],
        )
        .expect("create todo");

        let mut tasks = load_tasks(&backlog_dir);
        for task in &mut tasks {
            task.updated_date = Some("2099-01-15 10:00".to_string());
        }

        let result = archive_tasks(
            &backlog_dir,
            &tasks,
            &ArchiveOptions {
                before: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
                statuses: Vec::new(),
                task_ids: vec!["task-002".to_string(), "task-missing".to_string()],
            },
        )
        .expect("archive");

        assert_eq!(result.archived, vec!["task-002".to_string()]);
        assert_eq!(result.skipped, vec!["task-missing".to_string()]);
        assert!(fs::read_dir(&tasks_dir)
            .expect("read tasks")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("task-001 - done task")));
        assert!(!fs::read_dir(&tasks_dir)
            .expect("read tasks")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("task-002 - todo task")));
        let archive_dir = backlog_dir
            .join("archive")
            .join("2099")
            .join("01")
            .join("to-do");
        assert!(dir_contains_file_starting_with(
            &archive_dir,
            "task-002 - todo task"
        ));
    }
}
