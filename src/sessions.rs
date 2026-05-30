use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::Runner;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SessionPreset {
    #[serde(skip_serializing_if = "Option::is_none")] pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub mode: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub cwd: Option<String>,
    pub last_activity_epoch: i64,
    pub live: bool,
    pub pending: bool,
    pub has_preset: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub preset_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub preset_mode: Option<String>,
}

pub fn presets_load(path: &Path) -> HashMap<String, SessionPreset> {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}
pub fn presets_save(path: &Path, m: &HashMap<String, SessionPreset>) {
    if let Some(p) = path.parent() { let _ = std::fs::create_dir_all(p); }
    let _ = std::fs::write(path, serde_json::to_string_pretty(m).unwrap());
}
pub fn preset_for(path: &Path, id: &str) -> SessionPreset {
    presets_load(path).get(id).cloned().unwrap_or_default()
}
/// Upsert: Some("")=clear field, Some(x)=set, None=leave. Removes entry if both empty.
pub fn upsert_preset(path: &Path, id: &str, message: Option<String>, mode: Option<String>) {
    let mut m = presets_load(path);
    let e = m.entry(id.to_string()).or_default();
    match message { Some(s) if s.is_empty() => e.message = None, Some(s) => e.message = Some(s), None => {} }
    match mode { Some(s) if s.is_empty() => e.mode = None, Some(s) => e.mode = Some(s), None => {} }
    if e.message.is_none() && e.mode.is_none() { m.remove(id); }
    presets_save(path, &m);
}

fn jsonl_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() { walk(&p, out); }
                else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") { out.push(p); }
            }
        }
    }
    walk(dir, &mut out);
    out
}

/// Discover sessions whose transcript was modified within `window_secs` of `now`,
/// newest first, annotated with live/pending/preset flags.
pub fn discover(projects_dir: &Path, presets_path: &Path, pending_dir: &Path,
                now: i64, window_secs: i64, runner: &dyn Runner,
                which: &dyn Fn(&str) -> Option<String>) -> Vec<SessionInfo> {
    let presets = presets_load(presets_path);
    let mut out = vec![];
    for path in jsonl_files(projects_dir) {
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        let mtime = meta.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64).unwrap_or(0);
        if (now - mtime).abs() > window_secs { continue; }
        let sid = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if sid.is_empty() { continue; }
        let cwd = crate::detect::resolve_cwd(&path);
        let live = crate::resume::is_session_live(path.to_str().unwrap_or(""), runner, which);
        let pending = crate::pending::read(pending_dir, &sid).is_some();
        let preset = presets.get(&sid).cloned();
        out.push(SessionInfo {
            session_id: sid,
            cwd,
            last_activity_epoch: mtime,
            live,
            pending,
            has_preset: preset.is_some(),
            preset_message: preset.as_ref().and_then(|p| p.message.clone()),
            preset_mode: preset.as_ref().and_then(|p| p.mode.clone()),
        });
    }
    out.sort_by(|a, b| b.last_activity_epoch.cmp(&a.last_activity_epoch));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmdOut;
    use std::cell::RefCell;
    struct Fake { live: bool, calls: RefCell<u32> }
    impl Runner for Fake { fn run(&self, _a: &[String], _c: Option<&str>) -> CmdOut {
        *self.calls.borrow_mut() += 1;
        CmdOut { stdout: if self.live { "123\n".into() } else { String::new() }, ..Default::default() } } }
    fn lsof_yes(n: &str) -> Option<String> { if n == "lsof" { Some("lsof".into()) } else { None } }

    #[test]
    fn presets_roundtrip_and_upsert() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sessions.json");
        upsert_preset(&p, "s1", Some("hello".into()), Some("ask".into()));
        let pr = preset_for(&p, "s1");
        assert_eq!(pr.message.as_deref(), Some("hello"));
        assert_eq!(pr.mode.as_deref(), Some("ask"));
        upsert_preset(&p, "s1", Some("".into()), None);
        assert_eq!(preset_for(&p, "s1").message, None);
        assert_eq!(preset_for(&p, "s1").mode.as_deref(), Some("ask"));
        upsert_preset(&p, "s1", None, Some("".into()));
        assert_eq!(preset_for(&p, "s1"), SessionPreset::default());
    }
    #[test]
    fn discover_includes_recent_with_flags() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        let pres = proj.path().join("sessions.json");
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        std::fs::write(proj.path().join("p/abc123.jsonl"),
            "{\"type\":\"user\",\"cwd\":\"/work\",\"message\":{\"content\":\"hi\"}}\n").unwrap();
        upsert_preset(&pres, "abc123", Some("go".into()), None);
        let f = Fake { live: true, calls: RefCell::new(0) };
        let got = discover(proj.path(), &pres, pend.path(), 1_000_000_000, i64::MAX, &f, &lsof_yes);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].session_id, "abc123");
        assert_eq!(got[0].cwd.as_deref(), Some("/work"));
        assert!(got[0].live);
        assert!(got[0].has_preset);
        assert_eq!(got[0].preset_message.as_deref(), Some("go"));
        assert!(!got[0].pending);
    }
    #[test]
    fn discover_excludes_outside_window() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        let pres = proj.path().join("sessions.json");
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        std::fs::write(proj.path().join("p/old.jsonl"), "{}\n").unwrap();
        let f = Fake { live: false, calls: RefCell::new(0) };
        let got = discover(proj.path(), &pres, pend.path(), 1_000_000_000, -1, &f, &lsof_yes);
        assert!(got.is_empty());
    }
}
