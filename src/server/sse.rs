use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use std::convert::Infallible;
use std::time::Duration;
use crate::server::{api, AppState};
use futures_core::Stream;

pub async fn events(State(s): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        loop {
            tick.tick().await;
            let json = api::state_json(&s).await;
            yield Ok(Event::default().data(json.to_string()));
        }
    };
    Sse::new(stream)
}

#[cfg(test)]
mod tests {
    use crate::server::{build_router, test_state};
    use crate::config::Config;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn events_requires_token() {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::default(); cfg.token = "tk".into();
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/events").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn events_opens_with_token() {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::default(); cfg.token = "tk".into();
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/events?token=tk").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/event-stream"));
    }
}
