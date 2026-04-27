mod session;
mod slack;
mod telegram;

use std::sync::Arc;
use session::SessionStore;
use crate::config::Config;
use super::agent::HermesAgent;
use super::provider::make_provider;
use super::tool::ToolRegistry;
use acc_client::Client;

pub async fn run() {
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[hermes-gateway] config error: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("[hermes-gateway] starting agent={} hub={}", cfg.agent_name, cfg.acc_url);

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .or_else(|_| std::env::var("OPENAI_API_KEY"))
        .unwrap_or_default();
    let model = std::env::var("HERMES_MODEL")
        .unwrap_or_else(|_| "claude-opus-4-7".to_string());
    let provider = make_provider(api_key, model);
    let tools = ToolRegistry::default_tools();
    let client = Client::new(&cfg.acc_url, &cfg.acc_token).expect("acc client");
    let agent = Arc::new(HermesAgent::new(cfg.clone(), client, provider, tools));

    let sessions_dir = cfg.acc_dir.join("data").join("sessions");
    let sessions = Arc::new(SessionStore::new(sessions_dir));

    let mut handles = Vec::new();

    // Start Slack if configured.
    match slack::SlackAdapter::new(sessions.clone(), agent.clone()).await {
        Some(adapter) => {
            eprintln!("[hermes-gateway] Slack adapter started");
            let adapter = Arc::new(adapter);
            handles.push(tokio::spawn(async move { adapter.run().await }));
        }
        None => eprintln!("[hermes-gateway] Slack not configured (SLACK_APP_TOKEN / SLACK_BOT_TOKEN missing)"),
    }

    // Start Telegram if configured.
    match telegram::TelegramAdapter::new(sessions.clone(), agent.clone()).await {
        Some(adapter) => {
            eprintln!("[hermes-gateway] Telegram adapter started");
            let adapter = Arc::new(adapter);
            handles.push(tokio::spawn(async move { adapter.run().await }));
        }
        None => eprintln!("[hermes-gateway] Telegram not configured (TELEGRAM_BOT_TOKEN missing)"),
    }

    if handles.is_empty() {
        eprintln!("[hermes-gateway] no platforms configured — exiting");
        std::process::exit(1);
    }

    // Wait for all adapters (they run forever unless they panic).
    for h in handles {
        let _ = h.await;
    }
}
