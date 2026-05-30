use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentEntry {
    pub session_id: String,
    pub cwd: Option<String>,
    pub transcript_path: String,
    pub outcome: String,
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    #[serde(default)] pub limit_hit_times: Vec<i64>,
    #[serde(default)] pub auto_resumes: u64,
    #[serde(default)] pub recent: Vec<RecentEntry>,
}

const RECENT_CAP: usize = 20;

impl Stats {
    pub fn load(path: &Path) -> Stats {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) {
        if let Some(p) = path.parent() { let _ = std::fs::create_dir_all(p); }
        let _ = std::fs::write(path, serde_json::to_string_pretty(self).unwrap());
    }
    /// Count of limit hits within the last 7 days relative to `now`.
    pub fn hits_7d(&self, now: i64) -> usize {
        let cutoff = now - 7 * 86400;
        self.limit_hit_times.iter().filter(|&&t| t >= cutoff).count()
    }
    pub fn record_limit_hit(path: &Path, now: i64) {
        let mut s = Stats::load(path);
        s.limit_hit_times.push(now);
        let cutoff = now - 7 * 86400;
        s.limit_hit_times.retain(|&t| t >= cutoff);
        s.save(path);
    }
    pub fn record_resume(path: &Path, entry: RecentEntry) {
        let mut s = Stats::load(path);
        if entry.outcome == "ok" { s.auto_resumes += 1; }
        s.recent.insert(0, entry);
        s.recent.truncate(RECENT_CAP);
        s.save(path);
    }
}

/// Active Claude sessions in the last 7 days, from `<home>/.claude/stats-cache.json`.
pub fn sessions_7d(home: &Path, now: i64) -> u64 {
    let raw = match std::fs::read_to_string(home.join(".claude/stats-cache.json")) {
        Ok(r) => r, Err(_) => return 0,
    };
    let v: Value = match serde_json::from_str(&raw) { Ok(v) => v, Err(_) => return 0 };
    let cutoff = now - 7 * 86400;
    let mut total = 0u64;
    if let Some(days) = v.get("dailyActivity").and_then(|d| d.as_array()) {
        for day in days {
            let date = day.get("date").and_then(|d| d.as_str()).unwrap_or("");
            if let Ok(d) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
                let epoch = d.and_hms_opt(0,0,0).unwrap().and_utc().timestamp();
                if epoch >= cutoff {
                    total += day.get("sessionCount").and_then(|c| c.as_u64()).unwrap_or(0);
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hits_7d_windows() {
        let s = Stats { limit_hit_times: vec![100, 200, 1_000_000], ..Default::default() };
        assert_eq!(s.hits_7d(1_000_000), 1);
    }
    #[test]
    fn record_limit_hit_prunes_old() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("stats.json");
        Stats::record_limit_hit(&p, 1000);
        Stats::record_limit_hit(&p, 1_000_000);
        let s = Stats::load(&p);
        assert_eq!(s.limit_hit_times, vec![1_000_000]);
    }
    #[test]
    fn record_resume_counts_ok_and_caps_recent() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("stats.json");
        for i in 0..25 {
            Stats::record_resume(&p, RecentEntry { session_id: format!("s{i}"), cwd: None,
                transcript_path: "/t".into(), outcome: "ok".into(), at: i });
        }
        let s = Stats::load(&p);
        assert_eq!(s.auto_resumes, 25);
        assert_eq!(s.recent.len(), 20);
        assert_eq!(s.recent[0].session_id, "s24");
    }
    #[test]
    fn sessions_7d_reads_cache() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".claude")).unwrap();
        std::fs::write(d.path().join(".claude/stats-cache.json"),
            r#"{"dailyActivity":[{"date":"2000-01-01","sessionCount":99},{"date":"2026-05-29","sessionCount":7}]}"#).unwrap();
        let now = chrono::NaiveDate::parse_from_str("2026-05-30","%Y-%m-%d").unwrap().and_hms_opt(0,0,0).unwrap().and_utc().timestamp();
        assert_eq!(sessions_7d(d.path(), now), 7);
    }
    #[test]
    fn sessions_7d_zero_when_missing() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(sessions_7d(d.path(), 1_000_000_000), 0);
    }
}
