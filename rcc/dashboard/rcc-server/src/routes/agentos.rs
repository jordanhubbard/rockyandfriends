/// /api/agentos — AgentOS routes (stub implementation)
/// Full implementation is SOA-010 (complex, later phase).
/// This stub provides the endpoints the dashboard WASM uses today.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use crate::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/agentos/timeline", get(get_timeline))
        .route("/api/agentos/events", get(get_events))
        .route("/api/agentos/cap-events", get(get_cap_events).post(post_cap_event))
        .route("/api/agentos/cap-events/push", post(push_cap_event))
        .route("/api/agentos/slots", get(get_slots))
        .route("/api/agentos/shell", get(shell_stub))
        .route("/api/agentos/debug/sessions", get(debug_sessions))
        .route("/api/upvote/:id", post(upvote))
}

// ── GET /api/agentos/timeline ─────────────────────────────────────────────

async fn get_timeline(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit: usize = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);

    // Read from bus log as event source
    let bus_path = std::env::var("BUS_LOG_PATH")
        .unwrap_or_else(|_| "./data/bus.jsonl".to_string());

    let events: Vec<Value> = match tokio::fs::read_to_string(&bus_path).await {
        Ok(content) => content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        Err(_) => vec![],
    };

    Json(json!({"ok": true, "events": events, "count": events.len()}))
}

// ── GET /api/agentos/events ───────────────────────────────────────────────

async fn get_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit: usize = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50);

    // Cap events stored in data/cap-events.jsonl
    let path = std::env::var("CAP_EVENTS_PATH")
        .unwrap_or_else(|_| "./data/cap-events.jsonl".to_string());

    let events: Vec<Value> = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        Err(_) => vec![],
    };

    Json(json!({"ok": true, "events": events}))
}

// ── GET/POST /api/agentos/cap-events ─────────────────────────────────────

async fn get_cap_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit: usize = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
    let path = std::env::var("CAP_EVENTS_PATH")
        .unwrap_or_else(|_| "./data/cap-events.jsonl".to_string());

    let events: Vec<Value> = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        Err(_) => vec![],
    };

    Json(json!({"ok": true, "events": events, "count": events.len()}))
}

async fn post_cap_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    push_event_to_log(body).await;
    Json(json!({"ok": true}))
}

async fn push_cap_event(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    push_event_to_log(body).await;
    Json(json!({"ok": true}))
}

async fn push_event_to_log(mut body: Value) {
    if body.get("ts").is_none() {
        body.as_object_mut().unwrap().insert("ts".to_string(), json!(chrono::Utc::now().to_rfc3339()));
    }
    let path = std::env::var("CAP_EVENTS_PATH")
        .unwrap_or_else(|_| "./data/cap-events.jsonl".to_string());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(mut line) = serde_json::to_string(&body) {
        line.push('\n');
        use tokio::io::AsyncWriteExt;
        if let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open(&path).await {
            let _ = f.write_all(line.as_bytes()).await;
        }
    }
}

// ── GET /api/agentos/slots ────────────────────────────────────────────────

async fn get_slots() -> impl IntoResponse {
    Json(json!({"ok": true, "slots": [], "note": "slot management is SOA-010"}))
}

// ── GET /api/agentos/shell ────────────────────────────────────────────────

async fn shell_stub() -> impl IntoResponse {
    (axum::http::StatusCode::NOT_IMPLEMENTED, Json(json!({"error": "shell endpoint not yet implemented in Rust — SOA-010"})))
}

// ── GET /api/agentos/debug/sessions ──────────────────────────────────────

async fn debug_sessions() -> impl IntoResponse {
    Json(json!({"ok": true, "sessions": [], "note": "debug sessions are SOA-010"}))
}

// ── POST /api/upvote/:id ──────────────────────────────────────────────────

async fn upvote(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if !state.is_authed(&headers) {
        return (axum::http::StatusCode::UNAUTHORIZED, Json(json!({"error":"Unauthorized"}))).into_response();
    }
    let voter = body.get("agent").or_else(|| body.get("voter"))
        .and_then(|v| v.as_str())
        .unwrap_or("anonymous")
        .to_string();

    let mut queue = state.queue.write().await;
    let item = queue.items.iter_mut().find(|i| i.get("id").and_then(|v| v.as_str()) == Some(&id));
    match item {
        None => {
            drop(queue);
            (axum::http::StatusCode::NOT_FOUND, Json(json!({"error":"Item not found"}))).into_response()
        }
        Some(item) => {
            let votes = item.as_object_mut().unwrap()
                .entry("votes")
                .or_insert(json!([]))
                .as_array_mut()
                .unwrap();
            if !votes.iter().any(|v| v.as_str() == Some(&voter)) {
                votes.push(json!(voter));
            }
            let updated = item.clone();
            drop(queue);
            crate::state::flush_queue(&state).await;
            (axum::http::StatusCode::OK, Json(json!({"ok": true, "item": updated}))).into_response()
        }
    }
}
