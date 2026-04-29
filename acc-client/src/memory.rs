//! Memory endpoints: semantic search and ingest.

use crate::{Client, Error, Result};
use acc_model::{MemoryHit, MemorySearchRequest, MemoryStoreRequest};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct MemoryApi<'a> {
    pub(crate) client: &'a Client,
}

impl<'a> MemoryApi<'a> {
    /// Search semantic memory via `GET /api/memory/recall`.
    ///
    /// If a collection is specified, use the lower-level vector search route
    /// because the default memory recall endpoint searches the canonical
    /// `acc_memory` collection.
    pub async fn search(self, req: &MemorySearchRequest) -> Result<Vec<MemoryHit>> {
        let collection = req.collection.as_deref().filter(|s| !s.is_empty());
        let mut path = if collection.is_some() {
            format!("/api/vector/search?q={}", query_encode(&req.query))
        } else {
            format!("/api/memory/recall?q={}", query_encode(&req.query))
        };
        if let Some(limit) = req.limit {
            path.push_str("&k=");
            path.push_str(&limit.to_string());
        }
        if let Some(collection) = collection {
            path.push_str("&collections=");
            path.push_str(&query_encode(collection));
        }

        let resp = self
            .client
            .http()
            .get(self.client.url(&path))
            .send()
            .await?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await?;
        if !(200..300).contains(&status) {
            return Err(Error::from_response(status, &bytes));
        }
        let env: SearchEnvelope = serde_json::from_slice(&bytes)?;
        Ok(match env {
            SearchEnvelope::Wrapped { results } => results,
            SearchEnvelope::Hits { hits } => hits,
            SearchEnvelope::Bare(v) => v,
        })
    }

    /// Store memory via `POST /api/memory/ingest`.
    ///
    /// Requests that explicitly name a collection use `POST /api/vector/upsert`
    /// so callers can still address non-default vector collections.
    pub async fn store(self, req: &MemoryStoreRequest) -> Result<()> {
        let collection = req.collection.as_deref().filter(|s| !s.is_empty());
        let (path, body) = if let Some(collection) = collection {
            (
                "/api/vector/upsert",
                json!({
                    "collection": collection,
                    "id": memory_store_id(req),
                    "text": req.text.clone(),
                    "metadata": req.metadata.clone(),
                }),
            )
        } else {
            ("/api/memory/ingest", ingest_body(req))
        };

        let resp = self
            .client
            .http()
            .post(self.client.url(path))
            .json(&body)
            .send()
            .await?;
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let bytes = resp.bytes().await?;
        Err(Error::from_response(status, &bytes))
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SearchEnvelope {
    Wrapped { results: Vec<MemoryHit> },
    Hits { hits: Vec<MemoryHit> },
    Bare(Vec<MemoryHit>),
}

fn ingest_body(req: &MemoryStoreRequest) -> Value {
    let mut body = Map::new();
    body.insert("text".to_string(), json!(req.text.clone()));

    if let Some(Value::Object(meta)) = &req.metadata {
        for key in [
            "platform",
            "workspace_id",
            "channel_id",
            "user_id",
            "conv_id",
            "agent",
            "source",
            "source_type",
        ] {
            if let Some(value) = meta.get(key) {
                body.insert(key.to_string(), value.clone());
            }
        }
    }

    Value::Object(body)
}

fn memory_store_id(req: &MemoryStoreRequest) -> String {
    let mut hash = Sha256::new();
    hash.update(req.text.as_bytes());
    if let Some(metadata) = &req.metadata {
        hash.update(b"\0");
        hash.update(metadata.to_string().as_bytes());
    }
    hex::encode(hash.finalize())
}

fn query_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
