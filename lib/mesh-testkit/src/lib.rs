use futures_util::{Stream, StreamExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureRoot(pub PathBuf);

impl FixtureRoot {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self(path.as_ref().to_path_buf())
    }

    pub fn child(&self, name: impl AsRef<Path>) -> PathBuf {
        self.0.join(name)
    }
}

#[derive(Debug)]
pub struct TempFixture(tempfile::TempDir);

impl TempFixture {
    pub fn new(prefix: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("create temp fixture");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    pub fn root(&self) -> FixtureRoot {
        FixtureRoot::new(self.path())
    }
}

pub async fn next_stream_chunk_text<S, T, E>(
    stream: &mut S,
    timeout_duration: Duration,
) -> Result<String, String>
where
    S: Stream<Item = Result<T, E>> + Unpin,
    T: AsRef<[u8]>,
    E: std::fmt::Display,
{
    let item = tokio::time::timeout(timeout_duration, stream.next())
        .await
        .map_err(|_| format!("stream chunk timeout after {:?}", timeout_duration))?
        .ok_or_else(|| "stream ended before producing a chunk".to_string())?
        .map_err(|err| format!("stream chunk error: {err}"))?;

    std::str::from_utf8(item.as_ref())
        .map(|text| text.to_string())
        .map_err(|err| format!("stream chunk not utf8: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use std::time::Duration;

    #[test]
    fn fixture_root_builds_child_paths() {
        let root = FixtureRoot::new("fixtures");
        assert_eq!(
            root.child("events.jsonl"),
            PathBuf::from("fixtures/events.jsonl")
        );
    }

    #[test]
    fn temp_fixture_creates_named_root() {
        let fixture = TempFixture::new("mesh-testkit");
        assert!(fixture.path().exists());
        assert!(fixture.path().to_string_lossy().contains("mesh-testkit"));
    }

    #[tokio::test]
    async fn next_stream_chunk_text_collects_utf8_payload() {
        let mut values = stream::iter(vec![Ok::<Vec<u8>, std::io::Error>(
            b"event: ping\n\n".to_vec(),
        )]);
        let text = next_stream_chunk_text(&mut values, Duration::from_secs(1))
            .await
            .expect("chunk");
        assert_eq!(text, "event: ping\n\n");
    }
}
