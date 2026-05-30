use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use axum::extract::Path;
use std::convert::Infallible;
use std::time::Duration;
use std::io::{Read, Seek, SeekFrom};
use serde::Deserialize;
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

#[derive(Deserialize)]
pub struct AfterQ { #[serde(default)] pub after: u64 }

pub async fn session_stream(State(s): State<AppState>, Path(id): Path<String>,
                            axum::extract::Query(q): axum::extract::Query<AfterQ>)
    -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let path = api::find_transcript(&s, &id);
    let mut offset = q.after;
    let stream = async_stream::stream! {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let Some(ref p) = path else { continue };
            let Ok(meta) = std::fs::metadata(p) else { continue };
            let size = meta.len();
            if size < offset { offset = 0; }
            if size == offset { continue; }
            let Ok(mut f) = std::fs::File::open(p) else { continue };
            if f.seek(SeekFrom::Start(offset)).is_err() { continue; }
            let mut chunk = String::new();
            if f.read_to_string(&mut chunk).is_err() { continue; }
            offset = size;
            for line in chunk.lines() {
                for m in crate::transcript::parse_line(line) {
                    yield Ok(Event::default().data(serde_json::to_string(&m).unwrap()));
                }
            }
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
        let cfg = Config { token: "tk".into(), ..Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/events").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn events_opens_with_token() {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = Config { token: "tk".into(), ..Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/events?token=tk").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/event-stream"));
    }

    #[tokio::test]
    async fn session_stream_requires_token() {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = Config { token: "tk".into(), ..Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/session/x/stream").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn session_stream_opens_event_stream() {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cfg = Config { token: "tk".into(), ..Config::default() };
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/api/session/x/stream?token=tk").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().get("content-type").unwrap().to_str().unwrap().contains("text/event-stream"));
    }
}
