use crate::{config::Config, detect::LimitEvent, notify, parse_reset::parse_reset, pending, pending::Pending, Runner};
use chrono::Utc;
use chrono_tz::Tz;
use std::path::Path;

/// Arm a resume from a LimitEvent. `now`/`default_tz` injected for tests.
/// `schedule` is called with fire_at only when the record is confirmed (auto mode).
pub fn arm(dir: &Path, cfg: &Config, ev: &LimitEvent, now: chrono::DateTime<Utc>,
           default_tz: Tz, runner: &dyn Runner, schedule: &mut dyn FnMut(i64)) -> Option<Pending> {
    // session preset (highest precedence) at <base>/sessions.json, base = parent of pending dir
    let preset = dir.parent()
        .map(|base| crate::sessions::preset_for(&base.join("sessions.json"), &ev.session_id))
        .unwrap_or_default();
    let mode = preset.mode.clone().unwrap_or_else(|| cfg.mode_for(ev.cwd.as_deref()));
    if mode == "off" {
        notify::notify("cc-autoresume", &format!("Hit limit; auto-resume OFF here. Resets {}", ev.reset_str), runner);
        return None;
    }
    let fire_at = parse_reset(&ev.reset_str, now, default_tz)
        .unwrap_or_else(|| now.timestamp() + cfg.backoff.every_sec as i64);
    if pending::exists_for(dir, &ev.session_id, fire_at) { return None; }
    let confirmed = mode != "ask";
    let rec = Pending {
        session_id: ev.session_id.clone(),
        cwd: ev.cwd.clone(),
        transcript_path: ev.transcript_path.clone(),
        reset_str: ev.reset_str.clone(),
        fire_at,
        message: preset.message.clone().unwrap_or_else(|| cfg.message_for(ev.cwd.as_deref())),
        armed_at: now.timestamp(),
        cancelled: false,
        confirmed,
        attempts: 0,
    };
    let _ = pending::write(dir, &rec);
    let short = &ev.session_id[..8.min(ev.session_id.len())];
    if confirmed {
        notify::notify("cc-autoresume", &format!("⏸ Hit limit. Auto-resume armed for {}. `cc-autoresume cancel {}` to stop.", ev.reset_str, short), runner);
        schedule(fire_at);
    } else {
        notify::notify("cc-autoresume", &format!("⏸ Hit limit (resets {}). Run `cc-autoresume arm {}` to enable auto-resume.", ev.reset_str, short), runner);
    }
    Some(rec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmdOut;
    use chrono_tz::Asia::Saigon as SG;
    use chrono::TimeZone;
    struct Null;
    impl Runner for Null { fn run(&self, _a: &[String], _c: Option<&str>) -> CmdOut { CmdOut::default() } }
    fn ev() -> LimitEvent {
        LimitEvent { session_id: "s1abcdef".into(), cwd: Some("/x".into()),
            reset_str: "4pm (Asia/Saigon)".into(), transcript_path: "/t.jsonl".into(), ts: "".into() }
    }
    fn now() -> chrono::DateTime<Utc> {
        SG.with_ymd_and_hms(2026, 5, 30, 1, 0, 0).single().unwrap().with_timezone(&Utc)
    }
    #[test]
    fn writes_pending_confirmed_auto() {
        let d = tempfile::tempdir().unwrap();
        let mut sched = vec![];
        let r = arm(d.path(), &Config::default(), &ev(), now(), SG, &Null, &mut |f| sched.push(f));
        assert!(r.is_some());
        let p = pending::read(d.path(), "s1abcdef").unwrap();
        assert!(p.confirmed);
        assert_eq!(p.fire_at, SG.with_ymd_and_hms(2026,5,30,16,0,0).single().unwrap().timestamp());
        assert_eq!(sched.len(), 1);
    }
    #[test]
    fn off_mode_no_write() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.per_project.insert("/x".into(), crate::config::ProjectCfg { mode: Some("off".into()), message: None });
        assert!(arm(d.path(), &c, &ev(), now(), SG, &Null, &mut |_| {}).is_none());
        assert!(pending::read(d.path(), "s1abcdef").is_none());
    }
    #[test]
    fn ask_mode_unconfirmed_no_schedule() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.per_project.insert("/x".into(), crate::config::ProjectCfg { mode: Some("ask".into()), message: None });
        let mut sched = vec![];
        let r = arm(d.path(), &c, &ev(), now(), SG, &Null, &mut |f| sched.push(f)).unwrap();
        assert!(!r.confirmed);
        assert!(sched.is_empty());
        assert!(pending::due(d.path(), i64::MAX).is_empty());
    }
    #[test]
    fn idempotent() {
        let d = tempfile::tempdir().unwrap();
        assert!(arm(d.path(), &Config::default(), &ev(), now(), SG, &Null, &mut |_| {}).is_some());
        assert!(arm(d.path(), &Config::default(), &ev(), now(), SG, &Null, &mut |_| {}).is_none());
    }
    #[test]
    fn unparseable_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let mut e = ev(); e.reset_str = "soon-ish".into();
        let r = arm(d.path(), &Config::default(), &e, now(), SG, &Null, &mut |_| {}).unwrap();
        assert_eq!(r.fire_at, now().timestamp() + 300);
    }
    #[test]
    fn preset_message_and_mode_take_precedence() {
        let base = tempfile::tempdir().unwrap();
        let pend = base.path().join("pending");
        std::fs::create_dir_all(&pend).unwrap();
        crate::sessions::upsert_preset(&base.path().join("sessions.json"), "s1abcdef",
            Some("preset msg".into()), None);
        let rec = arm(&pend, &Config::default(), &ev(), now(), SG, &Null, &mut |_| {}).unwrap();
        assert_eq!(rec.message, "preset msg");
        assert!(rec.confirmed);
    }
    #[test]
    fn preset_mode_off_blocks() {
        let base = tempfile::tempdir().unwrap();
        let pend = base.path().join("pending");
        std::fs::create_dir_all(&pend).unwrap();
        crate::sessions::upsert_preset(&base.path().join("sessions.json"), "s1abcdef",
            None, Some("off".into()));
        assert!(arm(&pend, &Config::default(), &ev(), now(), SG, &Null, &mut |_| {}).is_none());
    }
    #[test]
    fn preset_mode_ask_unconfirms() {
        let base = tempfile::tempdir().unwrap();
        let pend = base.path().join("pending");
        std::fs::create_dir_all(&pend).unwrap();
        crate::sessions::upsert_preset(&base.path().join("sessions.json"), "s1abcdef",
            None, Some("ask".into()));
        let rec = arm(&pend, &Config::default(), &ev(), now(), SG, &Null, &mut |_| {}).unwrap();
        assert!(!rec.confirmed);
    }
}
