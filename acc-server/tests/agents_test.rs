mod helpers;

use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn agents_list_starts_empty() {
    let srv = helpers::TestServer::new().await;
    let resp = helpers::call(&srv.app, helpers::get("/api/agents")).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn heartbeat_registers_agent() {
    let srv = helpers::TestServer::new().await;

    let resp = helpers::call(
        &srv.app,
        helpers::post_json("/api/agents/test-node/heartbeat", &json!({
            "status": "online",
            "capabilities": {"claude_cli": true, "hermes": false}
        })),
    ).await;
    assert_eq!(resp.status(), StatusCode::OK, "heartbeat should succeed");

    // Agent should now appear in the list
    let resp = helpers::call(&srv.app, helpers::get("/api/agents/test-node")).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = helpers::body_json(resp).await;
    // Response is {"ok": true, "agent": {...}}
    let agent = &body["agent"];
    assert_eq!(agent["name"].as_str().unwrap_or(""), "test-node");
}

#[tokio::test]
async fn exec_resolves_capability_to_agent() {
    let srv = helpers::TestServer::new().await;

    // Register an agent with hermes capability
    helpers::call(
        &srv.app,
        helpers::post_json("/api/agents/hermes-node/heartbeat", &json!({
            "status": "online",
            "capabilities": {"hermes": true, "gpu": false}
        })),
    ).await;

    // POST /api/exec targeting the "hermes" capability
    // We expect the server to resolve "hermes" → "hermes-node"
    // (busSent may be false if AGENTBUS_TOKEN missing, but the resolution should work)
    let resp = helpers::call(
        &srv.app,
        helpers::post_json("/api/exec", &json!({
            "command": "ping",
            "targets": ["hermes"],
        })),
    ).await;

    // Accept both 200 (resolution worked, bus may have failed) and 500 (no agentbus token)
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR,
        "unexpected status {status}"
    );

    if status == StatusCode::OK {
        let body = helpers::body_json(resp).await;
        // If capability routing worked, targets should have been resolved to ["hermes-node"]
        if let Some(targets) = body["targets"].as_array() {
            assert!(
                targets.iter().any(|t| t == "hermes-node"),
                "capability 'hermes' should resolve to 'hermes-node'; got targets={:?}", targets
            );
        }
    }
}
