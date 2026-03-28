use leptos::prelude::*;
use crate::types::QueueItem;

#[component]
pub fn MetricsPanel(queue: RwSignal<Vec<QueueItem>>) -> impl IntoView {
    view! {
        <div class="metrics-grid">
            <MetricCard
                label="Total Items"
                value=Signal::derive(move || queue.get().len().to_string())
                color="#58a6ff"
            />
            <MetricCard
                label="In Progress"
                value=Signal::derive(move || {
                    queue.get().iter().filter(|i| i.status == "in-progress").count().to_string()
                })
                color="#3fb950"
            />
            <MetricCard
                label="Blocked"
                value=Signal::derive(move || {
                    queue.get().iter().filter(|i| i.status == "blocked").count().to_string()
                })
                color="#f85149"
            />
            <MetricCard
                label="Awaiting jkh"
                value=Signal::derive(move || {
                    queue.get().iter()
                        .filter(|i| i.needs_human || i.status == "awaiting-jkh")
                        .count().to_string()
                })
                color="#ffd700"
            />
            <MetricCard
                label="Bugs"
                value=Signal::derive(move || {
                    queue.get().iter()
                        .filter(|i| i.card_type() == "bug")
                        .count().to_string()
                })
                color="#f85149"
            />
            <MetricCard
                label="Ideas"
                value=Signal::derive(move || {
                    queue.get().iter()
                        .filter(|i| i.card_type() == "idea")
                        .count().to_string()
                })
                color="#d29922"
            />
        </div>
    }
}

#[component]
fn MetricCard(
    label: &'static str,
    value: Signal<String>,
    color: &'static str,
) -> impl IntoView {
    view! {
        <div class="metric-card">
            <div class="metric-value" style=format!("color: {}", color)>
                {move || value.get()}
            </div>
            <div class="metric-label">{label}</div>
        </div>
    }
}
