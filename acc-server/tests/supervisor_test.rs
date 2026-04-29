mod helpers;

use acc_server::supervisor::{ProcessStatus, SupervisorHandle};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_supervisor_status_no_auth_required() {
    let ts = helpers::TestServer::new().await;
    let req = Request::builder()
        .method("GET")
        .uri("/api/supervisor/status")
        .body(Body::empty())
        .unwrap();
    let resp = helpers::call(&ts.app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_supervisor_status_shape_when_disabled() {
    let ts = helpers::TestServer::new().await;
    let req = Request::builder()
        .method("GET")
        .uri("/api/supervisor/status")
        .body(Body::empty())
        .unwrap();
    let body = helpers::body_json(helpers::call(&ts.app, req).await).await;
    // TestServer sets supervisor: None — enabled=false, empty process list
    assert_eq!(body["processes"].as_array().unwrap().len(), 0);
    assert_eq!(body["enabled"], false);
}

#[tokio::test]
async fn test_supervisor_status_distinguishes_no_healthcheck() {
    let tmp = tempfile::tempdir().unwrap();
    let mut state = helpers::make_state(&tmp).await;
    Arc::get_mut(&mut state).unwrap().supervisor = Some(Arc::new(SupervisorHandle {
        statuses: Arc::new(RwLock::new(vec![ProcessStatus {
            name: "bus".to_string(),
            pid: Some(1234),
            healthy: true,
            health_status: "running_no_healthcheck".to_string(),
            health_reason: Some("no health_url configured".to_string()),
            health_url: None,
            restarts: 0,
            started_at: None,
        }])),
    }));
    let app = acc_server::build_app(state);

    let req = Request::builder()
        .method("GET")
        .uri("/api/supervisor/status")
        .body(Body::empty())
        .unwrap();
    let body = helpers::body_json(helpers::call(&app, req).await).await;
    let proc = &body["processes"][0];
    assert_eq!(proc["healthy"], true);
    assert_eq!(proc["health_status"], "running_no_healthcheck");
    assert_eq!(proc["health_reason"], "no health_url configured");
}
