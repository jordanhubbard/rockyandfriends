use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::types::QueueItem;
use crate::api;

#[component]
pub fn AppealQueue(queue: RwSignal<Vec<QueueItem>>) -> impl IntoView {
    let appeal_items = move || {
        queue.get()
            .into_iter()
            .filter(|item| {
                item.needs_human
                    || item.status == "awaiting-jkh"
                    || item.tags.iter().any(|t| t == "appeal" || t == "needs-human")
            })
            .collect::<Vec<_>>()
    };

    view! {
        <div class="appeal-list">
            {move || {
                let items = appeal_items();
                if items.is_empty() {
                    view! {
                        <div class="card">
                            <span style="color: var(--text-dim); font-size: 13px;">"✅ No pending appeals"</span>
                        </div>
                    }.into_any()
                } else {
                    items.into_iter().map(|item| {
                        view! { <AppealCard item=item queue=queue /> }
                    }).collect_view().into_any()
                }
            }}
        </div>
    }
}

#[component]
fn AppealCard(item: QueueItem, queue: RwSignal<Vec<QueueItem>>) -> impl IntoView {
    let heat_class = heat_class_for(&item);
    let id = item.id.clone();
    let title = item.title.clone();
    let assignee = item.assignee.clone().unwrap_or_else(|| "unassigned".to_string());
    let reason = item.needs_human_reason.clone().unwrap_or_default();
    let age = waiting_age(&item);

    let id_approve = id.clone();
    let id_reject  = id.clone();
    let queue_approve = queue;
    let queue_reject  = queue;

    let on_approve = move |_| {
        let id = id_approve.clone();
        let q = queue_approve;
        spawn_local(async move {
            let patch = serde_json::json!({ "status": "approved", "needsHuman": false });
            if api::patch_item(&id, patch).await.is_ok() {
                // Refresh queue signal after patch
                if let Ok(items) = api::fetch_queue().await {
                    q.set(items);
                }
            }
        });
    };

    let on_reject = move |_| {
        let id = id_reject.clone();
        let q = queue_reject;
        spawn_local(async move {
            let patch = serde_json::json!({ "status": "rejected", "needsHuman": false });
            if api::patch_item(&id, patch).await.is_ok() {
                if let Ok(items) = api::fetch_queue().await {
                    q.set(items);
                }
            }
        });
    };

    view! {
        <div class=format!("appeal-item {}", heat_class)>
            <div class="appeal-header">
                <span class="appeal-title">{title}</span>
                {if heat_class.contains("alarm") {
                    view! { <span>"⚠️"</span> }.into_any()
                } else {
                    view! { <span /> }.into_any()
                }}
                <span class="appeal-age">{age}</span>
            </div>
            <div class="appeal-meta">
                {format!("Flagged by: {} · Status: {}", assignee, item.status)}
            </div>
            {if !reason.is_empty() {
                view! { <div class="appeal-reason">{reason}</div> }.into_any()
            } else {
                view! { <span /> }.into_any()
            }}
            <div class="appeal-actions">
                <button on:click=on_approve>"✅ Approve"</button>
                <button on:click=on_reject>"❌ Reject"</button>
                <button>"💬 Comment"</button>
                <button>"🔀 Reassign"</button>
            </div>
        </div>
    }
}

/// Heat class based on how long an item has been waiting.
fn heat_class_for(item: &QueueItem) -> &'static str {
    let ts = item.updated_at.as_deref()
        .or(item.created_at.as_deref())
        .unwrap_or("");
    if ts.is_empty() {
        return "";
    }
    let ms = js_sys::Date::parse(ts);
    if ms.is_nan() {
        return "";
    }
    let age_hours = (js_sys::Date::now() - ms) / 3_600_000.0;
    if age_hours > 72.0 {
        "heat-red heat-alarm"
    } else if age_hours > 24.0 {
        "heat-red"
    } else if age_hours > 2.0 {
        "heat-amber"
    } else {
        ""
    }
}

fn waiting_age(item: &QueueItem) -> String {
    let ts = item.updated_at.as_deref()
        .or(item.created_at.as_deref())
        .unwrap_or("");
    if ts.is_empty() {
        return "unknown".to_string();
    }
    let ms = js_sys::Date::parse(ts);
    if ms.is_nan() {
        return "unknown".to_string();
    }
    let secs = (js_sys::Date::now() - ms) / 1000.0;
    if secs < 3600.0 {
        format!("{}m", (secs / 60.0) as u64)
    } else if secs < 86400.0 {
        format!("{}h", (secs / 3600.0) as u64)
    } else {
        format!("{}d", (secs / 86400.0) as u64)
    }
}
