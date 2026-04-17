//! Minimal S3/MinIO client — reqwest + AWS Signature V4.
//! Uses only crates already in Cargo.toml (reqwest, hmac, sha2, hex, chrono).
//! Replaces aws-sdk-s3 to avoid OOM on low-RAM build hosts.

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub struct MinioClient {
    endpoint: String,
    access_key: String,
    secret_key: String,
    region: String,
    http: Arc<reqwest::Client>,
}

pub struct S3Object {
    pub key: String,
    pub size: i64,
    pub last_modified: String,
}

#[derive(Debug)]
pub struct S3Error(pub String);

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl MinioClient {
    pub fn new(endpoint: &str, access_key: &str, secret_key: &str, region: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            region: region.to_string(),
            http: Arc::new(
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()
                    .unwrap_or_default(),
            ),
        }
    }

    pub fn is_no_such_key(err: &S3Error) -> bool {
        err.0 == "NoSuchKey"
    }

    pub async fn get_object(&self, bucket: &str, key: &str) -> Result<(Vec<u8>, String), S3Error> {
        let path = format!("/{}/{}", bucket, percent_encode_path(key));
        let url = format!("{}{}", self.endpoint, path);
        let signed = self.sign("GET", &path, "", &[]);

        let resp = self
            .http
            .get(&url)
            .header("Authorization", &signed.authorization)
            .header("x-amz-date", &signed.amz_date)
            .header("x-amz-content-sha256", &signed.content_sha256)
            .send()
            .await
            .map_err(|e| S3Error(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(S3Error("NoSuchKey".to_string()));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(S3Error(format!("HTTP {}: {}", status, body)));
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = resp.bytes().await.map_err(|e| S3Error(e.to_string()))?.to_vec();
        Ok((bytes, content_type))
    }

    pub async fn put_object(&self, bucket: &str, key: &str, body: Vec<u8>) -> Result<(), S3Error> {
        let path = format!("/{}/{}", bucket, percent_encode_path(key));
        let url = format!("{}{}", self.endpoint, path);
        let signed = self.sign("PUT", &path, "", &body);

        let resp = self
            .http
            .put(&url)
            .header("Authorization", &signed.authorization)
            .header("x-amz-date", &signed.amz_date)
            .header("x-amz-content-sha256", &signed.content_sha256)
            .header("content-type", "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| S3Error(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(S3Error(format!("HTTP {}: {}", status, body)));
        }
        Ok(())
    }

    pub async fn list_objects_v2(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> Result<Vec<S3Object>, S3Error> {
        let path = format!("/{}", bucket);
        let query = if prefix.is_empty() {
            "list-type=2".to_string()
        } else {
            format!("list-type=2&prefix={}", percent_encode_value(prefix))
        };
        let url = format!("{}{}?{}", self.endpoint, path, query);
        let signed = self.sign("GET", &path, &query, &[]);

        let resp = self
            .http
            .get(&url)
            .header("Authorization", &signed.authorization)
            .header("x-amz-date", &signed.amz_date)
            .header("x-amz-content-sha256", &signed.content_sha256)
            .send()
            .await
            .map_err(|e| S3Error(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(S3Error(format!("HTTP {}: {}", status, body)));
        }
        let xml = resp.text().await.map_err(|e| S3Error(e.to_string()))?;
        Ok(parse_list_response(&xml))
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<(), S3Error> {
        let path = format!("/{}/{}", bucket, percent_encode_path(key));
        let url = format!("{}{}", self.endpoint, path);
        let signed = self.sign("DELETE", &path, "", &[]);

        let resp = self
            .http
            .delete(&url)
            .header("Authorization", &signed.authorization)
            .header("x-amz-date", &signed.amz_date)
            .header("x-amz-content-sha256", &signed.content_sha256)
            .send()
            .await
            .map_err(|e| S3Error(e.to_string()))?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(S3Error(format!("HTTP {}: {}", status, body)));
        }
        Ok(())
    }

    pub async fn head_object(&self, bucket: &str, key: &str) -> Result<bool, S3Error> {
        let path = format!("/{}/{}", bucket, percent_encode_path(key));
        let url = format!("{}{}", self.endpoint, path);
        let signed = self.sign("HEAD", &path, "", &[]);

        let resp = self
            .http
            .head(&url)
            .header("Authorization", &signed.authorization)
            .header("x-amz-date", &signed.amz_date)
            .header("x-amz-content-sha256", &signed.content_sha256)
            .send()
            .await
            .map_err(|e| S3Error(e.to_string()))?;

        Ok(resp.status().is_success())
    }

    fn sign(&self, method: &str, path: &str, query: &str, body: &[u8]) -> SignedHeaders {
        let now = Utc::now();
        let date_stamp = now.format("%Y%m%d").to_string();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        // Extract host:port from endpoint URL (strip scheme)
        let host = self
            .endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string();

        let body_hash = sha256_hex(body);

        let canonical_headers = format!(
            "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
            host, body_hash, amz_date
        );
        let signed_headers = "host;x-amz-content-sha256;x-amz-date";

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method,
            path,   // already percent-encoded by callers
            query,  // already sorted/encoded by callers
            canonical_headers,
            signed_headers,
            body_hash,
        );

        let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            credential_scope,
            sha256_hex(canonical_request.as_bytes())
        );

        let k_date = hmac_sha256(
            format!("AWS4{}", self.secret_key).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
            self.access_key, credential_scope, signed_headers, signature
        );

        SignedHeaders {
            authorization,
            amz_date,
            content_sha256: body_hash,
        }
    }
}

struct SignedHeaders {
    authorization: String,
    amz_date: String,
    content_sha256: String,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Percent-encode a path, preserving '/' separators.
fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// Percent-encode a query-string value (encodes '/' as %2F).
fn percent_encode_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

fn parse_list_response(xml: &str) -> Vec<S3Object> {
    let mut objects = vec![];
    let mut remaining = xml;
    while let Some(start) = remaining.find("<Contents>") {
        remaining = &remaining[start + "<Contents>".len()..];
        let Some(end) = remaining.find("</Contents>") else {
            break;
        };
        let block = &remaining[..end];
        let key = extract_xml_field(block, "Key").unwrap_or_default().to_string();
        let size = extract_xml_field(block, "Size")
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let last_modified = extract_xml_field(block, "LastModified")
            .unwrap_or_default()
            .to_string();
        if !key.is_empty() {
            objects.push(S3Object {
                key,
                size,
                last_modified,
            });
        }
        remaining = &remaining[end + "</Contents>".len()..];
    }
    objects
}

fn extract_xml_field<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim())
}
