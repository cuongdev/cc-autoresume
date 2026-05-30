use axum::response::Html;

const INDEX_HTML: &str = include_str!("../web/index.html");

pub async fn serve_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

#[cfg(test)]
mod tests {
    use crate::server::{build_router, test_state};
    use crate::config::Config;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn index_served_without_token() {
        let base = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let mut cfg = Config::default(); cfg.token = "tk".into();
        cfg.save(&base.path().join("config.json")).unwrap();
        let app = build_router(test_state(base.path().into(), home.path().into()));
        let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/html"));
        let body = res.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&body).contains("cc-autoresume"));
    }
}
