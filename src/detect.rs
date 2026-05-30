use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq)]
pub struct LimitEvent {
    pub session_id: String,
    pub cwd: Option<String>,
    pub reset_str: String,
    pub transcript_path: String,
    pub ts: String,
}

static LIMIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    // `·` is U+00B7
    Regex::new(r"hit your (?:session )?limit · resets (.+?)\s*$").unwrap()
});

fn extract_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

pub fn detect_line(line: &str, session_id: &str, cwd: Option<&str>, transcript_path: &str) -> Option<LimitEvent> {
    let obj: Value = serde_json::from_str(line).ok()?;
    let msg = obj.get("message")?;
    if msg.get("model").and_then(|m| m.as_str()) != Some("<synthetic>") {
        return None;
    }
    let text = extract_text(msg.get("content").unwrap_or(&Value::Null));
    let caps = LIMIT_RE.captures(&text)?;
    Some(LimitEvent {
        session_id: session_id.to_string(),
        cwd: cwd.map(|c| c.to_string()),
        reset_str: caps.get(1).unwrap().as_str().trim().to_string(),
        transcript_path: transcript_path.to_string(),
        ts: obj.get("timestamp").and_then(|t| t.as_str()).unwrap_or("").to_string(),
    })
}

pub fn resolve_cwd(transcript_path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(transcript_path).ok()?;
    let mut cwd = None;
    for line in content.lines() {
        if let Ok(o) = serde_json::from_str::<Value>(line) {
            if let Some(c) = o.get("cwd").and_then(|c| c.as_str()) {
                cwd = Some(c.to_string());
            }
        }
    }
    cwd
}

#[cfg(test)]
mod tests {
    use super::*;
    const LIMIT: &str = r#"{"type":"assistant","timestamp":"2026-05-30T18:30:00Z","cwd":"/Users/dev/projects/demo","message":{"model":"<synthetic>","stop_reason":"stop_sequence","content":"You've hit your limit · resets 4pm (Asia/Saigon)"}}"#;
    const USERLINE: &str = r#"{"type":"user","cwd":"/x","message":{"role":"user","content":"hi"}}"#;
    const NORMAL: &str = r#"{"type":"assistant","message":{"model":"claude-opus-4-8","content":[{"type":"text","text":"there is no limit to what we can do"}]}}"#;

    #[test]
    fn detects_synthetic() {
        let ev = detect_line(LIMIT, "abc", Some("/x"), "/t.jsonl").unwrap();
        assert_eq!(ev.reset_str, "4pm (Asia/Saigon)");
        assert_eq!(ev.session_id, "abc");
    }
    #[test]
    fn ignores_user() { assert!(detect_line(USERLINE, "a", None, "/t").is_none()); }
    #[test]
    fn ignores_real_model() { assert!(detect_line(NORMAL, "a", None, "/t").is_none()); }
    #[test]
    fn ignores_garbage() { assert!(detect_line("not json", "a", None, "/t").is_none()); }
    #[test]
    fn resolve_cwd_reads_last() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(f, "{}", LIMIT).unwrap();
        assert_eq!(resolve_cwd(f.path()).as_deref(), Some("/Users/dev/projects/demo"));
    }
}
