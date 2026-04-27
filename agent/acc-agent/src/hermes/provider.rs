use std::future::Future;
use std::pin::Pin;
use serde_json::{json, Value};

#[derive(Debug)]
pub struct LlmResponse {
    pub content: Vec<Value>,
    pub stop_reason: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

pub type ProviderResult = Result<LlmResponse, String>;

/// Object-safe LLM provider trait. Uses boxed futures so Box<dyn LlmProvider> works.
pub trait LlmProvider: Send + Sync {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [Value],
        max_tokens: u32,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>>;
}

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to build reqwest client for AnthropicProvider");
        Self {
            api_key,
            model,
            client,
            base_url: std::env::var("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
        }
    }
}

impl LlmProvider for AnthropicProvider {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [Value],
        max_tokens: u32,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            let mut body = json!({
                "model": self.model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": messages,
            });
            if !tools.is_empty() {
                body["tools"] = json!(tools);
            }

            let resp = self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("HTTP error: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(format!("API error {status}: {}", &text[..text.len().min(500)]));
            }

            let val: Value = resp
                .json()
                .await
                .map_err(|e| format!("JSON parse error: {e}"))?;

            let content = val["content"].as_array().cloned().unwrap_or_default();
            let stop_reason = val["stop_reason"]
                .as_str()
                .unwrap_or("end_turn")
                .to_string();
            let input_tokens =
                val["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32;
            let output_tokens =
                val["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

            Ok(LlmResponse {
                content,
                stop_reason,
                input_tokens,
                output_tokens,
            })
        })
    }
}
