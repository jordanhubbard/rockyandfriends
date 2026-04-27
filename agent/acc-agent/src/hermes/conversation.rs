use serde_json::{json, Value};

/// In-memory conversation history for the Anthropic messages API.
/// Messages are stored as raw JSON to match the API format directly.
pub struct ConversationHistory {
    pub messages: Vec<Value>,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl ConversationHistory {
    pub fn new() -> Self {
        Self { messages: Vec::new(), input_tokens: 0, output_tokens: 0 }
    }

    pub fn push_user_text(&mut self, text: &str) {
        self.messages.push(json!({
            "role": "user",
            "content": [{"type": "text", "text": text}]
        }));
    }

    pub fn push_assistant_content(&mut self, content: Vec<Value>) {
        self.messages.push(json!({
            "role": "assistant",
            "content": content
        }));
    }

    pub fn push_tool_results(&mut self, results: Vec<Value>) {
        self.messages.push(json!({
            "role": "user",
            "content": results
        }));
    }

    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}
