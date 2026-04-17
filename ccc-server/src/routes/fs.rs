/// /api/fs/* — S3-backed AgentFS API
///
/// MinIO endpoint from MINIO_ENDPOINT env (default http://localhost:9000).
/// Bucket from MINIO_BUCKET env (default "agents").
use crate::s3::MinioClient;
use crate::AppState;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, head, post},
    Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/fs/read", get(fs_read))
        .route("/api/fs/write", post(fs_write))
        .route("/api/fs/list", get(fs_list))
        .route("/api/fs/delete", delete(fs_delete))
        .route("/api/fs/exists", head(fs_exists))
}

// ── Path validation ───────────────────────────────────────────────────────────

fn validate_path(path: &str, agent: Option<&str>) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("path required");
    }
    if path.contains("..") {
        return Err("path traversal not allowed");
    }
    if path.starts_with("shared/") {
        return Ok(());
    }
    if let Some(agent) = agent {
        let prefix = format!("{}/", agent);
        if path.starts_with(&prefix) {
            return Ok(());
        }
    }
    Err("path must start with 'shared/' or '{agent}/'")
}

fn s3_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "S3 not configured"})),
    )
        .into_response()
}

fn client(state: &AppState) -> Option<&Arc<MinioClient>> {
    state.s3_client.as_ref()
}

// ── GET /api/fs/read?path=...&agent=... ───────────────────────────────────────

#[derive(Deserialize)]
struct ReadQuery {
    path: String,
    #[serde(default)]
    agent: Option<String>,
}

async fn fs_read(State(state): State<Arc<AppState>>, Query(params): Query<ReadQuery>) -> Response {
    let Some(s3) = client(&state) else {
        return s3_unavailable();
    };

    if params.path.contains("..") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path traversal not allowed"})),
        )
            .into_response();
    }
    if params.path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path required"})),
        )
            .into_response();
    }

    match s3.get_object(&state.s3_bucket, &params.path).await {
        Ok((bytes, content_type)) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, content_type)],
            bytes,
        )
            .into_response(),
        Err(e) if MinioClient::is_no_such_key(&e) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── POST /api/fs/write ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct WriteBody {
    path: String,
    content: String,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

async fn fs_write(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WriteBody>,
) -> impl IntoResponse {
    let Some(s3) = client(&state) else {
        return s3_unavailable();
    };

    if let Err(e) = validate_path(&body.path, body.agent.as_deref()) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response();
    }

    let _scope = body.scope.as_deref().unwrap_or("private");
    let content_bytes = body.content.into_bytes();
    let size = content_bytes.len();

    match s3.put_object(&state.s3_bucket, &body.path, content_bytes).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({"ok": true, "path": body.path, "size": size})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── GET /api/fs/list?prefix=...&agent=... ─────────────────────────────────────

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    agent: Option<String>,
}

async fn fs_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListQuery>,
) -> impl IntoResponse {
    let Some(s3) = client(&state) else {
        return s3_unavailable();
    };

    let prefix = params.prefix.unwrap_or_default();

    match s3.list_objects_v2(&state.s3_bucket, &prefix).await {
        Ok(objects) => {
            let items: Vec<serde_json::Value> = objects
                .iter()
                .map(|obj| {
                    json!({
                        "key": obj.key,
                        "size": obj.size,
                        "lastModified": obj.last_modified,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"ok": true, "objects": items}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── DELETE /api/fs/delete?path=... ────────────────────────────────────────────

#[derive(Deserialize)]
struct DeleteQuery {
    path: String,
}

async fn fs_delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<DeleteQuery>,
) -> impl IntoResponse {
    if !state.is_authed(&headers) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Unauthorized"})),
        )
            .into_response();
    }

    let Some(s3) = client(&state) else {
        return s3_unavailable();
    };

    if params.path.contains("..") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path traversal not allowed"})),
        )
            .into_response();
    }
    if params.path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "path required"})),
        )
            .into_response();
    }

    match s3.delete_object(&state.s3_bucket, &params.path).await {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok": false, "error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── HEAD /api/fs/exists?path=... ──────────────────────────────────────────────

#[derive(Deserialize)]
struct ExistsQuery {
    path: String,
}

async fn fs_exists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExistsQuery>,
) -> StatusCode {
    let Some(s3) = client(&state) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    if params.path.contains("..") || params.path.is_empty() {
        return StatusCode::BAD_REQUEST;
    }

    match s3.head_object(&state.s3_bucket, &params.path).await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
