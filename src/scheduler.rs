use crate::Runner;
use chrono::{Local, TimeZone};

pub fn which_path(bin: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/bin/which").arg(bin).output().ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() { return Some(s); }
    }
    None
}

/// Best-effort wake scheduling. `which` injected for tests.
pub fn pmset_wake_with(fire_at: i64, runner: &dyn Runner, which: &dyn Fn(&str) -> Option<String>) {
    let Some(pmset) = which("pmset") else { return };
    let stamp = Local.timestamp_opt(fire_at, 0).single()
        .map(|dt| dt.format("%m/%d/%y %H:%M:%S").to_string())
        .unwrap_or_default();
    runner.run(&[pmset, "schedule".into(), "wake".into(), stamp], None);
}

pub fn pmset_wake(fire_at: i64, runner: &dyn Runner) {
    pmset_wake_with(fire_at, runner, &which_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmdOut;
    use std::cell::RefCell;
    struct Rec(RefCell<Vec<Vec<String>>>);
    impl Runner for Rec { fn run(&self, a: &[String], _c: Option<&str>) -> CmdOut { self.0.borrow_mut().push(a.to_vec()); CmdOut::default() } }

    #[test]
    fn formats_and_calls() {
        let r = Rec(RefCell::new(vec![]));
        pmset_wake_with(0, &r, &|_| Some("/usr/bin/pmset".into()));
        let c = r.0.borrow();
        assert_eq!(&c[0][0..3], &["/usr/bin/pmset".to_string(), "schedule".into(), "wake".into()]);
    }
    #[test]
    fn noop_without_binary() {
        let r = Rec(RefCell::new(vec![]));
        pmset_wake_with(0, &r, &|_| None);
        assert!(r.0.borrow().is_empty());
    }
}
