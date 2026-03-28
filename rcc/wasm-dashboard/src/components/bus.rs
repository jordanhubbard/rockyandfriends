use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{EventSource, MessageEvent};
use gloo_timers::callback::Interval;
use crate::types::BusMessage;
use crate::api;

// ── Bus tab ───────────────────────────────────────────────────────────────────

#[component]
pub fn BusTab() -> impl IntoView {
    let messages:   RwSignal<Vec<BusMessage>> = RwSignal::new(vec![]);
    let search_q:   RwSignal<String>          = RwSignal::new(String::new());
    let filter_from: RwSignal<String>         = RwSignal::new("all".into());
    let send_from:   RwSignal<String>         = RwSignal::new("jkh".into());
    let send_to:     RwSignal<String>         = RwSignal::new("all".into());
    let send_type:   RwSignal<String>         = RwSignal::new("message".into());
    let send_body:   RwSignal<String>         = RwSignal::new(String::new());
    let send_status: RwSignal<Option<String>> = RwSignal::new(None);

    // Initial load
    {
        let msgs = messages;
        spawn_local(async move {
            if let Ok(m) = api::fetch_bus_messages(100).await {
                let mut v = m;
                v.reverse();
                msgs.set(v);
            }
        });
    }

    // Poll bus messages every 5s
    {
        let msgs = messages;
        let _iv = Interval::new(5_000, move || {
            let m = msgs;
            spawn_local(async move {
                if let Ok(mut v) = api::fetch_bus_messages(100).await {
                    v.reverse();
                    m.set(v);
                }
            });
        });
        _iv.forget();
    }

    // SSE connection — append new messages live
    {
        let msgs = messages;
        if let Ok(es) = EventSource::new("/bus/stream") {
            let cb = Closure::wrap(Box::new(move |e: MessageEvent| {
                if let Some(text) = e.data().as_string() {
                    if let Ok(msg) = serde_json::from_str::<BusMessage>(&text) {
                        msgs.update(|v| {
                            v.insert(0, msg);
                            if v.len() > 200 { v.truncate(200); }
                        });
                    }
                }
            }) as Box<dyn FnMut(_)>);
            es.set_onmessage(Some(cb.as_ref().unchecked_ref()));
            cb.forget();
            // Keep EventSource alive
            let _ = Box::new(es);
        }
    }

    // Filtered view
    let filtered = move || {
        let q  = search_q.get().to_lowercase();
        let ff = filter_from.get();
        messages.get()
            .into_iter()
            .filter(|m| {
                let from = m.from.as_deref().unwrap_or("").to_lowercase();
                let body = m.body.as_deref().unwrap_or("").to_lowercase();
                (ff == "all" || from == ff.to_lowercase())
                    && (q.is_empty() || body.contains(&q) || from.contains(&q))
            })
            .collect::<Vec<_>>()
    };

    // Send handler
    let on_send = {
        let sf = send_from;
        let st = send_to;
        let sty = send_type;
        let sb = send_body;
        let ss = send_status;
        move |_| {
            let from  = sf.get();
            let to    = st.get();
            let stype = sty.get();
            let body  = sb.get();
            let sb2   = sb;
            let ss2   = ss;
            if body.trim().is_empty() { return; }
            spawn_local(async move {
                match api::send_bus_message(&from, &to, &stype, &body).await {
                    Ok(()) => {
                        sb2.set(String::new());
                        ss2.set(Some("✅ Sent".to_string()));
                    }
                    Err(e) => {
                        ss2.set(Some(format!("❌ {}", e)));
                    }
                }
            });
        }
    };

    view! {
        <div class="bus-layout">
            // Filters
            <div class="bus-filters">
                <input
                    type="text"
                    placeholder="Search messages…"
                    on:input=move |e| {
                        search_q.set(event_target_value(&e));
                    }
                />
                <select on:change=move |e| {
                    filter_from.set(event_target_value(&e));
                }>
                    <option value="all">"All agents"</option>
                    <option value="rocky">"Rocky"</option>
                    <option value="bullwinkle">"Bullwinkle"</option>
                    <option value="natasha">"Natasha"</option>
                    <option value="boris">"Boris"</option>
                    <option value="jkh">"jkh"</option>
                </select>
                <span style="font-size:11px;color:var(--text-dim)">
                    {move || format!("{} messages", filtered().len())}
                </span>
            </div>

            // Stream
            <div class="bus-stream">
                {move || {
                    let items = filtered();
                    if items.is_empty() {
                        view! {
                            <div class="loading">"No messages"</div>
                        }.into_any()
                    } else {
                        items.into_iter().map(|msg| {
                            view! { <BusEntry msg=msg /> }
                        }).collect_view().into_any()
                    }
                }}
            </div>

            // Send widget
            <div class="bus-send">
                <div class="bus-send-row">
                    <span style="font-size:12px;color:var(--text-muted)">"From:"</span>
                    <select on:change=move |e| { send_from.set(event_target_value(&e)); }>
                        <option value="jkh">"jkh"</option>
                        <option value="rocky">"Rocky"</option>
                        <option value="bullwinkle">"Bullwinkle"</option>
                        <option value="natasha">"Natasha"</option>
                        <option value="boris">"Boris"</option>
                    </select>
                    <span style="font-size:12px;color:var(--text-muted)">"To:"</span>
                    <select on:change=move |e| { send_to.set(event_target_value(&e)); }>
                        <option value="all">"all"</option>
                        <option value="rocky">"Rocky"</option>
                        <option value="bullwinkle">"Bullwinkle"</option>
                        <option value="natasha">"Natasha"</option>
                        <option value="boris">"Boris"</option>
                    </select>
                    <span style="font-size:12px;color:var(--text-muted)">"Type:"</span>
                    <select on:change=move |e| { send_type.set(event_target_value(&e)); }>
                        <option value="message">"message"</option>
                        <option value="command">"command"</option>
                        <option value="alert">"alert"</option>
                        <option value="lesson">"lesson"</option>
                    </select>
                </div>
                <div class="bus-send-row">
                    <textarea
                        placeholder="Message body…"
                        prop:value=move || send_body.get()
                        on:input=move |e| { send_body.set(event_target_value(&e)); }
                    />
                    <button on:click=on_send>"Send"</button>
                </div>
                {move || send_status.get().map(|s| view! {
                    <div style="font-size:11px;color:var(--text-muted)">{s}</div>
                })}
            </div>
        </div>
    }
}

// ── Single message row ────────────────────────────────────────────────────────

#[component]
fn BusEntry(msg: BusMessage) -> impl IntoView {
    let ts   = msg.ts.as_deref().unwrap_or("").to_string();
    let from = msg.from.as_deref().unwrap_or("?").to_string();
    let to   = msg.to.as_deref().unwrap_or("?").to_string();
    let typ  = msg.msg_type.as_deref().unwrap_or("").to_string();
    let body = msg.body.as_deref().unwrap_or("").to_string();

    // Format timestamp: take last 19 chars (HH:MM:SS) from ISO string
    let ts_short = if ts.len() >= 19 {
        ts[11..19].to_string()
    } else {
        ts.clone()
    };

    view! {
        <div class="bus-entry">
            <span class="bus-ts">{ts_short}</span>
            <span class="bus-from">{from}</span>
            <span style="color:var(--text-dim)">"→"</span>
            <span class="bus-to">{to}</span>
            {if !typ.is_empty() {
                view! { <span class="bus-type">{format!("[{}]", typ)}</span> }.into_any()
            } else {
                view! { <span /> }.into_any()
            }}
            <span class="bus-body">{body}</span>
        </div>
    }
}
