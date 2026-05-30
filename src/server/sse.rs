use axum::{extract::State, response::IntoResponse, http::StatusCode};
use crate::server::AppState;
pub async fn events(State(_s): State<AppState>) -> impl IntoResponse { StatusCode::OK }
