mod helpers;

use axum::http::StatusCode;
use serde_json::json;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test(flavor = "current_thread")]
async fn test_exec_accepts_legacy_ccc_token_alias_for_signing() {
    let _guard = ENV_LOCK.lock().unwrap();
    let ts = helpers::TestServer::new().await;
    let exec_log = ts.tmp.path().join("exec.jsonl");

    let keys = [
        "AGENTBUS_TOKEN",
        "SQUIRRELBUS_TOKEN",
        "ACC_AGENT_TOKEN",
        "CCC_AGENT_TOKEN",
        "AGENTBUS_URL",
        "SQUIRRELBUS_URL",
        "ACC_URL",
        "CCC_URL",
        "EXEC_LOG_PATH",
    ];
    let saved: Vec<(&str, Option<String>)> = keys
        .iter()
        .map(|key| (*key, std::env::var(key).ok()))
        .collect();

    for key in keys {
        unsafe { std::env::remove_var(key) };
    }
    unsafe {
        std::env::set_var("CCC_AGENT_TOKEN", "legacy-token");
        std::env::set_var("CCC_URL", "http://127.0.0.1:9");
        std::env::set_var("EXEC_LOG_PATH", exec_log.as_os_str());
    }

    let resp = helpers::call(
        &ts.app,
        helpers::post_json(
            "/api/exec",
            &json!({
                "targets": ["all"],
                "command": "ping"
            }),
        ),
    )
    .await;

    for (key, value) in saved {
        unsafe {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }

    assert_eq!(resp.status(), StatusCode::OK);
    let body = helpers::body_json(resp).await;
    assert_eq!(body["ok"], true);
    assert_eq!(body["targets"][0], "all");
}
