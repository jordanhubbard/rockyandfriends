mod helpers;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;

fn no_auth(method: &str, path: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let mut req = Request::builder().method(method).uri(path);
    let body = if let Some(body) = body {
        req = req.header("Content-Type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    req.body(body).unwrap()
}

#[tokio::test]
async fn test_sessions_require_auth() {
    let ts = helpers::TestServer::new().await;

    let cases = [
        no_auth("GET", "/api/sessions", None),
        no_auth("GET", "/api/sessions/slack%3AC123%3Athread", None),
        no_auth(
            "PUT",
            "/api/sessions/slack%3AC123%3Athread",
            Some(json!({"agent": "natasha", "messages": []})),
        ),
        no_auth("DELETE", "/api/sessions/slack%3AC123%3Athread", None),
    ];

    for req in cases {
        let resp = helpers::call(&ts.app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn test_session_persistence_with_auth() {
    let ts = helpers::TestServer::new().await;
    let key = "slack:C123:1710000000.000100";
    let path = "/api/sessions/slack%3AC123%3A1710000000.000100";

    let put_resp = helpers::call(
        &ts.app,
        helpers::put_json(
            path,
            &json!({
                "agent": "natasha",
                "workspace": "omgjkh",
                "messages": [
                    {"role": "user", "content": "fix the build"},
                    {"role": "assistant", "content": "on it"}
                ]
            }),
        ),
    )
    .await;
    assert_eq!(put_resp.status(), StatusCode::OK);

    let get_resp = helpers::call(&ts.app, helpers::get(path)).await;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = helpers::body_json(get_resp).await;
    assert_eq!(body["key"], key);
    assert_eq!(body["messages"].as_array().unwrap().len(), 2);

    let list_resp = helpers::call(&ts.app, helpers::get("/api/sessions")).await;
    assert_eq!(list_resp.status(), StatusCode::OK);
    let body = helpers::body_json(list_resp).await;
    assert_eq!(body["count"], 1);
    assert_eq!(body["sessions"][0]["key"], key);

    let delete_resp = helpers::call(&ts.app, helpers::delete(path)).await;
    assert_eq!(delete_resp.status(), StatusCode::OK);

    let missing_resp = helpers::call(&ts.app, helpers::get(path)).await;
    assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);
}
