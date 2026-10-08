use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};

pub mod ha;
pub mod idempotency;
pub mod lease;

pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("store primitive not implemented yet")]
    NotImplemented,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("deserialize json from {path}: {source}")]
    Deserialize {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid jsonl line for {path}: {source}")]
    InvalidJsonLine {
        path: std::path::PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("timed out acquiring lock {lock_path} after {timeout_ms}ms")]
    LockTimeout {
        lock_path: std::path::PathBuf,
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlReadResult<T> {
    pub records: Vec<T>,
    pub malformed_trailing_lines: usize,
}

pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn acquire_lock_path(lock_path: &Path, timeout: Duration) -> Result<LockGuard, StoreError> {
    acquire_lock(lock_path, timeout)
}

pub fn with_lock_path<T, E, F>(lock_path: &Path, timeout: Duration, action: F) -> Result<T, E>
where
    E: From<StoreError>,
    F: FnOnce() -> Result<T, E>,
{
    let _lock = acquire_lock_path(lock_path, timeout)?;
    action()
}

pub fn with_path_lock<T, E, F>(path: &Path, timeout: Duration, action: F) -> Result<T, E>
where
    E: From<StoreError>,
    F: FnOnce() -> Result<T, E>,
{
    let lock_path = sibling_lock_path(path);
    with_lock_path(&lock_path, timeout, action)
}

pub fn with_path_lock_io<T, F>(path: &Path, timeout: Duration, action: F) -> io::Result<T>
where
    F: FnOnce() -> io::Result<T>,
{
    let lock_path = sibling_lock_path(path);
    let _lock = acquire_lock(&lock_path, timeout).map_err(store_error_to_io)?;
    action()
}

pub fn atomic_write_text(path: &Path, text: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = temp_path(path);
    {
        let mut file = File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }

    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(StoreError::Io(err));
    }

    sync_parent_dir(path)?;
    Ok(())
}

pub fn append_line_locked(path: &Path, line: &str) -> Result<(), StoreError> {
    with_path_lock(path, DEFAULT_LOCK_TIMEOUT, || {
        append_line_unchecked(path, line)?;
        Ok(())
    })
}

pub fn read_jsonl_tolerant<T>(path: &Path) -> Result<JsonlReadResult<T>, StoreError>
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return Ok(JsonlReadResult {
            records: Vec::new(),
            malformed_trailing_lines: 0,
        });
    }

    let raw = fs::read_to_string(path)?;
    parse_jsonl_tolerant(path, &raw)
}

pub fn truncate_jsonl_trailing_invalid(path: &Path) -> Result<usize, StoreError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(StoreError::Io(err)),
    };

    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return Ok(0);
    }

    let mut malformed_at: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(line.trim()).is_err() {
            let trailing_only_invalid = lines[idx + 1..]
                .iter()
                .filter(|candidate| !candidate.trim().is_empty())
                .all(|candidate| {
                    serde_json::from_str::<serde_json::Value>(candidate.trim()).is_err()
                });
            if trailing_only_invalid {
                malformed_at = Some(idx);
                break;
            }
            return Err(StoreError::InvalidJsonLine {
                path: path.to_path_buf(),
                source: serde_json::Error::io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("non-trailing malformed jsonl line {}", idx + 1),
                )),
            });
        }
    }

    let Some(start) = malformed_at else {
        return Ok(0);
    };
    let removed = lines[start..]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();
    let kept = lines[..start]
        .iter()
        .filter(|line| !line.trim().is_empty())
        .copied()
        .collect::<Vec<_>>();
    let payload = if kept.is_empty() {
        String::new()
    } else {
        let mut body = kept.join("\n");
        body.push('\n');
        body
    };
    atomic_write_text(path, &payload)?;
    Ok(removed)
}

pub fn atomic_write_json<T>(path: &Path, value: &T) -> Result<(), StoreError>
where
    T: Serialize + ?Sized,
{
    let body = serde_json::to_string_pretty(value).map_err(|source| StoreError::Deserialize {
        path: path.to_path_buf(),
        source,
    })?;
    atomic_write_text(path, &body)
}

pub fn read_json_or_default<T>(path: &Path) -> Result<T, StoreError>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|source| StoreError::Deserialize {
        path: path.to_path_buf(),
        source,
    })
}

pub fn append_jsonl_record<T>(path: &Path, value: &T) -> Result<(), StoreError>
where
    T: Serialize,
{
    let line = serde_json::to_string(value).map_err(|source| StoreError::Deserialize {
        path: path.to_path_buf(),
        source,
    })?;
    append_line_locked(path, &line)
}

pub fn write_jsonl_records<T>(path: &Path, records: &[T]) -> Result<(), StoreError>
where
    T: Serialize,
{
    let mut body = String::new();
    for record in records {
        let line = serde_json::to_string(record).map_err(|source| StoreError::Deserialize {
            path: path.to_path_buf(),
            source,
        })?;
        body.push_str(&line);
        body.push('\n');
    }
    atomic_write_text(path, &body)
}

fn append_line_unchecked(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", line)?;
    file.sync_data()?;
    Ok(())
}

fn parse_jsonl_tolerant<T>(path: &Path, raw: &str) -> Result<JsonlReadResult<T>, StoreError>
where
    T: DeserializeOwned,
{
    let lines = raw.lines().collect::<Vec<_>>();
    let mut records = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let trimmed = lines[idx].trim();
        idx += 1;
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<T>(trimmed) {
            Ok(value) => records.push(value),
            Err(source) => {
                let trailing_only_invalid = lines[idx..]
                    .iter()
                    .filter(|candidate| !candidate.trim().is_empty())
                    .all(|candidate| {
                        serde_json::from_str::<serde_json::Value>(candidate.trim()).is_err()
                    });
                if !trailing_only_invalid {
                    return Err(StoreError::Deserialize {
                        path: path.to_path_buf(),
                        source,
                    });
                }
                let malformed = 1usize
                    + lines[idx..]
                        .iter()
                        .filter(|candidate| !candidate.trim().is_empty())
                        .count();
                return Ok(JsonlReadResult {
                    records,
                    malformed_trailing_lines: malformed,
                });
            }
        }
    }

    Ok(JsonlReadResult {
        records,
        malformed_trailing_lines: 0,
    })
}

fn acquire_lock(lock_path: &Path, timeout: Duration) -> Result<LockGuard, StoreError> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;

    let start = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(LockGuard { file }),
            Err(err) if is_lock_contention(&err) => {
                if start.elapsed() >= timeout {
                    return Err(StoreError::LockTimeout {
                        lock_path: lock_path.to_path_buf(),
                        timeout_ms: timeout.as_millis() as u64,
                    });
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(err) => return Err(StoreError::Io(err)),
        }
    }
}

fn is_lock_contention(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::WouldBlock {
        return true;
    }

    #[cfg(windows)]
    {
        return matches!(err.raw_os_error(), Some(33 | 32));
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn sync_parent_dir(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };

    #[cfg(unix)]
    {
        let dir = File::open(parent)?;
        dir.sync_all()?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        if let Err(err) = File::open(parent).and_then(|dir| dir.sync_all()) {
            if err.kind() != io::ErrorKind::PermissionDenied
                && err.kind() != io::ErrorKind::InvalidInput
            {
                return Err(err);
            }
        }
        Ok(())
    }
}

fn sibling_lock_path(path: &Path) -> PathBuf {
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

fn temp_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("mesh-store");
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(".{}.{}.{}.tmp", stem, pid, nanos))
}

fn store_error_to_io(err: StoreError) -> io::Error {
    match err {
        StoreError::Io(err) => err,
        StoreError::Deserialize { source, .. } | StoreError::InvalidJsonLine { source, .. } => {
            io::Error::new(io::ErrorKind::InvalidData, source.to_string())
        }
        StoreError::LockTimeout {
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
        StoreError::NotImplemented => io::Error::other("store primitive not implemented yet"),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;

    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
    struct CounterPayload {
        value: u64,
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
    fn with_path_lock_serializes_read_modify_write_updates() {
        let temp = TempDir::new().expect("tempdir");
        let path = Arc::new(temp.path().join("counter.txt"));
        atomic_write_text(path.as_ref(), "0\n").expect("seed");

        let (workers, increments_per_worker) = (4usize, 8usize);
        let mut handles = Vec::new();

        for _ in 0..workers {
            let shared_path = Arc::clone(&path);
            handles.push(thread::spawn(move || -> io::Result<()> {
                for _ in 0..increments_per_worker {
                    with_path_lock_io(shared_path.as_ref(), DEFAULT_LOCK_TIMEOUT, || {
                        let current = fs::read_to_string(shared_path.as_ref())?;
                        let parsed = current.trim().parse::<usize>().map_err(|err| {
                            io::Error::new(io::ErrorKind::InvalidData, err.to_string())
                        })?;
                        atomic_write_text(shared_path.as_ref(), &format!("{}\n", parsed + 1))
                            .map_err(store_error_to_io)?;
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
    fn with_lock_path_times_out_when_held() {
        let temp = TempDir::new().expect("tempdir");
        let lock_path = temp.path().join(".locks").join("resource.lock");
        let barrier = Arc::new(Barrier::new(2));

        let hold_barrier = Arc::clone(&barrier);
        let holder_lock = lock_path.clone();
        let holder = thread::spawn(move || {
            with_lock_path(
                &holder_lock,
                Duration::from_secs(1),
                || -> Result<(), StoreError> {
                    hold_barrier.wait();
                    thread::sleep(Duration::from_millis(200));
                    Ok(())
                },
            )
            .expect("holder lock");
        });

        barrier.wait();
        let contender = with_lock_path(&lock_path, Duration::from_millis(20), || {
            Ok::<(), StoreError>(())
        });
        assert!(matches!(contender, Err(StoreError::LockTimeout { .. })));

        holder.join().expect("join holder");
    }

    #[test]
    fn read_jsonl_tolerant_ignores_trailing_malformed_lines() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        atomic_write_text(&path, "{\"value\":1}\n{\n").expect("seed");

        let result = read_jsonl_tolerant::<CounterPayload>(&path).expect("read");
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].value, 1);
        assert_eq!(result.malformed_trailing_lines, 1);
    }

    #[test]
    fn read_jsonl_tolerant_rejects_non_trailing_malformed_line() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        atomic_write_text(&path, "{\"value\":1}\n{\n{\"value\":2}\n").expect("seed");

        let err = read_jsonl_tolerant::<CounterPayload>(&path).expect_err("must fail");
        assert!(matches!(err, StoreError::Deserialize { .. }));
    }

    #[test]
    fn truncate_jsonl_trailing_invalid_only_trims_tail() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("events.jsonl");
        atomic_write_text(&path, "{\"value\":1}\n{\"value\":2}\n{\n").expect("seed");

        let removed = truncate_jsonl_trailing_invalid(&path).expect("truncate");
        assert_eq!(removed, 1);
        let raw = fs::read_to_string(&path).expect("read");
        assert_eq!(raw, "{\"value\":1}\n{\"value\":2}\n");
    }

    #[test]
    fn atomic_write_json_and_read_json_or_default_round_trip() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("state.json");
        let payload = CounterPayload { value: 42 };

        atomic_write_json(&path, &payload).expect("write json");
        let loaded: CounterPayload = read_json_or_default(&path).expect("read json");
        assert_eq!(loaded, payload);
    }

    #[test]
    fn read_json_or_default_returns_default_for_missing_path() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("missing.json");

        let loaded: Vec<CounterPayload> = read_json_or_default(&path).expect("read default");
        assert!(loaded.is_empty());
    }

    #[test]
    fn append_jsonl_record_and_write_jsonl_records_round_trip() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join("records.jsonl");

        append_jsonl_record(&path, &CounterPayload { value: 1 }).expect("append first");
        append_jsonl_record(&path, &CounterPayload { value: 2 }).expect("append second");
        let loaded = read_jsonl_tolerant::<CounterPayload>(&path).expect("read appended");
        assert_eq!(
            loaded.records,
            vec![CounterPayload { value: 1 }, CounterPayload { value: 2 }]
        );

        write_jsonl_records(
            &path,
            &[CounterPayload { value: 3 }, CounterPayload { value: 4 }],
        )
        .expect("rewrite");
        let rewritten = read_jsonl_tolerant::<CounterPayload>(&path).expect("read rewritten");
        assert_eq!(
            rewritten.records,
            vec![CounterPayload { value: 3 }, CounterPayload { value: 4 }]
        );
    }
}
