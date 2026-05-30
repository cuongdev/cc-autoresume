use crate::{config::Config, notify, pending, pending::Pending, Runner};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

pub fn is_session_live(transcript_path: &str, runner: &dyn Runner,
                       which: &dyn Fn(&str) -> Option<String>) -> bool {
    let Some(lsof) = which("lsof") else { return false };
    let out = runner.run(&[lsof, "-t".into(), transcript_path.into()], None);
    !out.stdout.trim().is_empty()
}

pub fn build_cmd(message: &str, session_id: &str, claude_bin: &str) -> Vec<String> {
    vec![claude_bin.into(), "-p".into(), message.into(), "--resume".into(), session_id.into()]
}

static LIMITED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)hit your (?:session )?limit").unwrap());

fn looks_limited(text: &str) -> bool {
    LIMITED_RE.is_match(text)
}

fn short(id: &str) -> &str { &id[..8.min(id.len())] }

/// Returns one of: "live-skip" | "still-limited" | "ok" | "error".
pub fn run_resume(rec: &Pending, cfg: &Config, runner: &dyn Runner,
                  which: &dyn Fn(&str) -> Option<String>, claude_bin: &str) -> &'static str {
    if !cfg.force_headless && is_session_live(&rec.transcript_path, runner, which) {
        notify::notify("cc-autoresume",
            &format!("Quota back — press continue in your open session {}", short(&rec.session_id)), runner);
        return "live-skip";
    }
    let out = runner.run(&build_cmd(&rec.message, &rec.session_id, claude_bin), rec.cwd.as_deref());
    let combined = format!("{}{}", out.stdout, out.stderr);
    if looks_limited(&combined) { return "still-limited"; }
    if out.code == 0 { "ok" } else { "error" }
}

/// Drives one fire attempt against on-disk pending; returns a status string.
pub fn fire(dir: &Path, id: &str, cfg: &Config, now: i64, runner: &dyn Runner,
            which: &dyn Fn(&str) -> Option<String>, claude_bin: &str) -> &'static str {
    let Some(rec) = pending::read(dir, id) else { return "cancelled" };
    if rec.cancelled { return "cancelled"; }
    let status = run_resume(&rec, cfg, runner, which, claude_bin);
    match status {
        "still-limited" | "error" => {
            let mut r = rec;
            r.attempts += 1;
            if r.attempts >= cfg.backoff.max_attempts {
                pending::remove(dir, id);
                notify::notify("cc-autoresume", &format!("Gave up resuming {} after {} tries", short(id), r.attempts), runner);
                "gave-up"
            } else {
                r.fire_at = now + cfg.backoff.every_sec as i64;
                let _ = pending::write(dir, &r);
                "retry-scheduled"
            }
        }
        "ok" => {
            notify::notify("cc-autoresume", &format!("▶ Resumed {}", short(id)), runner);
            pending::remove(dir, id);
            "ok"
        }
        "live-skip" => { pending::remove(dir, id); "live-skip" }
        other => other,
    }
}

/// Return the first path in `cands` that exists.
pub fn first_existing(cands: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    cands.iter().find(|p| p.exists()).cloned()
}

/// Resolve the `claude` binary to an absolute path (LaunchAgents have a minimal PATH
/// that excludes ~/.local/bin etc.). Falls back to bare "claude".
pub fn resolve_claude_bin() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let cands: Vec<std::path::PathBuf> = [
        format!("{home}/.local/bin/claude"),
        format!("{home}/.claude/local/claude"),
        "/opt/homebrew/bin/claude".into(),
        "/usr/local/bin/claude".into(),
    ].iter().map(std::path::PathBuf::from).collect();
    if let Some(p) = first_existing(&cands) {
        return p.to_string_lossy().into_owned();
    }
    if let Some(p) = crate::scheduler::which_path("claude") { return p; }
    "claude".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmdOut;
    use std::cell::RefCell;

    struct Fake { lsof_out: String, stdout: String, code: i32, calls: RefCell<Vec<Vec<String>>> }
    impl Fake { fn new(lsof: &str, out: &str, code: i32) -> Self {
        Self { lsof_out: lsof.into(), stdout: out.into(), code, calls: RefCell::new(vec![]) } } }
    impl Runner for Fake {
        fn run(&self, a: &[String], _c: Option<&str>) -> CmdOut {
            self.calls.borrow_mut().push(a.to_vec());
            if a[0].ends_with("lsof") { CmdOut { stdout: self.lsof_out.clone(), code: 0, ..Default::default() } }
            else { CmdOut { stdout: self.stdout.clone(), code: self.code, ..Default::default() } }
        }
    }
    fn lsof_yes(n: &str) -> Option<String> { if n == "lsof" { Some("lsof".into()) } else { None } }
    fn cfg(force: bool, max: u32) -> Config {
        Config { force_headless: force, backoff: crate::config::Backoff { every_sec: 60, max_attempts: max }, ..Config::default() }
    }
    fn rec() -> Pending {
        Pending { session_id: "s1abcdef".into(), cwd: Some("/x".into()), transcript_path: "/t.jsonl".into(),
            reset_str: "5pm".into(), fire_at: 1, message: "continue".into(), armed_at: 0,
            cancelled: false, confirmed: true, attempts: 0 }
    }

    #[test]
    fn first_existing_picks_present() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("claude");
        std::fs::write(&real, "x").unwrap();
        let cands = vec![d.path().join("nope"), real.clone()];
        assert_eq!(first_existing(&cands), Some(real));
        assert_eq!(first_existing(&[d.path().join("none")]), None);
    }
    #[test]
    fn build_cmd_shape() {
        assert_eq!(build_cmd("go", "s1", "claude"), vec!["claude","-p","go","--resume","s1"]);
    }
    #[test]
    fn live_skips_headless() {
        let f = Fake::new("123\n", "done", 0);
        assert_eq!(run_resume(&rec(), &cfg(false, 3), &f, &lsof_yes, "claude"), "live-skip");
        assert!(f.calls.borrow().iter().all(|c| !c[0].contains("claude")));
    }
    #[test]
    fn force_headless_ignores_liveness() {
        let f = Fake::new("123\n", "done", 0);
        assert_eq!(run_resume(&rec(), &cfg(true, 3), &f, &lsof_yes, "claude"), "ok");
    }
    #[test]
    fn dead_runs_headless() {
        let f = Fake::new("", "done", 0);
        assert_eq!(run_resume(&rec(), &cfg(false, 3), &f, &lsof_yes, "claude"), "ok");
        assert!(f.calls.borrow().iter().any(|c| c.contains(&"--resume".to_string())));
    }
    #[test]
    fn still_limited_from_output() {
        let f = Fake::new("", "You've hit your limit · resets 5pm", 0);
        assert_eq!(run_resume(&rec(), &cfg(false, 3), &f, &lsof_yes, "claude"), "still-limited");
    }
    #[test]
    fn fire_error_and_limited_back_off_then_giveup() {
        let d = tempfile::tempdir().unwrap();
        pending::write(d.path(), &rec()).unwrap();
        let f = Fake::new("", "boom", 1);
        let c = cfg(false, 2);
        assert_eq!(fire(d.path(), "s1abcdef", &c, 1000, &f, &lsof_yes, "claude"), "retry-scheduled");
        assert_eq!(pending::read(d.path(), "s1abcdef").unwrap().attempts, 1);
        assert_eq!(pending::read(d.path(), "s1abcdef").unwrap().fire_at, 1060);
        assert_eq!(fire(d.path(), "s1abcdef", &c, 2000, &f, &lsof_yes, "claude"), "gave-up");
        assert!(pending::read(d.path(), "s1abcdef").is_none());
    }
    #[test]
    fn fire_success_removes() {
        let d = tempfile::tempdir().unwrap();
        pending::write(d.path(), &rec()).unwrap();
        let f = Fake::new("", "done", 0);
        assert_eq!(fire(d.path(), "s1abcdef", &cfg(false, 3), 0, &f, &lsof_yes, "claude"), "ok");
        assert!(pending::read(d.path(), "s1abcdef").is_none());
    }
}
