use glib::KeyFile;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            token: None,
        }
    }
}

fn config_path() -> PathBuf {
    let mut dir = glib::user_config_dir();
    dir.push("wander");
    let _ = std::fs::create_dir_all(&dir);
    dir.push("settings.conf");
    dir
}

pub fn load() -> Option<ServerConfig> {
    load_from(&config_path())
}

pub fn save(config: &ServerConfig) {
    save_to(&config_path(), config);
}

fn load_from(path: &Path) -> Option<ServerConfig> {
    let keyfile = KeyFile::new();
    keyfile
        .load_from_file(path.to_string_lossy().as_ref(), glib::KeyFileFlags::NONE)
        .ok()?;
    let host = keyfile.value("server", "host").ok()?.to_string();
    let port = keyfile.value("server", "port").ok()?.parse().ok()?;
    let token = keyfile
        .value("server", "token")
        .ok()
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty());
    Some(ServerConfig { host, port, token })
}

fn save_to(path: &Path, config: &ServerConfig) {
    let keyfile = KeyFile::new();
    keyfile.set_string("server", "host", &config.host);
    keyfile.set_string("server", "port", &config.port.to_string());
    keyfile.set_string("server", "token", config.token.as_deref().unwrap_or(""));
    if keyfile
        .save_to_file(path.to_string_lossy().as_ref())
        .is_err()
    {
        return;
    }
    restrict(path);
}

/// The bearer token is stored in plain text, so keep the file off every other
/// account on the machine. `save_to_file` honours the process umask, which on a
/// typical desktop leaves it world-readable.
#[cfg(unix)]
fn restrict(path: &Path) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// `save_to`/`load_from` take an explicit path so this never touches the
    /// real `~/.config/wander/settings.conf`.
    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "wander-settings-test-{}-{name}",
            std::process::id()
        ));
        p
    }

    #[test]
    fn round_trips_a_config() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        let config = ServerConfig {
            host: "cairn.example.org".into(),
            port: 9001,
            token: Some("s3cret".into()),
        };
        save_to(&path, &config);

        let loaded = load_from(&path).expect("loads back");
        assert_eq!(loaded.host, "cairn.example.org");
        assert_eq!(loaded.port, 9001);
        assert_eq!(loaded.token.as_deref(), Some("s3cret"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_token_loads_as_none() {
        let path = temp_path("notoken");
        let _ = std::fs::remove_file(&path);
        save_to(
            &path,
            &ServerConfig {
                host: "127.0.0.1".into(),
                port: 8080,
                token: None,
            },
        );
        assert!(load_from(&path).expect("loads back").token.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_saved_file_is_not_readable_by_others() {
        let path = temp_path("perms");
        let _ = std::fs::remove_file(&path);
        // Pre-create it world-readable so the assertion cannot pass by accident
        // of a restrictive umask.
        std::fs::write(&path, b"").expect("seed");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("seed mode");

        save_to(
            &path,
            &ServerConfig {
                host: "127.0.0.1".into(),
                port: 8080,
                token: Some("s3cret".into()),
            },
        );

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "token file must not be group/world readable");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_is_not_a_config() {
        assert!(load_from(Path::new("/nonexistent/wander/settings.conf")).is_none());
    }
}
