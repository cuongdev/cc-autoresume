use crate::{config::Config, pending, resume, RealRunner};
use chrono::Utc;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

fn dirs_home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/tmp"))
}
fn base_dir() -> PathBuf { dirs_home().join(".claude/auto-resume") }
fn config_path() -> PathBuf { base_dir().join("config.json") }
fn pending_dir() -> PathBuf { base_dir().join("pending") }

#[derive(Parser)]
#[command(name = "cc-autoresume")]
struct Cli { #[command(subcommand)] cmd: Cmd }

#[derive(Subcommand)]
enum Cmd {
    Watch,
    List,
    Status,
    Mode { value: String },
    Msg { text: String },
    Cancel { prefix: Option<String> },
    Arm { prefix: String },
    Fire { session: String },
    Dashboard,
    Token { #[arg(long)] rotate: bool },
}

fn short(id: &str) -> &str { &id[..8.min(id.len())] }

pub fn run(args: Vec<String>) -> i32 {
    let argv = std::iter::once("cc-autoresume".to_string()).chain(args);
    let cli = match Cli::try_parse_from(argv) {
        Ok(c) => c,
        Err(e) => { let _ = e.print(); return 2; }
    };
    match cli.cmd {
        Cmd::Watch => { crate::server::serve(dirs_home()); 0 }
        Cmd::Mode { value } => {
            if !["auto","ask","off"].contains(&value.as_str()) { eprintln!("mode must be auto|ask|off"); return 2; }
            let mut c = Config::load(&config_path()); c.mode = value.clone(); let _ = c.save(&config_path());
            println!("mode = {value}"); 0
        }
        Cmd::Msg { text } => {
            let mut c = Config::load(&config_path()); c.default_message = text; let _ = c.save(&config_path());
            println!("default resume message updated"); 0
        }
        Cmd::List | Cmd::Status => {
            let recs = pending::list_all(&pending_dir());
            if recs.is_empty() { println!("no pending resumes"); }
            for r in recs {
                let flag = if r.cancelled { "cancelled".to_string() }
                    else if !r.confirmed { format!("awaiting confirm (resets {})", r.reset_str) }
                    else { format!("fires {}", r.reset_str) };
                println!("{}  {}  {}", short(&r.session_id), r.cwd.unwrap_or_default(), flag);
            }
            0
        }
        Cmd::Cancel { prefix } => {
            match prefix {
                None => { for r in pending::list_all(&pending_dir()) { pending::cancel(&pending_dir(), &r.session_id); }
                          println!("cancelled all"); 0 }
                Some(p) => {
                    let id = resolve(&p).unwrap_or(p);
                    if pending::cancel(&pending_dir(), &id) { println!("cancelled"); 0 } else { println!("no match"); 1 }
                }
            }
        }
        Cmd::Arm { prefix } => {
            let id = resolve(&prefix).unwrap_or(prefix);
            match pending::read(&pending_dir(), &id) {
                Some(mut r) => { r.confirmed = true; r.cancelled = false; let _ = pending::write(&pending_dir(), &r);
                    crate::scheduler::pmset_wake(r.fire_at, &RealRunner); println!("armed {}", short(&id)); 0 }
                None => { println!("no match"); 1 }
            }
        }
        Cmd::Fire { session } => {
            let id = resolve(&session).unwrap_or(session);
            let c = Config::load(&config_path());
            let s = resume::fire(&pending_dir(), &id, &c, Utc::now().timestamp(), &RealRunner, &crate::scheduler::which_path, "claude");
            println!("{s}"); 0
        }
        Cmd::Dashboard => { let c = ensure_cfg(); println!("http://{}:{}/?token={}", lan_ip(), c.port, c.token); 0 }
        Cmd::Token { rotate } => {
            let mut c = Config::load(&config_path());
            if rotate { c.token = String::new(); }
            c.ensure_token(); let _ = c.save(&config_path());
            println!("{}", c.token); 0
        }
    }
}

fn resolve(prefix: &str) -> Option<String> {
    let m: Vec<_> = pending::list_all(&pending_dir()).into_iter()
        .map(|r| r.session_id).filter(|s| s.starts_with(prefix)).collect();
    if m.len() == 1 { Some(m[0].clone()) } else { None }
}
fn ensure_cfg() -> Config {
    let mut c = Config::load(&config_path());
    if c.ensure_token() { let _ = c.save(&config_path()); }
    c
}
fn lan_ip() -> String {
    let out = std::process::Command::new("/usr/sbin/ipconfig").args(["getifaddr","en0"]).output();
    out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty()).unwrap_or_else(|| "127.0.0.1".into())
}

