use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Backoff { pub every_sec: u64, pub max_attempts: u32 }
impl Default for Backoff { fn default() -> Self { Self { every_sec: 300, max_attempts: 6 } } }

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ProjectCfg { pub mode: Option<String>, pub message: Option<String> }

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub mode: String,
    pub opt_out_window_sec: u64,
    pub default_message: String,
    pub force_headless: bool,
    pub backoff: Backoff,
    pub per_project: HashMap<String, ProjectCfg>,
    pub port: u16,
    #[serde(default)]
    pub token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: "auto".into(),
            opt_out_window_sec: 300,
            default_message: "Quota đã reset. Đọc lại việc đang dở và tiếp tục từ chỗ dừng, không hỏi lại.".into(),
            force_headless: false,
            backoff: Backoff::default(),
            per_project: HashMap::new(),
            port: 7317,
            token: String::new(),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Config {
        std::fs::read_to_string(path).ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())?;
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt;
          let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)); }
        Ok(())
    }
    pub fn mode_for(&self, cwd: Option<&str>) -> String {
        cwd.and_then(|c| self.per_project.get(c)).and_then(|p| p.mode.clone())
            .unwrap_or_else(|| self.mode.clone())
    }
    pub fn message_for(&self, cwd: Option<&str>) -> String {
        cwd.and_then(|c| self.per_project.get(c)).and_then(|p| p.message.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| self.default_message.clone())
    }
    /// Generate a token if empty; returns whether it changed.
    pub fn ensure_token(&mut self) -> bool {
        if self.token.is_empty() {
            use rand::Rng;
            const CS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
            let mut r = rand::thread_rng();
            self.token = (0..33).map(|_| CS[r.gen_range(0..CS.len())] as char).collect();
            true
        } else { false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_when_missing() {
        let c = Config::load(Path::new("/nonexistent/cfg.json"));
        assert_eq!(c.mode, "auto");
        assert_eq!(c.backoff.max_attempts, 6);
        assert_eq!(c.port, 7317);
    }
    #[test]
    fn roundtrip_camelcase() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        let c = Config { mode: "off".into(), ..Config::default() };
        c.save(&p).unwrap();
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.contains("\"defaultMessage\""));
        assert!(raw.contains("\"everySec\""));
        assert!(raw.contains("\"maxAttempts\""));
        assert_eq!(Config::load(&p).mode, "off");
    }
    #[test]
    fn loads_python_camelcase_backoff() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"mode":"auto","optOutWindowSec":300,"defaultMessage":"x","forceHeadless":false,"backoff":{"everySec":60,"maxAttempts":2},"perProject":{},"port":7317}"#).unwrap();
        let c = Config::load(&p);
        assert_eq!(c.backoff.every_sec, 60);
        assert_eq!(c.backoff.max_attempts, 2);
    }
    #[test]
    fn per_project_override() {
        let mut c = Config::default();
        c.per_project.insert("/p".into(), ProjectCfg { mode: Some("off".into()), message: None });
        assert_eq!(c.mode_for(Some("/p")), "off");
        assert_eq!(c.mode_for(Some("/other")), "auto");
        assert_eq!(c.mode_for(None), "auto");
    }
    #[test]
    fn message_fallback() {
        let mut c = Config { default_message: "D".into(), ..Config::default() };
        c.per_project.insert("/p".into(), ProjectCfg { mode: None, message: Some("P".into()) });
        assert_eq!(c.message_for(Some("/p")), "P");
        assert_eq!(c.message_for(Some("/q")), "D");
    }
    #[test]
    fn ensure_token_generates_once() {
        let mut c = Config::default();
        assert!(c.ensure_token());
        assert_eq!(c.token.len(), 33);
        let t = c.token.clone();
        assert!(!c.ensure_token());
        assert_eq!(c.token, t);
    }
}
