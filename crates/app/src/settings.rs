use glib::KeyFile;
use std::path::PathBuf;

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
    let keyfile = KeyFile::new();
    keyfile
        .load_from_file(
            config_path().to_string_lossy().as_ref(),
            glib::KeyFileFlags::NONE,
        )
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

pub fn save(config: &ServerConfig) {
    let keyfile = KeyFile::new();
    keyfile.set_string("server", "host", &config.host);
    keyfile.set_string("server", "port", &config.port.to_string());
    keyfile.set_string("server", "token", config.token.as_deref().unwrap_or(""));
    let _ = keyfile.save_to_file(config_path().to_string_lossy().as_ref());
}
