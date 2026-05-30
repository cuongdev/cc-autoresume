use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use crate::{account, config::Config, pending, sessions, stats, server::AppState};
use chrono::Utc;
use crate::{resume, scheduler, transcript};

pub async fn state_json(s: &AppState) -> serde_json::Value {
    let now = chrono::Utc::now().timestamp();
    let cfg = Config::load(&s.config_path());
    let st = stats::Stats::load(&s.stats_path());
    let pend = pending::list_all(&s.pending_dir());
    let next_reset = pend.iter().filter(|r| !r.cancelled && r.confirmed && r.fire_at > now).map(|r| r.fire_at).min();
    let status = s.status.read().await.clone();
    serde_json::json!({
        "account": account::read(&s.home),
        "mode": cfg.mode, "forceHeadless": cfg.force_headless, "defaultMessage": cfg.default_message,
        "perProject": cfg.per_project, "watcher": status,
        "stats": { "limitHits7d": st.hits_7d(now), "autoResumes": st.auto_resumes,
            "sessions7d": stats::sessions_7d(&s.home, now), "nextResetEpoch": next_reset },
        "pending": pend, "recent": st.recent,
    })
}

pub async fn get_state(State(s): State<AppState>) -> impl IntoResponse {
    Json(state_json(&s).await)
}

#[derive(serde::Deserialize)]
pub struct ModeBody { pub value: String }
pub async fn set_mode(State(s): State<AppState>, Json(b): Json<ModeBody>) -> impl IntoResponse {
    if !["auto","ask","off"].contains(&b.value.as_str()) { return StatusCode::BAD_REQUEST; }
    let mut cfg = Config::load(&s.config_path());
    cfg.mode = b.value;
    let _ = cfg.save(&s.config_path());
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct MsgBody { pub text: String, #[serde(default)] pub cwd: Option<String> }
pub async fn set_message(State(s): State<AppState>, Json(b): Json<MsgBody>) -> impl IntoResponse {
    let mut cfg = Config::load(&s.config_path());
    match b.cwd {
        Some(c) => { cfg.per_project.entry(c).or_default().message = Some(b.text); }
        None => { cfg.default_message = b.text; }
    }
    let _ = cfg.save(&s.config_path());
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct FhBody { pub value: bool }
pub async fn set_force_headless(State(s): State<AppState>, Json(b): Json<FhBody>) -> impl IntoResponse {
    let mut cfg = Config::load(&s.config_path());
    cfg.force_headless = b.value;
    let _ = cfg.save(&s.config_path());
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct TextBody { pub text: String }
pub async fn set_session_message(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>, Json(b): Json<TextBody>) -> impl IntoResponse {
    match pending::read(&s.pending_dir(), &id) {
        Some(mut r) => { r.message = b.text; let _ = pending::write(&s.pending_dir(), &r); StatusCode::OK }
        None => StatusCode::NOT_FOUND,
    }
}

pub async fn cancel_pending(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    if pending::cancel(&s.pending_dir(), &id) { StatusCode::OK } else { StatusCode::NOT_FOUND }
}

pub async fn arm_pending(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    match pending::read(&s.pending_dir(), &id) {
        Some(mut r) => {
            r.confirmed = true; r.cancelled = false;
            let _ = pending::write(&s.pending_dir(), &r);
            scheduler::pmset_wake(r.fire_at, s.runner.as_ref());
            StatusCode::OK
        }
        None => StatusCode::NOT_FOUND,
    }
}

pub async fn fire_pending(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    if pending::read(&s.pending_dir(), &id).is_none() { return StatusCode::NOT_FOUND; }
    let cfg = Config::load(&s.config_path());
    let now = Utc::now().timestamp();
    let _ = resume::fire(&s.pending_dir(), &id, &cfg, now, s.runner.as_ref(), &scheduler::which_path, "claude");
    StatusCode::OK
}

pub async fn open_terminal(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    let Some(tp) = find_transcript(&s, &id) else { return StatusCode::NOT_FOUND; };
    let cwd = crate::detect::resolve_cwd(&tp).unwrap_or_else(|| ".".into());
    // Write a .command script and `open` it. This launches Terminal WITHOUT needing
    // Automation (AppleEvent) permission, which a background LaunchAgent can't obtain.
    let body = format!("#!/bin/bash\ncd {} && claude --resume {}\n", sh_quote(&cwd), id);
    let safe_id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(36).collect();
    let path = std::env::temp_dir().join(format!("cc-autoresume-{safe_id}.command"));
    if std::fs::write(&path, body).is_err() { return StatusCode::INTERNAL_SERVER_ERROR; }
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt;
      let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)); }
    let out = s.runner.run(&["open".into(), path.to_string_lossy().into_owned()], None);
    if out.code == 0 { StatusCode::OK } else { StatusCode::INTERNAL_SERVER_ERROR }
}

/// Single-quote a string for safe interpolation into a bash script.
fn sh_quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }

pub async fn list_sessions(State(s): State<AppState>) -> impl IntoResponse {
    let projects = s.projects_dir.clone();
    let presets = s.base.join("sessions.json");
    let pend = s.pending_dir();
    let list = tokio::task::spawn_blocking(move || {
        let now = chrono::Utc::now().timestamp();
        sessions::discover(&projects, &presets, &pend, now, 86_400, 60, 120)
    }).await.unwrap_or_default();
    let count = list.len();
    Json(serde_json::json!({ "sessions": list, "count": count }))
}

#[derive(serde::Deserialize)]
pub struct PresetBody { #[serde(default)] pub message: Option<String>, #[serde(default)] pub mode: Option<String> }

pub async fn set_preset(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>,
                        Json(b): Json<PresetBody>) -> impl IntoResponse {
    if let Some(ref m) = b.mode {
        if !["auto", "ask", "off", ""].contains(&m.as_str()) { return StatusCode::BAD_REQUEST; }
    }
    sessions::upsert_preset(&s.base.join("sessions.json"), &id, b.message.clone(), b.mode.clone());
    if let Some(mut rec) = pending::read(&s.pending_dir(), &id) {
        if let Some(msg) = &b.message { if !msg.is_empty() { rec.message = msg.clone(); } }
        match b.mode.as_deref() {
            Some("auto") => { rec.confirmed = true; rec.cancelled = false; }
            Some("ask")  => { rec.confirmed = false; }
            Some("off")  => { rec.cancelled = true; }
            _ => {}
        }
        let _ = pending::write(&s.pending_dir(), &rec);
    }
    StatusCode::OK
}

#[derive(serde::Deserialize)]
pub struct QrQ { pub data: String }

pub async fn qr_svg(State(_s): State<AppState>, axum::extract::Query(q): axum::extract::Query<QrQ>) -> impl IntoResponse {
    use qrcode::QrCode;
    use qrcode::render::svg;
    match QrCode::new(q.data.as_bytes()) {
        Ok(code) => {
            let s = code.render::<svg::Color>().min_dimensions(180, 180).quiet_zone(true).build();
            ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], s).into_response()
        }
        Err(_) => StatusCode::BAD_REQUEST.into_response(),
    }
}

/// Resolve a session id to its transcript path: pending record → recent → search projects dir.
pub fn find_transcript(s: &AppState, id: &str) -> Option<std::path::PathBuf> {
    if let Some(r) = pending::read(&s.pending_dir(), id) {
        let p = std::path::PathBuf::from(&r.transcript_path);
        if p.exists() { return Some(p); }
    }
    if let Some(r) = stats::Stats::load(&s.stats_path()).recent.into_iter().find(|r| r.session_id == id) {
        let p = std::path::PathBuf::from(&r.transcript_path);
        if p.exists() { return Some(p); }
    }
    fn walk(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        let rd = std::fs::read_dir(dir).ok()?;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { if let Some(f) = walk(&p, name) { return Some(f); } }
            else if p.file_name().and_then(|n| n.to_str()) == Some(name) { return Some(p); }
        }
        None
    }
    walk(&s.projects_dir, &format!("{id}.jsonl"))
}

pub async fn session_messages(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    match find_transcript(&s, &id) {
        Some(path) => {
            let msgs = transcript::read_messages(&path);
            let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Json(serde_json::json!({ "messages": msgs, "offset": offset })).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(serde::Serialize)]
pub struct TokenResp { pub token: String }
pub async fn rotate_token(State(s): State<AppState>) -> impl IntoResponse {
    let mut cfg = Config::load(&s.config_path());
    cfg.token = String::new();
    cfg.ensure_token();
    let _ = cfg.save(&s.config_path());
    Json(TokenResp { token: cfg.token })
}

pub async fn selftest(State(s): State<AppState>) -> impl IntoResponse {
    let runner = s.runner.clone();
    let (bin, code, version, path) = tokio::task::spawn_blocking(move || {
        let bin = resume::resolve_claude_bin();
        let out = runner.run(&[bin.clone(), "--version".into()], None);
        let mut v = out.stdout.trim().to_string();
        if v.is_empty() { v = out.stderr.trim().to_string(); }
        (bin, out.code, v, std::env::var("PATH").unwrap_or_default())
    }).await.unwrap_or_else(|_| ("claude".into(), -1, "spawn failed".into(), String::new()));
    Json(serde_json::json!({ "claudeBin": bin, "ok": code == 0, "code": code, "version": version, "daemonPath": path }))
}

#[derive(serde::Deserialize)]
pub struct TestResumeBody { #[serde(default)] pub message: Option<String> }

pub async fn test_resume(State(s): State<AppState>, axum::extract::Path(id): axum::extract::Path<String>,
                         Json(b): Json<TestResumeBody>) -> impl IntoResponse {
    let Some(tp) = find_transcript(&s, &id) else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"outcome":"not-found"}))).into_response();
    };
    let mut cfg = Config::load(&s.config_path());
    cfg.force_headless = true; // a test must actually run claude even if a TUI holds the session
    let msg = b.message.filter(|m| !m.is_empty()).unwrap_or_else(|| cfg.message_for(None));
    let cwd = crate::detect::resolve_cwd(&tp);
    let runner = s.runner.clone();
    let id2 = id.clone();
    let tp2 = tp.to_string_lossy().into_owned();
    let outcome = tokio::task::spawn_blocking(move || {
        let bin = resume::resolve_claude_bin();
        let rec = pending::Pending {
            session_id: id2, cwd, transcript_path: tp2, reset_str: String::new(),
            fire_at: 0, message: msg, armed_at: 0, cancelled: false, confirmed: true, attempts: 0,
        };
        resume::run_resume(&rec, &cfg, runner.as_ref(), &scheduler::which_path, &bin)
    }).await.unwrap_or("error");
    Json(serde_json::json!({ "outcome": outcome })).into_response()
}

#[cfg(test)]
mod tests {
    use crate::server::{build_router, test_state};
    use crate::config::Config;
    use crate::pending::{self, Pending};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn setup() -> (tempfile::TempDir, tempfile::TempDir, String) {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = Config { token: "secrettoken".into(), ..Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        (base, home, "secrettoken".to_string())
    }

    #[tokio::test]
    async fn state_requires_token() {
        let (base, home, _t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/state").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn state_ok_with_token() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/state")
            .header("authorization", format!("Bearer {t}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["mode"], "auto");
        assert!(v["stats"]["autoResumes"].is_number());
        assert!(v["pending"].is_array());
    }

    #[tokio::test]
    async fn state_ok_with_query_token() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri(format!("/api/state?token={t}"))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn healthz_open() {
        let (base, home, _t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    fn seed(base: &std::path::Path) {
        let dir = base.join("pending");
        let rec = Pending { session_id: "deadbeef".into(), cwd: Some("/x".into()), transcript_path: "/t".into(),
            reset_str: "4pm".into(), fire_at: 9_999_999_999, message: "m".into(), armed_at: 0,
            cancelled: false, confirmed: false, attempts: 0 };
        pending::write(&dir, &rec).unwrap();
    }

    #[tokio::test]
    async fn set_mode_persists() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().method("POST").uri("/api/mode")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"value":"off"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(Config::load(&base.path().join("config.json")).mode, "off");
    }

    #[tokio::test]
    async fn set_mode_rejects_bad() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().method("POST").uri("/api/mode")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"value":"bogus"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cancel_and_arm_and_session_message() {
        let (base, home, t) = setup();
        seed(base.path());
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/pending/deadbeef/message")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"text":"do X"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(pending::read(&base.path().join("pending"), "deadbeef").unwrap().message, "do X");
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/pending/deadbeef/arm")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(pending::read(&base.path().join("pending"), "deadbeef").unwrap().confirmed);
        let res = app.oneshot(Request::builder().method("POST").uri("/api/pending/nope/cancel")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rotate_changes_token() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/token/rotate")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app.oneshot(Request::builder().uri("/api/state")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn session_messages_reads_transcript() {
        let (base, home, t) = setup();
        std::fs::create_dir_all(home.path().join("proj")).unwrap();
        let tpath = home.path().join("proj/sess.jsonl");
        std::fs::write(&tpath, "{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n").unwrap();
        let rec = Pending { session_id: "sess".into(), cwd: None, transcript_path: tpath.to_string_lossy().into(),
            reset_str: "4pm".into(), fire_at: 1, message: "m".into(), armed_at: 0, cancelled: false, confirmed: true, attempts: 0 };
        pending::write(&base.path().join("pending"), &rec).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/session/sess/messages")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["messages"][0]["text"], "hello");
        assert!(v["offset"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn session_messages_unknown_404() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/session/nope/messages")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn qr_returns_svg() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/qr?data=http://x/?token=abc")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get("content-type").unwrap().to_str().unwrap().contains("image/svg+xml"));
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("<svg"));
    }

    #[tokio::test]
    async fn list_sessions_returns_array() {
        let (base, home, t) = setup();
        std::fs::create_dir_all(home.path().join(".claude/projects/p")).unwrap();
        std::fs::write(home.path().join(".claude/projects/p/zz.jsonl"),
            "{\"type\":\"user\",\"cwd\":\"/w\",\"message\":{\"content\":\"x\"}}\n").unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/sessions")
            .header("authorization", format!("Bearer {t}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(v["sessions"].is_array());
        assert_eq!(v["count"], v["sessions"].as_array().unwrap().len());
    }

    #[tokio::test]
    async fn set_preset_persists_and_validates() {
        let (base, home, t) = setup();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/session/s1/preset")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"mode":"bogus"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let res = app.oneshot(Request::builder().method("POST").uri("/api/session/s1/preset")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"message":"do X","mode":"ask"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let pr = crate::sessions::preset_for(&base.path().join("sessions.json"), "s1");
        assert_eq!(pr.message.as_deref(), Some("do X"));
        assert_eq!(pr.mode.as_deref(), Some("ask"));
    }

    #[tokio::test]
    async fn set_preset_updates_live_pending() {
        let (base, home, t) = setup();
        seed(base.path());
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().method("POST").uri("/api/session/deadbeef/preset")
            .header("authorization", format!("Bearer {t}")).header("content-type","application/json")
            .body(Body::from(r#"{"message":"live update","mode":"auto"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let rec = pending::read(&base.path().join("pending"), "deadbeef").unwrap();
        assert_eq!(rec.message, "live update");
        assert!(rec.confirmed);
    }

    #[tokio::test]
    async fn selftest_runs_version() {
        use crate::{CmdOut, Runner};
        use std::sync::{Arc, Mutex};
        struct R(Mutex<Vec<Vec<String>>>);
        impl Runner for R { fn run(&self, a:&[String], _c:Option<&str>)->CmdOut { self.0.lock().unwrap().push(a.to_vec()); CmdOut{ stdout:"claude 9.9.9 (test)".into(), stderr:String::new(), code:0 } } }
        let base = tempfile::tempdir().unwrap(); let home = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config { token: "tk".into(), ..crate::config::Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        let rec = Arc::new(R(Mutex::new(vec![])));
        let state = crate::server::AppState { base: base.path().into(), home: home.path().into(),
            projects_dir: home.path().join(".claude/projects"), runner: rec.clone(),
            status: Arc::new(tokio::sync::RwLock::new(crate::server::WatcherStatus::default())) };
        let app = build_router(state);
        let res = app.oneshot(Request::builder().uri("/api/selftest").header("authorization","Bearer tk").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["ok"], true);
        assert!(v["version"].as_str().unwrap().contains("9.9.9"));
        assert!(rec.0.lock().unwrap()[0].contains(&"--version".to_string()));
    }

    #[tokio::test]
    async fn test_resume_runs_and_404() {
        use crate::{CmdOut, Runner};
        use std::sync::{Arc, Mutex};
        struct R(Mutex<Vec<Vec<String>>>);
        impl Runner for R { fn run(&self, a:&[String], _c:Option<&str>)->CmdOut { self.0.lock().unwrap().push(a.to_vec()); CmdOut{ stdout:"done".into(), stderr:String::new(), code:0 } } }
        let base = tempfile::tempdir().unwrap(); let home = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config { token: "tk".into(), ..crate::config::Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        // a transcript so find_transcript resolves
        std::fs::create_dir_all(home.path().join(".claude/projects/p")).unwrap();
        std::fs::write(home.path().join(".claude/projects/p/sess.jsonl"), "{\"type\":\"user\",\"cwd\":\"/w\",\"message\":{\"content\":\"x\"}}\n").unwrap();
        let rec = Arc::new(R(Mutex::new(vec![])));
        let state = crate::server::AppState { base: base.path().into(), home: home.path().into(),
            projects_dir: home.path().join(".claude/projects"), runner: rec.clone(),
            status: Arc::new(tokio::sync::RwLock::new(crate::server::WatcherStatus::default())) };
        let app = build_router(state);
        // known session -> runs claude -> ok
        let res = app.clone().oneshot(Request::builder().method("POST").uri("/api/session/sess/test-resume")
            .header("authorization","Bearer tk").header("content-type","application/json").body(Body::from(r#"{"message":"hi"}"#)).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["outcome"], "ok");
        assert!(rec.0.lock().unwrap().iter().any(|c| c.iter().any(|s| s.contains("--resume"))));
        // unknown -> 404
        let res = app.oneshot(Request::builder().method("POST").uri("/api/session/nope/test-resume")
            .header("authorization","Bearer tk").header("content-type","application/json").body(Body::from("{}")).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn open_terminal_invokes_osascript() {
        use crate::{CmdOut, Runner};
        use std::sync::{Arc, Mutex};
        struct Rec(Mutex<Vec<Vec<String>>>);
        impl Runner for Rec { fn run(&self, a: &[String], _c: Option<&str>) -> CmdOut { self.0.lock().unwrap().push(a.to_vec()); CmdOut { code: 0, ..Default::default() } } }
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = crate::config::Config { token: "tk".into(), ..crate::config::Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        seed(base.path());
        // open_terminal resolves via find_transcript, which requires a real transcript file
        std::fs::create_dir_all(home.path().join(".claude/projects/p")).unwrap();
        std::fs::write(home.path().join(".claude/projects/p/deadbeef.jsonl"), "{\"cwd\":\"/x\"}\n").unwrap();
        let rec = Arc::new(Rec(Mutex::new(vec![])));
        let state = crate::server::AppState {
            base: base.path().into(), home: home.path().into(), projects_dir: home.path().join(".claude/projects"),
            runner: rec.clone(), status: Arc::new(tokio::sync::RwLock::new(crate::server::WatcherStatus::default())),
        };
        let app = build_router(state);
        let res = app.oneshot(Request::builder().method("POST").uri("/api/session/deadbeef/open")
            .header("authorization", "Bearer tk").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let calls = rec.0.lock().unwrap();
        assert_eq!(calls[0][0], "open");                                   // launches via `open`, not osascript
        let cmd_file = &calls[0][1];                                       // the .command script path
        assert!(cmd_file.ends_with(".command"));
        let body = std::fs::read_to_string(cmd_file).unwrap();
        assert!(body.contains("claude --resume deadbeef"));               // script runs the resume
    }
}
