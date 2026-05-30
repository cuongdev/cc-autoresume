use axum::{extract::State, http::StatusCode, response::IntoResponse};
use crate::server::AppState;
pub async fn get_state(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_mode(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_message(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_force_headless(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn set_session_message(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn cancel_pending(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn arm_pending(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn fire_pending(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn open_terminal(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
pub async fn rotate_token(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
