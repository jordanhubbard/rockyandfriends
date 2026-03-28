use leptos::prelude::*;
use crate::types::{HeartbeatMap, agent_emoji, agent_color};

const AGENTS: &[&str] = &["rocky", "bullwinkle", "natasha", "boris"];

#[component]
pub fn AgentStrip(heartbeats: RwSignal<HeartbeatMap>) -> impl IntoView {
    view! {
        <div class="agent-strip">
            {AGENTS.iter().map(|&agent| {
                view! { <AgentCard agent=agent heartbeats=heartbeats /> }
            }).collect_view()}
        </div>
    }
}

#[component]
fn AgentCard(agent: &'static str, heartbeats: RwSignal<HeartbeatMap>) -> impl IntoView {
    let color = agent_color(agent);
    let emoji = agent_emoji(agent);

    view! {
        <div class="agent-card" style=format!("border-left: 3px solid {}", color)>
            {move || {
                let hbs = heartbeats.get();
                let hb = hbs.get(agent);
                let (dot_class, age_label, activity, host) = match hb {
                    None => ("offline", "never".to_string(), "unknown".to_string(), "".to_string()),
                    Some(h) => (
                        h.status_class(),
                        h.age_label(),
                        h.activity.clone().unwrap_or_else(|| "idle".to_string()),
                        h.host.clone().unwrap_or_default(),
                    ),
                };
                view! {
                    <div class=format!("agent-dot {}", dot_class) />
                    <div class="agent-info">
                        <div class="agent-name" style=format!("color:{}", color)>
                            {format!("{} {}", emoji, capitalize(agent))}
                        </div>
                        <div class="agent-host">{host}</div>
                        <div class="agent-activity">{activity}</div>
                    </div>
                    <div class="agent-age">{age_label}</div>
                }
            }}
        </div>
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None    => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
