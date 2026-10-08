use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_LOCK_TIMEOUT: Duration = mesh_store::DEFAULT_LOCK_TIMEOUT;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VersionedState<T> {
    pub version: u64,
    pub updated_at: String,
    pub payload: T,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[error(
    "storage conflict at {path}: expected version {expected_version}, actual version {actual_version}"
)]
pub struct StorageConflict {
    pub path: PathBuf,
    pub expected_version: u64,
    pub actual_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceKey {
    RepoLocal {
        backlog_dir: PathBuf,
        resource: String,
    },
    Global {
        workmesh_home: PathBuf,
        resource: String,
    },
    Path(PathBuf),
}

impl ResourceKey {
    pub fn repo_local(backlog_dir: &Path, resource: &str) -> Self {
        Self::RepoLocal {
            backlog_dir: backlog_dir.to_path_buf(),
            resource: resource.to_string(),
        }
    }

    pub fn global(workmesh_home: &Path, resource: &str) -> Self {
        Self::Global {
            workmesh_home: workmesh_home.to_path_buf(),
            resource: resource.to_string(),
        }
    }

    pub fn path(path: &Path) -> Self {
        Self::Path(path.to_path_buf())
    }

    fn lock_path(&self) -> PathBuf {
        match self {
            ResourceKey::RepoLocal {
                backlog_dir,
                resource,
            } => backlog_dir
                .join(".locks")
                .join(format!("{}.lock", sanitize_lock_component(resource))),
            ResourceKey::Global {
                workmesh_home,
                resource,
            } => workmesh_home
                .join(".locks")
                .join(format!("{}.lock", sanitize_lock_component(resource))),
            ResourceKey::Path(path) => {
                let parent = path.parent().unwrap_or_else(|| Path::new("."));
                let file_component = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(sanitize_lock_component)
                    .unwrap_or_else(|| "resource".to_string());
                parent
                    .join(".locks")
                    .join(format!("{}.lock", file_component))
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("serialize json for {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("deserialize json from {path}: {source}")]
    Deserialize {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid jsonl line for {path}: {source}")]
    InvalidJsonLine {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("timed out acquiring lock {lock_path} after {timeout_ms}ms")]
    LockTimeout { lock_path: PathBuf, timeout_ms: u64 },
    #[error(transparent)]
    Conflict(#[from] StorageConflict),
}

impl From<mesh_store::StoreError> for StorageError {
    fn from(err: mesh_store::StoreError) -> Self {
        match err {
            mesh_store::StoreError::Io(source) => Self::Io(source),
            mesh_store::StoreError::Deserialize { path, source } => {
                Self::Deserialize { path, source }
            }
            mesh_store::StoreError::InvalidJsonLine { path, source } => {
                Self::InvalidJsonLine { path, source }
            }
            mesh_store::StoreError::LockTimeout {
                lock_path,
                timeout_ms,
            } => Self::LockTimeout {
                lock_path,
                timeout_ms,
            },
            mesh_store::StoreError::NotImplemented => {
                Self::Io(io::Error::other("mesh-store primitive not implemented"))
            }
        }
    }
}

pub fn with_resource_lock<T, F>(
    resource_key: &ResourceKey,
    timeout: Duration,
    action: F,
) -> Result<T, StorageError>
where
    F: FnOnce() -> Result<T, StorageError>,
{
    with_resource_lock_result(resource_key, timeout, action)
}

pub fn with_resource_lock_result<T, E, F>(
    resource_key: &ResourceKey,
    timeout: Duration,
    action: F,
) -> Result<T, E>
where
    E: From<StorageError>,
    F: FnOnce() -> Result<T, E>,
{
    let _lock = mesh_store::acquire_lock_path(&resource_key.lock_path(), timeout)
        .map_err(StorageError::from)?;
    action()
}

pub fn atomic_write_json<T>(path: &Path, value: &T) -> Result<(), StorageError>
where
    T: Serialize + ?Sized,
{
    let body = serde_json::to_string_pretty(value).map_err(|source| StorageError::Serialize {
        path: path.to_path_buf(),
        source,
    })?;
    atomic_write_text(path, &body)
}

pub fn atomic_write_text(path: &Path, text: &str) -> Result<(), StorageError> {
    mesh_store::atomic_write_text(path, text).map_err(Into::into)
}

pub fn append_jsonl_locked(path: &Path, line: &str) -> Result<(), StorageError> {
    append_jsonl_locked_with_key(path, line, &ResourceKey::path(path))
}

pub fn append_jsonl_locked_with_key(
    path: &Path,
    line: &str,
    resource_key: &ResourceKey,
) -> Result<(), StorageError> {
    serde_json::from_str::<serde_json::Value>(line).map_err(|source| {
        StorageError::InvalidJsonLine {
            path: path.to_path_buf(),
            source,
        }
    })?;

    with_resource_lock(resource_key, DEFAULT_LOCK_TIMEOUT, || {
        append_line_unchecked(path, line)?;
        Ok(())
    })
}

pub fn read_modify_write_json<T, F>(path: &Path, merge_fn: F) -> Result<T, StorageError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(Option<T>) -> Result<T, StorageError>,
{
    read_modify_write_json_with_key(path, &ResourceKey::path(path), merge_fn)
}

pub fn read_modify_write_json_with_key<T, F>(
    path: &Path,
    resource_key: &ResourceKey,
    merge_fn: F,
) -> Result<T, StorageError>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce(Option<T>) -> Result<T, StorageError>,
{
    with_resource_lock(resource_key, DEFAULT_LOCK_TIMEOUT, || {
        let current = read_json_optional(path)?;
        let next = merge_fn(current)?;
        atomic_write_json(path, &next)?;
        Ok(next)
    })
}

pub fn cas_update_json<T>(
    path: &Path,
    expected_version: u64,
    next_payload: T,
) -> Result<VersionedState<T>, StorageError>
where
    T: Serialize + DeserializeOwned,
{
    cas_update_json_with_key(
        path,
        &ResourceKey::path(path),
        expected_version,
        next_payload,
    )
}

pub fn cas_update_json_with_key<T>(
    path: &Path,
    resource_key: &ResourceKey,
    expected_version: u64,
    next_payload: T,
) -> Result<VersionedState<T>, StorageError>
where
    T: Serialize + DeserializeOwned,
{
    with_resource_lock(resource_key, DEFAULT_LOCK_TIMEOUT, || {
        let current = read_versioned_or_legacy_json::<T>(path)?;
        let actual_version = current.as_ref().map(|state| state.version).unwrap_or(0);
        if actual_version != expected_version {
            return Err(StorageError::Conflict(StorageConflict {
                path: path.to_path_buf(),
                expected_version,
                actual_version,
            }));
        }

        let next_state = VersionedState {
            version: actual_version.saturating_add(1),
            updated_at: now_rfc3339(),
            payload: next_payload,
        };
        atomic_write_json(path, &next_state)?;
        Ok(next_state)
    })
}

pub type JsonlReadResult<T> = mesh_store::JsonlReadResult<T>;

pub fn read_jsonl_tolerant<T>(path: &Path) -> Result<JsonlReadResult<T>, StorageError>
where
    T: DeserializeOwned,
{
    mesh_store::read_jsonl_tolerant(path).map_err(Into::into)
}

pub fn truncate_jsonl_trailing_invalid(path: &Path) -> Result<usize, StorageError> {
    mesh_store::truncate_jsonl_trailing_invalid(path).map_err(Into::into)
}

pub fn with_path_lock<T, E, F>(path: &Path, action: F) -> Result<T, E>
where
    E: From<io::Error>,
    F: FnOnce() -> Result<T, E>,
{
    let lock_path = ResourceKey::path(path).lock_path();
    let _lock = mesh_store::acquire_lock_path(&lock_path, DEFAULT_LOCK_TIMEOUT)
        .map_err(|err| E::from(storage_error_to_io(err.into())))?;
    action()
}

pub fn with_path_lock_io<T, F>(path: &Path, action: F) -> io::Result<T>
where
    F: FnOnce() -> io::Result<T>,
{
    with_path_lock(path, action)
}

pub fn write_string_atomic(path: &Path, body: &str) -> io::Result<()> {
    atomic_write_text(path, body).map_err(storage_error_to_io)
}

pub fn write_string_atomic_locked(path: &Path, body: &str) -> io::Result<()> {
    with_path_lock_io(path, || write_string_atomic(path, body))
}

pub fn append_line_locked(path: &Path, line: &str) -> io::Result<()> {
    mesh_store::append_line_locked(path, line).map_err(|err| storage_error_to_io(err.into()))
}

fn append_line_unchecked(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    use std::io::Write as _;
    writeln!(file, "{}", line)?;
    file.sync_data()?;
    Ok(())
}

fn read_json_optional<T>(path: &Path) -> Result<Option<T>, StorageError>
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let parsed = serde_json::from_str::<T>(&raw).map_err(|source| StorageError::Deserialize {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(parsed))
}

pub fn read_versioned_or_legacy_json<T>(
    path: &Path,
) -> Result<Option<VersionedState<T>>, StorageError>
where
    T: Serialize + DeserializeOwned,
{
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(None);
    }

    if let Ok(versioned) = serde_json::from_str::<VersionedState<T>>(&raw) {
        return Ok(Some(versioned));
    }

    let legacy = serde_json::from_str::<T>(&raw).map_err(|source| StorageError::Deserialize {
        path: path.to_path_buf(),
        source,
    })?;

    Ok(Some(VersionedState {
        version: 0,
        updated_at: now_rfc3339(),
        payload: legacy,
    }))
}

fn sanitize_lock_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "resource".to_string()
    } else {
        trimmed
    }
}

fn storage_error_to_io(err: StorageError) -> io::Error {
    match err {
        StorageError::Io(err) => err,
        StorageError::Serialize { source, .. }
        | StorageError::Deserialize { source, .. }
        | StorageError::InvalidJsonLine { source, .. } => {
            io::Error::new(io::ErrorKind::InvalidData, source.to_string())
        }
        StorageError::LockTimeout {
            lock_path,
            timeout_ms,
        } => io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "timed out acquiring lock {} after {}ms",
                lock_path.display(),
                timeout_ms
            ),
        ),
        StorageError::Conflict(conflict) => io::Error::new(io::ErrorKind::AlreadyExists, conflict),
    }
}

fn now_rfc3339() -> String {
    let now: DateTime<Utc> = Utc::now();
    now.to_rfc3339()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use super::*;
    use tempfile::TempDir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct CounterPayload {
        value: u64,
    }

    #[test]
    fn lock_namespace_convention_is_stable() {
        let temp = TempDir::new().expect("tempdir");

        let repo_key = ResourceKey::repo_local(temp.path(), "truth/events");
        assert_eq!(
            repo_key.lock_path(),
            temp.path().join(".locks").join("truth_events.lock")
        );

        let global_key = ResourceKey::global(temp.path(), "sessions/events");
        assert_eq!(
            global_key.lock_path(),
            temp.path().join(".locks").join("sessions_events.lock")
        );

        let file_key = ResourceKey::path(&temp.path().join("truth").join("events.jsonl"));
        assert_eq!(
            file_key.lock_path(),
            temp.path()
                .join("truth")
                .join(".locks")
                .join("events.jsonl.lock")
        );
    }

    #[test]
    fn write_string_atomic_locked_writes_file() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("state.json");
        write_string_atomic_locked(&path, "{\n  \"ok\": true\n}\n").expect("write");
        let raw = fs::read_to_string(path).expect("read");
        assert!(raw.contains("\"ok\""));
    }

    #[test]
    fn append_line_locked_creates_and_appends() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        append_line_locked(&path, r#"{"a":1}"#).expect("append 1");
        append_line_locked(&path, r#"{"a":2}"#).expect("append 2");
        let raw = fs::read_to_string(path).expect("read");
        assert_eq!(raw.lines().count(), 2);
    }

    #[test]
    fn append_jsonl_locked_rejects_invalid_json() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        let err = append_jsonl_locked(&path, "not-json").expect_err("must fail");
        assert!(matches!(err, StorageError::InvalidJsonLine { .. }));
    }

    #[test]
    fn with_path_lock_serializes_read_modify_write_updates() {
        let temp = TempDir::new().expect("tempdir");
        let path = Arc::new(temp.path().join("counter.txt"));
        write_string_atomic(path.as_ref(), "0\n").expect("seed");

        #[cfg(windows)]
        let (workers, increments_per_worker) = (4usize, 8usize);
        #[cfg(not(windows))]
        let (workers, increments_per_worker) = (6usize, 20usize);
        let mut handles = Vec::new();

        for _ in 0..workers {
            let shared_path = Arc::clone(&path);
            handles.push(thread::spawn(move || -> io::Result<()> {
                for _ in 0..increments_per_worker {
                    with_path_lock_io(shared_path.as_ref(), || {
                        let current = fs::read_to_string(shared_path.as_ref())?;
                        let parsed = current.trim().parse::<usize>().map_err(|err| {
                            io::Error::new(io::ErrorKind::InvalidData, err.to_string())
                        })?;
                        write_string_atomic(shared_path.as_ref(), &format!("{}\n", parsed + 1))?;
                        Ok(())
                    })?;
                }
                Ok(())
            }));
        }

        for handle in handles {
            handle.join().expect("join").expect("locked update");
        }

        let final_value = fs::read_to_string(path.as_ref())
            .expect("read final")
            .trim()
            .parse::<usize>()
            .expect("parse");
        assert_eq!(final_value, workers * increments_per_worker);
    }

    #[test]
    fn with_resource_lock_times_out_when_held() {
        let temp = TempDir::new().expect("tempdir");
        let key = ResourceKey::repo_local(temp.path(), "resource-a");
        let barrier = Arc::new(Barrier::new(2));

        let hold_barrier = Arc::clone(&barrier);
        let holder_key = key.clone();
        let holder = thread::spawn(move || {
            with_resource_lock(&holder_key, Duration::from_secs(1), || {
                hold_barrier.wait();
                thread::sleep(Duration::from_millis(200));
                Ok(())
            })
            .expect("holder lock");
        });

        barrier.wait();
        let contender = with_resource_lock(&key, Duration::from_millis(20), || Ok(()));
        assert!(matches!(contender, Err(StorageError::LockTimeout { .. })));

        holder.join().expect("join holder");
    }

    #[test]
    fn cas_update_json_detects_stale_version() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("state.json");

        let seeded = cas_update_json(&path, 0, CounterPayload { value: 1 }).expect("seed");
        assert_eq!(seeded.version, 1);

        let updated = cas_update_json(&path, 1, CounterPayload { value: 2 }).expect("update");
        assert_eq!(updated.version, 2);

        let stale = cas_update_json(&path, 1, CounterPayload { value: 3 }).expect_err("stale");
        match stale {
            StorageError::Conflict(conflict) => {
                assert_eq!(conflict.expected_version, 1);
                assert_eq!(conflict.actual_version, 2);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn read_modify_write_json_updates_payload_atomically() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("payload.json");

        let next = read_modify_write_json::<CounterPayload, _>(&path, |current| {
            let mut payload = current.unwrap_or(CounterPayload { value: 0 });
            payload.value += 1;
            Ok(payload)
        })
        .expect("update");
        assert_eq!(next.value, 1);

        let reread: CounterPayload =
            serde_json::from_str(&fs::read_to_string(&path).expect("read payload"))
                .expect("parse payload");
        assert_eq!(reread.value, 1);
    }

    #[test]
    fn cas_update_json_migrates_unversioned_payload() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("legacy.json");

        let legacy = CounterPayload { value: 5 };
        let legacy_raw = serde_json::to_string_pretty(&legacy).expect("serialize legacy");
        write_string_atomic(&path, &legacy_raw).expect("seed legacy");

        let migrated = cas_update_json(&path, 0, CounterPayload { value: 6 }).expect("migrate");
        assert_eq!(migrated.version, 1);
        assert_eq!(migrated.payload.value, 6);

        let stored: VersionedState<CounterPayload> =
            serde_json::from_str(&fs::read_to_string(&path).expect("read migrated"))
                .expect("parse migrated");
        assert_eq!(stored.version, 1);
        assert_eq!(stored.payload.value, 6);
    }

    #[test]
    fn read_jsonl_tolerant_ignores_trailing_malformed_lines() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        let payload = "{\n".to_string();
        write_string_atomic(
            &path,
            &format!("{}\n{}\n", r#"{"value":1}"#, payload.trim_end_matches('\n')),
        )
        .expect("seed");

        let result = read_jsonl_tolerant::<CounterPayload>(&path).expect("read");
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].value, 1);
        assert_eq!(result.malformed_trailing_lines, 1);
    }

    #[test]
    fn read_jsonl_tolerant_rejects_non_trailing_malformed_line() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        write_string_atomic(&path, "{\"value\":1}\n{\n{\"value\":2}\n").expect("seed");

        let err = read_jsonl_tolerant::<CounterPayload>(&path).expect_err("must fail");
        assert!(matches!(err, StorageError::Deserialize { .. }));
    }

    #[test]
    fn truncate_jsonl_trailing_invalid_only_trims_tail() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        write_string_atomic(&path, "{\"value\":1}\n{\"value\":2}\n{\n").expect("seed");

        let removed = truncate_jsonl_trailing_invalid(&path).expect("truncate");
        assert_eq!(removed, 1);
        let raw = fs::read_to_string(&path).expect("read");
        assert_eq!(raw, "{\"value\":1}\n{\"value\":2}\n");
    }
}
