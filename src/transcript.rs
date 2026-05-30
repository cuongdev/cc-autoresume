use serde_json::Value;

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Msg {
    pub role: String,
    pub kind: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// Parse one transcript JSONL line into zero or more renderable messages.
/// Skips synthetic limit lines, tool_result and thinking blocks.
pub fn parse_line(line: &str) -> Vec<Msg> {
    let Ok(obj) = serde_json::from_str::<Value>(line) else { return vec![] };
    let role = match obj.get("type").and_then(|t| t.as_str()) {
        Some("user") => "user",
        Some("assistant") => "assistant",
        _ => return vec![],
    };
    let msg = match obj.get("message") { Some(m) => m, None => return vec![] };
    if msg.get("model").and_then(|m| m.as_str()) == Some("<synthetic>") { return vec![]; }
    let content = msg.get("content").unwrap_or(&Value::Null);
    let mut out = vec![];
    match content {
        Value::String(s) => {
            let s = s.trim();
            if !s.is_empty() { out.push(Msg { role: role.into(), kind: "text".into(), text: s.into(), tool: None }); }
        }
        Value::Array(blocks) => {
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        let t = b.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                        if !t.is_empty() { out.push(Msg { role: role.into(), kind: "text".into(), text: t.into(), tool: None }); }
                    }
                    Some("tool_use") => {
                        let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("tool").to_string();
                        let summary = b.get("input").map(summarize_input).unwrap_or_default();
                        out.push(Msg { role: role.into(), kind: "tool".into(), text: summary, tool: Some(name) });
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

/// One-line summary of a tool_use input (file_path / command / pattern / first string value).
fn summarize_input(input: &Value) -> String {
    for key in ["file_path", "command", "pattern", "path", "url", "description"] {
        if let Some(s) = input.get(key).and_then(|x| x.as_str()) {
            return truncate(s, 120);
        }
    }
    truncate(&input.to_string(), 120)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() }
    else { s.chars().take(n).collect::<String>() + "…" }
}

/// Read and parse all messages from a transcript file.
pub fn read_messages(path: &std::path::Path) -> Vec<Msg> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };
    content.lines().flat_map(parse_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_string_content() {
        let m = parse_line(r#"{"type":"user","message":{"role":"user","content":"hi there"}}"#);
        assert_eq!(m, vec![Msg { role: "user".into(), kind: "text".into(), text: "hi there".into(), tool: None }]);
    }
    #[test]
    fn assistant_text_and_tool_blocks() {
        let line = r#"{"type":"assistant","message":{"model":"claude","content":[{"type":"text","text":"reading"},{"type":"tool_use","name":"Read","input":{"file_path":"/a/b.rs"}}]}}"#;
        let m = parse_line(line);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0], Msg { role:"assistant".into(), kind:"text".into(), text:"reading".into(), tool:None });
        assert_eq!(m[1], Msg { role:"assistant".into(), kind:"tool".into(), text:"/a/b.rs".into(), tool:Some("Read".into()) });
    }
    #[test]
    fn skips_synthetic_thinking_toolresult() {
        assert!(parse_line(r#"{"type":"assistant","message":{"model":"<synthetic>","content":"You've hit your limit"}}"#).is_empty());
        let line = r#"{"type":"assistant","message":{"model":"c","content":[{"type":"thinking","thinking":"hmm"},{"type":"tool_result","content":"x"}]}}"#;
        assert!(parse_line(line).is_empty());
    }
    #[test]
    fn skips_non_message_lines() {
        assert!(parse_line("not json").is_empty());
        assert!(parse_line(r#"{"type":"summary"}"#).is_empty());
    }
    #[test]
    fn read_messages_from_file() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(f, r#"{{"type":"user","message":{{"content":"q"}}}}"#).unwrap();
        writeln!(f, r#"{{"type":"assistant","message":{{"model":"c","content":[{{"type":"text","text":"a"}}]}}}}"#).unwrap();
        let m = read_messages(f.path());
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].text, "a");
    }
}
