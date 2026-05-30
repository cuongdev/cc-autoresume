pub mod account;
pub mod parse_reset;
pub mod detect;
pub mod config;
pub mod pending;
pub mod notify;
pub mod scheduler;
pub mod resume;
pub mod arm;
pub mod watch;
pub mod cli;

pub trait Runner {
    fn run(&self, args: &[String], cwd: Option<&str>) -> CmdOut;
}

#[derive(Default, Clone)]
pub struct CmdOut {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

/// Real runner used in production.
pub struct RealRunner;
impl Runner for RealRunner {
    fn run(&self, args: &[String], cwd: Option<&str>) -> CmdOut {
        use std::process::Command;
        if args.is_empty() {
            return CmdOut { code: 127, ..Default::default() };
        }
        let mut c = Command::new(&args[0]);
        c.args(&args[1..]);
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        match c.output() {
            Ok(o) => CmdOut {
                stdout: String::from_utf8_lossy(&o.stdout).to_string(),
                stderr: String::from_utf8_lossy(&o.stderr).to_string(),
                code: o.status.code().unwrap_or(-1),
            },
            Err(_) => CmdOut { code: 127, ..Default::default() },
        }
    }
}
