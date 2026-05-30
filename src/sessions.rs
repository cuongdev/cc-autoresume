use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    #[serde(skip_serializing_if = "Option::is_none")] pub title: Option<String>,
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
                if p.is_dir() {
                    if p.file_name().and_then(|n| n.to_str()) != Some("subagents") { walk(&p, out); }
                }
                else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") { out.push(p); }
            }
        }
    }
    walk(dir, &mut out);
    out
}

/// Discover sessions whose transcript mtime is within `window_secs` of `now`,
/// newest first, capped at `cap`. `live` = written within `live_window_secs`.
/// Cheap: only stats files, reads ≤10 lines per kept session for cwd, no lsof.
pub fn discover(projects_dir: &Path, presets_path: &Path, pending_dir: &Path,
                now: i64, window_secs: i64, cap: usize, live_window_secs: i64) -> Vec<SessionInfo> {
    let presets = presets_load(presets_path);
    let mut entries: Vec<(PathBuf, i64)> = vec![];
    for path in jsonl_files(projects_dir) {
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        let mtime = meta.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64).unwrap_or(0);
        if (now - mtime).abs() > window_secs { continue; }
        entries.push((path, mtime));
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.1));
    entries.truncate(cap);
    let mut out = vec![];
    for (path, mtime) in entries {
        let sid = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if sid.is_empty() { continue; }
        let cwd = cwd_fast(&path);
        let title = title_fast(&path);
        let live = (now - mtime).abs() < live_window_secs;
        let pending = crate::pending::read(pending_dir, &sid).is_some();
        let preset = presets.get(&sid).cloned();
        out.push(SessionInfo {
            session_id: sid, cwd, last_activity_epoch: mtime, live, pending,
            has_preset: preset.is_some(),
            preset_message: preset.as_ref().and_then(|p| p.message.clone()),
            preset_mode: preset.as_ref().and_then(|p| p.mode.clone()),
            title,
        });
    }
    out
}

/// Read cwd from the first ~10 JSONL lines (cheap; avoids reading huge transcripts).
fn cwd_fast(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    for line in BufReader::new(f).lines().take(10).map_while(Result::ok) {
        if let Ok(o) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(c) = o.get("cwd").and_then(|c| c.as_str()) { return Some(c.to_string()); }
        }
    }
    None
}

/// True if `text` is a synthetic / system / tooling line rather than something
/// the human actually typed (caveats, slash-command wrappers, limit notices…).
fn is_noise_title(text: &str) -> bool {
    let t = text.trim_start_matches(['⎿', '└', ' ', '\u{00b7}']).trim_start();
    let lower = t.to_lowercase();
    lower.contains("hit your limit")
        || lower.contains("hit your session limit")
        || t.starts_with("<local-command-caveat")
        || t.starts_with("<command-name")
        || t.starts_with("<command-message")
        || t.starts_with("<command-args")
        || t.starts_with("<bash-")
        || t.starts_with("<system-reminder")
        || t.starts_with("Caveat:")
        || t.starts_with("[Request interrupted")
}

/// First genuine user-prompt snippet (≤60 chars, whitespace-collapsed) for a friendly label.
fn title_fast(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    for line in BufReader::new(f).lines().take(40).map_while(Result::ok) {
        let Ok(o) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        if o.get("type").and_then(|t| t.as_str()) != Some("user") { continue; }
        // skip meta/system-injected turns (caveats, command output, etc.)
        if o.get("isMeta").and_then(|m| m.as_bool()) == Some(true) { continue; }
        let content = o.get("message").and_then(|m| m.get("content"));
        let text = match content {
            Some(serde_json::Value::String(s)) => s.clone(),
            // only real text blocks — ignore tool_result / image blocks
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .find_map(|b| b.get("text").and_then(|t| t.as_str()))
                .unwrap_or("")
                .to_string(),
            _ => continue,
        };
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let collapsed = collapsed.trim();
        if collapsed.is_empty() || is_noise_title(collapsed) { continue; }
        let snippet: String = collapsed.chars().take(60).collect();
        return Some(if collapsed.chars().count() > 60 { format!("{snippet}…") } else { snippet });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mtime_of(p: &std::path::Path) -> i64 {
        std::fs::metadata(p).unwrap().modified().unwrap()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
    }

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
    fn discover_recent_live_and_flags() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        let pres = proj.path().join("sessions.json");
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        let fp = proj.path().join("p/abc123.jsonl");
        std::fs::write(&fp, "{\"type\":\"user\",\"cwd\":\"/work\",\"message\":{\"content\":\"hi\"}}\n").unwrap();
        upsert_preset(&pres, "abc123", Some("go".into()), None);
        let m = mtime_of(&fp);
        let got = discover(proj.path(), &pres, pend.path(), m + 5, i64::MAX, 60, 120);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].session_id, "abc123");
        assert_eq!(got[0].cwd.as_deref(), Some("/work"));
        assert!(got[0].live);                         // age 5s < 120
        assert!(got[0].has_preset);
        assert_eq!(got[0].preset_message.as_deref(), Some("go"));
        assert!(!got[0].pending);
        assert_eq!(got[0].title.as_deref(), Some("hi"));
        let got2 = discover(proj.path(), &pres, pend.path(), m + 300, i64::MAX, 60, 120);
        assert!(!got2[0].live);                        // age 300s > 120
    }
    #[test]
    fn title_skips_synthetic_caveat_and_tool_results() {
        let d = tempfile::tempdir().unwrap();
        let fp = d.path().join("s.jsonl");
        let lines = [
            r#"{"type":"user","isMeta":true,"message":{"content":"<local-command-caveat>Caveat: The messages below were generated…</local-command-caveat>"}}"#,
            r#"{"type":"user","message":{"content":"⎿ You've hit your session limit · resets 2:30am (Asia/Saigon)"}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","text":"some tool output"}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#,
            r#"{"type":"user","message":{"content":"Refactor the auth module please"}}"#,
        ];
        std::fs::write(&fp, lines.join("\n") + "\n").unwrap();
        assert_eq!(title_fast(&fp).as_deref(), Some("Refactor the auth module please"));
    }

    #[test]
    fn is_noise_title_matches_synthetic_lines() {
        assert!(is_noise_title("⎿ You've hit your session limit · resets 2:30am"));
        assert!(is_noise_title("<local-command-caveat>x"));
        assert!(is_noise_title("Caveat: blah"));
        assert!(!is_noise_title("Build the billing API"));
    }

    #[test]
    fn discover_excludes_outside_window() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        let pres = proj.path().join("sessions.json");
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        std::fs::write(proj.path().join("p/old.jsonl"), "{}\n").unwrap();
        let got = discover(proj.path(), &pres, pend.path(), 1_000_000_000, -1, 60, 120);
        assert!(got.is_empty());
    }
    #[test]
    fn discover_skips_subagent_transcripts() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        let pres = proj.path().join("sessions.json");
        std::fs::create_dir_all(proj.path().join("sess/subagents")).unwrap();
        std::fs::write(proj.path().join("sess/main.jsonl"), "{\"type\":\"user\",\"message\":{\"content\":\"x\"}}\n").unwrap();
        std::fs::write(proj.path().join("sess/subagents/agent-abc.jsonl"), "{\"type\":\"user\",\"message\":{\"content\":\"y\"}}\n").unwrap();
        let ids: Vec<_> = discover(proj.path(), &pres, pend.path(), mtime_of(&proj.path().join("sess/main.jsonl")) + 1, i64::MAX, 60, 120)
            .into_iter().map(|s| s.session_id).collect();
        assert_eq!(ids, vec!["main".to_string()]);   // subagent excluded
    }
    #[test]
    fn discover_caps_results() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        let pres = proj.path().join("sessions.json");
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        for i in 0..5 { std::fs::write(proj.path().join(format!("p/s{i}.jsonl")), "{}\n").unwrap(); }
        let got = discover(proj.path(), &pres, pend.path(), mtime_of(&proj.path().join("p/s0.jsonl")) + 1, i64::MAX, 2, 120);
        assert_eq!(got.len(), 2);
    }
}
