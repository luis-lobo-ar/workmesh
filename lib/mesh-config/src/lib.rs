use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigSource {
    Default,
    File(String),
    Environment(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigValue {
    pub key: String,
    pub value: String,
    pub source: ConfigSource,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

pub fn env_var_name(prefix: &str, key: &str) -> String {
    format!(
        "{}_{}",
        prefix.to_ascii_uppercase(),
        key.replace(['.', '-'], "_").to_ascii_uppercase()
    )
}

pub fn resolve_user_home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let trimmed = profile.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    None
}

pub fn resolve_scoped_home_dir(env_var: &str, fallback_subdir: &str) -> Option<PathBuf> {
    if let Ok(value) = std::env::var(env_var) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    resolve_user_home_dir().map(|home| home.join(fallback_subdir))
}

pub fn find_config_root(start: &Path, candidates: &[&str]) -> Option<PathBuf> {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    for candidate in start.ancestors() {
        if candidates.iter().any(|name| candidate.join(name).is_file()) {
            return Some(candidate.to_path_buf());
        }
    }
    None
}

pub fn load_toml_config<T>(path: &Path) -> Option<T>
where
    T: DeserializeOwned,
{
    if !path.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str::<T>(&text).ok()
}

pub fn load_toml_config_with_path<T>(path: &Path) -> Option<(T, PathBuf)>
where
    T: DeserializeOwned,
{
    let config = load_toml_config(path)?;
    Some((config, path.to_path_buf()))
}

pub fn load_first_toml_config<T>(root: &Path, candidates: &[&str]) -> Option<T>
where
    T: DeserializeOwned,
{
    load_first_toml_config_with_path(root, candidates).map(|(config, _)| config)
}

pub fn load_first_toml_config_with_path<T>(root: &Path, candidates: &[&str]) -> Option<(T, PathBuf)>
where
    T: DeserializeOwned,
{
    for candidate in candidates {
        let path = root.join(candidate);
        if let Some(config) = load_toml_config(&path) {
            return Some((config, path));
        }
    }
    None
}

pub fn write_toml_config<T>(path: &Path, value: &T) -> Result<PathBuf, ConfigError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = toml::to_string_pretty(value)?;
    std::fs::write(path, body)?;
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::sync::{Mutex, OnceLock};

    use super::*;
    use tempfile::TempDir;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct DemoConfig {
        enabled: bool,
    }

    struct EnvGuard {
        home: Option<OsString>,
        userprofile: Option<OsString>,
        scoped: Option<OsString>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            Self {
                home: std::env::var_os("HOME"),
                userprofile: std::env::var_os("USERPROFILE"),
                scoped: std::env::var_os("GUILDMESH_HOME"),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.home.as_ref() {
                unsafe { std::env::set_var("HOME", value) };
            } else {
                unsafe { std::env::remove_var("HOME") };
            }
            if let Some(value) = self.userprofile.as_ref() {
                unsafe { std::env::set_var("USERPROFILE", value) };
            } else {
                unsafe { std::env::remove_var("USERPROFILE") };
            }
            if let Some(value) = self.scoped.as_ref() {
                unsafe { std::env::set_var("GUILDMESH_HOME", value) };
            } else {
                unsafe { std::env::remove_var("GUILDMESH_HOME") };
            }
        }
    }

    fn with_env_lock<T>(f: impl FnOnce() -> T) -> T {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock");
        let result = f();
        drop(guard);
        result
    }

    #[test]
    fn env_var_name_normalizes_prefix_and_key() {
        assert_eq!(
            env_var_name("guildmesh", "http.bind-address"),
            "GUILDMESH_HTTP_BIND_ADDRESS"
        );
    }

    #[test]
    fn resolve_user_home_dir_prefers_home_then_userprofile() {
        with_env_lock(|| {
            let _env = EnvGuard::capture();
            unsafe { std::env::set_var("HOME", "/tmp/home-a") };
            unsafe { std::env::set_var("USERPROFILE", "/tmp/profile-a") };
            assert_eq!(resolve_user_home_dir(), Some(PathBuf::from("/tmp/home-a")));

            unsafe { std::env::remove_var("HOME") };
            assert_eq!(
                resolve_user_home_dir(),
                Some(PathBuf::from("/tmp/profile-a"))
            );
        });
    }

    #[test]
    fn resolve_scoped_home_dir_prefers_explicit_env_then_home_fallback() {
        with_env_lock(|| {
            let _env = EnvGuard::capture();
            unsafe { std::env::set_var("HOME", "/tmp/home-b") };
            unsafe { std::env::set_var("GUILDMESH_HOME", "/tmp/guildmesh-home") };
            assert_eq!(
                resolve_scoped_home_dir("GUILDMESH_HOME", ".guildmesh"),
                Some(PathBuf::from("/tmp/guildmesh-home"))
            );

            unsafe { std::env::remove_var("GUILDMESH_HOME") };
            assert_eq!(
                resolve_scoped_home_dir("GUILDMESH_HOME", ".guildmesh"),
                Some(PathBuf::from("/tmp/home-b/.guildmesh"))
            );
        });
    }

    #[test]
    fn find_config_root_discovers_ancestor_candidate() {
        let temp = TempDir::new().expect("tempdir");
        let nested = temp.path().join("repo").join("deep").join("child");
        fs::create_dir_all(&nested).expect("mkdir");
        fs::write(
            temp.path().join("repo").join(".guildmesh.toml"),
            "enabled = true\n",
        )
        .expect("write config");

        assert_eq!(
            find_config_root(&nested, &[".guildmesh.toml"]),
            Some(temp.path().join("repo"))
        );
    }

    #[test]
    fn load_and_write_toml_config_round_trip() {
        let temp = TempDir::new().expect("tempdir");
        let path = temp.path().join(".guildmesh.toml");
        let config = DemoConfig { enabled: true };

        let written = write_toml_config(&path, &config).expect("write toml");
        assert_eq!(written, path);
        assert_eq!(load_toml_config::<DemoConfig>(&path), Some(config.clone()));
        assert_eq!(
            load_toml_config_with_path::<DemoConfig>(&path),
            Some((config, path))
        );
    }

    #[test]
    fn load_first_toml_config_uses_candidate_order() {
        let temp = TempDir::new().expect("tempdir");
        fs::write(temp.path().join(".guildmesh.toml"), "enabled = false\n").expect("write first");
        fs::write(temp.path().join(".guildmeshrc"), "enabled = true\n").expect("write second");

        assert_eq!(
            load_first_toml_config::<DemoConfig>(temp.path(), &[".guildmesh.toml", ".guildmeshrc"]),
            Some(DemoConfig { enabled: false })
        );
        assert_eq!(
            load_first_toml_config_with_path::<DemoConfig>(
                temp.path(),
                &[".guildmesh.toml", ".guildmeshrc"]
            ),
            Some((
                DemoConfig { enabled: false },
                temp.path().join(".guildmesh.toml")
            ))
        );
    }
}
