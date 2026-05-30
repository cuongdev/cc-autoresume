use crate::{arm, config::Config, detect, pending, scheduler, Runner};
use chrono::Utc;
use chrono_tz::Tz;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Watcher {
    pub projects_dir: PathBuf,
    pub offsets: HashMap<PathBuf, u64>,
}

impl Watcher {
    pub fn new(projects_dir: PathBuf) -> Self { Self { projects_dir, offsets: HashMap::new() } }

    fn jsonl_files(&self) -> Vec<PathBuf> {
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
        walk(&self.projects_dir, &mut out);
        out
    }

    /// Scan for new limit lines; arm each. Returns the records armed this scan.
    pub fn scan_once(&mut self, pending_dir: &Path, cfg: &Config, now: chrono::DateTime<Utc>,
                     default_tz: Tz, runner: &dyn Runner) -> Vec<pending::Pending> {
        use std::io::{Read, Seek, SeekFrom};
        let mut armed = vec![];
        for path in self.jsonl_files() {
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            let size = meta.len();
            let off = self.offsets.entry(path.clone()).or_insert(size);
            if size == *off { continue; }
            if size < *off { *off = 0; continue; }
            let Ok(mut f) = std::fs::File::open(&path) else { continue };
            if f.seek(SeekFrom::Start(*off)).is_err() { continue; }
            let mut chunk = String::new();
            if f.read_to_string(&mut chunk).is_err() { continue; }
            *off = size;
            let sid = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let cwd = detect::resolve_cwd(&path);
            for line in chunk.lines() {
                if let Some(ev) = detect::detect_line(line, &sid, cwd.as_deref(), path.to_str().unwrap_or("")) {
                    let mut sched = |fa: i64| scheduler::pmset_wake(fa, runner);
                    if let Some(rec) = arm::arm(pending_dir, cfg, &ev, now, default_tz, runner, &mut sched) {
                        armed.push(rec);
                    }
                }
            }
        }
        armed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmdOut;
    use chrono::TimeZone;
    use chrono_tz::Asia::Saigon as SG;
    struct Null;
    impl Runner for Null { fn run(&self, _a: &[String], _c: Option<&str>) -> CmdOut { CmdOut::default() } }
    const LIMIT: &str = "{\"type\":\"assistant\",\"cwd\":\"/Users/dev/projects/demo\",\"message\":{\"model\":\"<synthetic>\",\"stop_reason\":\"stop_sequence\",\"content\":\"You've hit your limit \u{00b7} resets 4pm (Asia/Saigon)\"}}\n";
    fn now() -> chrono::DateTime<Utc> { SG.with_ymd_and_hms(2026,5,30,1,0,0).single().unwrap().with_timezone(&Utc) }

    #[test]
    fn baseline_ignores_preexisting() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        std::fs::write(proj.path().join("p/sess.jsonl"), LIMIT).unwrap();
        let mut w = Watcher::new(proj.path().to_path_buf());
        assert!(w.scan_once(pend.path(), &Config::default(), now(), SG, &Null).is_empty());
        assert!(pending::read(pend.path(), "sess").is_none());
    }
    #[test]
    fn detects_appended() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        let f = proj.path().join("p/sess.jsonl");
        std::fs::write(&f, "{\"type\":\"user\",\"cwd\":\"/Users/dev/projects/demo\",\"message\":{}}\n").unwrap();
        let mut w = Watcher::new(proj.path().to_path_buf());
        w.scan_once(pend.path(), &Config::default(), now(), SG, &Null);
        use std::io::Write;
        let mut fh = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        fh.write_all(LIMIT.as_bytes()).unwrap();
        let armed = w.scan_once(pend.path(), &Config::default(), now(), SG, &Null);
        assert_eq!(armed.len(), 1);
        assert_eq!(pending::read(pend.path(), "sess").unwrap().cwd.as_deref(), Some("/Users/dev/projects/demo"));
    }
    #[test]
    fn truncation_defers() {
        let proj = tempfile::tempdir().unwrap();
        let pend = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join("p")).unwrap();
        let f = proj.path().join("p/sess.jsonl");
        std::fs::write(&f, "x".repeat(100) + "\n").unwrap();
        let mut w = Watcher::new(proj.path().to_path_buf());
        w.scan_once(pend.path(), &Config::default(), now(), SG, &Null);
        std::fs::write(&f, "short\n").unwrap();
        let armed = w.scan_once(pend.path(), &Config::default(), now(), SG, &Null);
        assert!(armed.is_empty());
        assert_eq!(*w.offsets.get(&f).unwrap(), 0);
    }
}
