use crate::Runner;

/// Send a desktop notification, best-effort. `which` resolves a binary path or None.
pub fn notify_with(title: &str, body: &str, runner: &dyn Runner,
                   which: &dyn Fn(&str) -> Option<String>) {
    if let Some(tn) = which("terminal-notifier") {
        runner.run(&[tn, "-title".into(), title.into(), "-message".into(), body.into()], None);
    } else if let Some(osa) = which("osascript") {
        let script = format!("display notification {:?} with title {:?}", body, title);
        runner.run(&[osa, "-e".into(), script], None);
    }
}

pub fn which(bin: &str) -> Option<String> {
    crate::scheduler::which_path(bin)
}

pub fn notify(title: &str, body: &str, runner: &dyn Runner) {
    notify_with(title, body, runner, &which);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CmdOut;
    use std::cell::RefCell;
    struct Rec(RefCell<Vec<Vec<String>>>);
    impl Runner for Rec { fn run(&self, a: &[String], _c: Option<&str>) -> CmdOut { self.0.borrow_mut().push(a.to_vec()); CmdOut::default() } }

    #[test]
    fn uses_terminal_notifier() {
        let r = Rec(RefCell::new(vec![]));
        notify_with("T", "B", &r, &|n| if n == "terminal-notifier" { Some("/bin/tn".into()) } else { None });
        let calls = r.0.borrow();
        assert_eq!(calls[0][0], "/bin/tn");
        assert!(calls[0].contains(&"B".to_string()));
    }
    #[test]
    fn falls_back_to_osascript() {
        let r = Rec(RefCell::new(vec![]));
        notify_with("T", "B", &r, &|n| if n == "osascript" { Some("/usr/bin/osascript".into()) } else { None });
        assert_eq!(r.0.borrow()[0][0], "/usr/bin/osascript");
    }
    #[test]
    fn noop_when_none() {
        let r = Rec(RefCell::new(vec![]));
        notify_with("T", "B", &r, &|_| None);
        assert!(r.0.borrow().is_empty());
    }
}
