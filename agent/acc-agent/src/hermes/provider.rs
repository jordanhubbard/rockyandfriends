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

/// OpenAI-compatible provider — works with Ollama, OpenRouter, any /v1/chat/completions endpoint.
/// Set HERMES_PROVIDER=openai (or any non-empty OPENAI_BASE_URL) to activate.
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .expect("failed to build reqwest client for OpenAiProvider");
        Self {
            api_key,
            model,
            client,
            base_url: std::env::var("OPENAI_BASE_URL")
                .or_else(|_| std::env::var("HERMES_BACKEND_URL"))
                .unwrap_or_else(|_| "https://api.openai.com".to_string()),
        }
    }
}

impl LlmProvider for OpenAiProvider {
    fn complete<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [Value],
        max_tokens: u32,
    ) -> Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>> {
        Box::pin(async move {
            // Translate Anthropic messages → OpenAI chat format
            let mut oai_messages = vec![json!({"role": "system", "content": system})];
            for msg in messages {
                let role = msg["role"].as_str().unwrap_or("user");
                let content = &msg["content"];
                // Content may be array (Anthropic) or string (simple)
                let oai_content: Value = if content.is_array() {
                    let parts = content.as_array().unwrap();
                    // Collect text blocks; tool_use becomes tool_calls on assistant, tool_result becomes tool on user
                    let text_parts: Vec<&Value> = parts.iter().filter(|p| p["type"] == "text").collect();
                    if text_parts.len() == 1 {
                        text_parts[0]["text"].clone()
                    } else if text_parts.is_empty() {
                        // Handle tool results or tool_use — pass raw for now
                        json!(serde_json::to_string(content).unwrap_or_default())
                    } else {
                        json!(text_parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"))
                    }
                } else {
                    content.clone()
                };
                oai_messages.push(json!({"role": role, "content": oai_content}));
            }

            // Translate Anthropic tool format → OpenAI function format
            let oai_tools: Vec<Value> = tools.iter().map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t["name"],
                        "description": t["description"],
                        "parameters": t["input_schema"]
                    }
                })
            }).collect();

            let mut body = json!({
                "model": self.model,
                "max_tokens": max_tokens,
                "messages": oai_messages,
            });
            if !oai_tools.is_empty() {
                body["tools"] = json!(oai_tools);
            }

            let resp = self
                .client
                .post(format!("{}/v1/chat/completions", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
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

            let choice = &val["choices"][0];
            let msg = &choice["message"];
            let stop_reason = choice["finish_reason"].as_str().unwrap_or("stop").to_string();

            // Translate back to Anthropic content format
            let mut content_blocks: Vec<Value> = Vec::new();

            if let Some(text) = msg["content"].as_str() {
                if !text.is_empty() {
                    content_blocks.push(json!({"type": "text", "text": text}));
                }
            }

            // Translate tool_calls → Anthropic tool_use blocks
            if let Some(tool_calls) = msg["tool_calls"].as_array() {
                for tc in tool_calls {
                    let fn_name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                    let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                    content_blocks.push(json!({
                        "type": "tool_use",
                        "id": tc["id"].as_str().unwrap_or("call-0"),
                        "name": fn_name,
                        "input": args
                    }));
                }
            }

            // Normalize stop_reason to Anthropic format
            let anthropic_stop = match stop_reason.as_str() {
                "tool_calls" => "tool_use",
                "length" => "max_tokens",
                "stop" | "" => "end_turn",
                other => other,
            }.to_string();

            let input_tokens = val["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32;
            let output_tokens = val["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32;

            Ok(LlmResponse {
                content: content_blocks,
                stop_reason: anthropic_stop,
                input_tokens,
                output_tokens,
            })
        })
    }
}

/// Select provider based on environment variables.
/// - HERMES_PROVIDER=openai → OpenAiProvider (also triggered by OPENAI_BASE_URL or HERMES_BACKEND_URL)
/// - default → AnthropicProvider
pub fn make_provider(api_key: String, model: String) -> Box<dyn LlmProvider> {
    let use_openai = std::env::var("HERMES_PROVIDER").as_deref() == Ok("openai")
        || std::env::var("OPENAI_BASE_URL").is_ok()
        || std::env::var("HERMES_BACKEND_URL").is_ok();
    if use_openai {
        let oai_key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .unwrap_or(api_key);
        Box::new(OpenAiProvider::new(oai_key, model))
    } else {
        Box::new(AnthropicProvider::new(api_key, model))
    }
}
