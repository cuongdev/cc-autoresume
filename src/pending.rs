use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub session_id: String,
    pub cwd: Option<String>,
    pub transcript_path: String,
    pub reset_str: String,
    pub fire_at: i64,
    pub message: String,
    pub armed_at: i64,
    pub cancelled: bool,
    #[serde(default = "yes")]
    pub confirmed: bool,
    pub attempts: u32,
}
fn yes() -> bool { true }

fn path_of(dir: &Path, id: &str) -> PathBuf { dir.join(format!("{id}.json")) }

pub fn write(dir: &Path, rec: &Pending) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(path_of(dir, &rec.session_id), serde_json::to_string_pretty(rec).unwrap())
}
pub fn read(dir: &Path, id: &str) -> Option<Pending> {
    serde_json::from_str(&std::fs::read_to_string(path_of(dir, id)).ok()?).ok()
}
pub fn exists_for(dir: &Path, id: &str, fire_at: i64) -> bool {
    matches!(read(dir, id), Some(r) if r.fire_at == fire_at && !r.cancelled)
}
pub fn cancel(dir: &Path, id: &str) -> bool {
    match read(dir, id) {
        Some(mut r) => { r.cancelled = true; let _ = write(dir, &r); true }
        None => false,
    }
}
pub fn list_all(dir: &Path) -> Vec<Pending> {
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().extension().and_then(|x| x.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(e.path()) {
                    if let Ok(r) = serde_json::from_str(&s) { out.push(r); }
                }
            }
        }
    }
    out
}
pub fn remove(dir: &Path, id: &str) { let _ = std::fs::remove_file(path_of(dir, id)); }
pub fn due(dir: &Path, now: i64) -> Vec<Pending> {
    list_all(dir).into_iter()
        .filter(|r| !r.cancelled && r.confirmed && r.fire_at <= now)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(id: &str, fire: i64, cancelled: bool, confirmed: bool) -> Pending {
        Pending { session_id: id.into(), cwd: Some("/x".into()), transcript_path: "/t".into(),
            reset_str: "4pm".into(), fire_at: fire, message: "go".into(), armed_at: 0,
            cancelled, confirmed, attempts: 0 }
    }
    #[test]
    fn write_read() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("s1", 100, false, true)).unwrap();
        assert_eq!(read(d.path(), "s1").unwrap().fire_at, 100);
    }
    #[test]
    fn exists_for_checks() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("s1", 100, false, true)).unwrap();
        assert!(exists_for(d.path(), "s1", 100));
        assert!(!exists_for(d.path(), "s1", 999));
    }
    #[test]
    fn cancel_sets_flag() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("s1", 100, false, true)).unwrap();
        assert!(cancel(d.path(), "s1"));
        assert!(read(d.path(), "s1").unwrap().cancelled);
        assert!(!cancel(d.path(), "nope"));
    }
    #[test]
    fn due_filters() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("past", 50, false, true)).unwrap();
        write(d.path(), &rec("future", 500, false, true)).unwrap();
        write(d.path(), &rec("cancelled", 50, true, true)).unwrap();
        write(d.path(), &rec("unconfirmed", 50, false, false)).unwrap();
        let ids: Vec<_> = due(d.path(), 100).into_iter().map(|r| r.session_id).collect();
        assert_eq!(ids, vec!["past".to_string()]);
    }
    #[test]
    fn confirmed_defaults_true_on_legacy() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("x.json"),
            r#"{"sessionId":"x","cwd":null,"transcriptPath":"/t","resetStr":"4pm","fireAt":1,"message":"m","armedAt":0,"cancelled":false,"attempts":0}"#).unwrap();
        assert!(read(d.path(), "x").unwrap().confirmed);
    }
}
