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
