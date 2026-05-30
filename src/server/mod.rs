pub mod auth;
pub mod api;
pub mod sse;

use crate::Runner;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatcherStatus {
    pub running: bool,
    pub last_scan_epoch: i64,
    pub sessions_tracked: usize,
}

#[derive(Clone)]
pub struct AppState {
    pub base: PathBuf,
    pub projects_dir: PathBuf,
    pub home: PathBuf,
    pub runner: Arc<dyn Runner + Send + Sync>,
    pub status: Arc<RwLock<WatcherStatus>>,
}

impl AppState {
    pub fn config_path(&self) -> PathBuf { self.base.join("config.json") }
    pub fn pending_dir(&self) -> PathBuf { self.base.join("pending") }
    pub fn stats_path(&self) -> PathBuf { self.base.join("stats.json") }
}

pub fn build_router(state: AppState) -> axum::Router {
    use axum::routing::{get, post};
    use axum::middleware;
    let protected = axum::Router::new()
        .route("/api/state", get(api::get_state))
        .route("/api/mode", post(api::set_mode))
        .route("/api/message", post(api::set_message))
        .route("/api/force-headless", post(api::set_force_headless))
        .route("/api/pending/:id/message", post(api::set_session_message))
        .route("/api/pending/:id/cancel", post(api::cancel_pending))
        .route("/api/pending/:id/arm", post(api::arm_pending))
        .route("/api/pending/:id/fire", post(api::fire_pending))
        .route("/api/session/:id/open", post(api::open_terminal))
        .route("/api/token/rotate", post(api::rotate_token))
        .route("/events", get(sse::events))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_token));
    axum::Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(protected)
        .with_state(state)
}

#[cfg(test)]
pub fn test_state(base: PathBuf, home: PathBuf) -> AppState {
    AppState {
        projects_dir: home.join(".claude/projects"),
        base, home,
        runner: Arc::new(crate::RealRunner),
        status: Arc::new(RwLock::new(WatcherStatus::default())),
    }
}

use crate::{config::Config, pending, resume, scheduler, watch::Watcher, RealRunner};
use chrono::Utc;

/// Blocking entrypoint for `cc-autoresume watch`: runs the scan/fire loop and the HTTP server.
pub fn serve(home: PathBuf) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        let base = home.join(".claude/auto-resume");
        let mut cfg = Config::load(&base.join("config.json"));
        if cfg.ensure_token() { let _ = cfg.save(&base.join("config.json")); }
        let port = cfg.port;
        let status = Arc::new(RwLock::new(WatcherStatus { running: true, ..Default::default() }));
        let state = AppState {
            base: base.clone(),
            projects_dir: home.join(".claude/projects"),
            home: home.clone(),
            runner: Arc::new(RealRunner),
            status: status.clone(),
        };
        let loop_base = base.clone();
        let loop_projects = home.join(".claude/projects");
        let loop_status = status.clone();
        tokio::spawn(async move {
            let tz = std::env::var("TZ").ok().and_then(|t| t.parse().ok()).unwrap_or(chrono_tz::UTC);
            let pending_dir = loop_base.join("pending");
            let config_path = loop_base.join("config.json");
            let stats_path = loop_base.join("stats.json");
            let mut w = Watcher::new(loop_projects);
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(20));
            loop {
                tick.tick().await;
                let cfg = Config::load(&config_path);
                let now = Utc::now();
                let armed = w.scan_once(&pending_dir, &cfg, now, tz, &RealRunner);
                for rec in &armed { crate::stats::Stats::record_limit_hit(&stats_path, rec.armed_at); }
                for r in pending::due(&pending_dir, now.timestamp()) {
                    let outcome = resume::fire(&pending_dir, &r.session_id, &cfg, now.timestamp(), &RealRunner, &scheduler::which_path, "claude");
                    crate::stats::Stats::record_resume(&stats_path, crate::stats::RecentEntry {
                        session_id: r.session_id.clone(), cwd: r.cwd.clone(), transcript_path: r.transcript_path.clone(),
                        outcome: outcome.to_string(), at: now.timestamp() });
                }
                let mut st = loop_status.write().await;
                st.last_scan_epoch = now.timestamp();
                st.sessions_tracked = w.offsets.len();
            }
        });
        let app = build_router(state);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
        eprintln!("cc-autoresume dashboard on http://0.0.0.0:{port}");
        axum::serve(listener, app).await.expect("serve");
    });
}
