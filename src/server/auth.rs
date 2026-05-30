use axum::{extract::State, http::Request, middleware::Next, response::Response};
use crate::server::AppState;
pub async fn require_token(State(_s): State<AppState>, req: Request<axum::body::Body>, next: Next) -> Response {
    next.run(req).await
}
