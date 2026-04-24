//! acc — ACC fleet CLI
//!
//! Config (lowest to highest priority):
//!   ~/.acc/.env       ACC_HUB_URL, ACC_TOKEN / ACC_AGENT_TOKEN
//!   environment vars  ACC_HUB_URL, ACC_TOKEN
//!   --hub / --token flags
//!
//! Examples:
//!   acc tasks list --status=open
//!   acc tasks create --title "Fix bug" --project proj-abc
//!   acc tasks complete task-123 --output "done"
//!   acc agents list --online
//!   acc bus tail --filter github
//!   acc bus tail --raw

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "acc",
    about = "ACC fleet CLI — tasks, projects, agents, bus",
    version
)]
struct Cli {
    /// Hub base URL
    #[arg(long, env = "ACC_HUB_URL", default_value = "http://localhost:8789")]
    hub: String,

    /// API bearer token (falls back to ACC_TOKEN env, then ~/.acc/.env)
    #[arg(long, env = "ACC_TOKEN")]
    token: Option<String>,

    /// Emit raw JSON instead of formatted output
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fleet task operations
    Tasks {
        #[command(subcommand)]
        sub: TaskCmd,
    },
    /// Project operations
    Projects {
        #[command(subcommand)]
        sub: ProjectCmd,
    },
    /// Agent inspection
    Agents {
        #[command(subcommand)]
        sub: AgentCmd,
    },
    /// Message bus
    Bus {
        #[command(subcommand)]
        sub: BusCmd,
    },
    /// Work queue
    Queue {
        #[command(subcommand)]
        sub: QueueCmd,
    },
}

// ── tasks subcommands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum TaskCmd {
    /// List tasks
    List {
        #[arg(long)] status: Option<String>,
        #[arg(long)] project: Option<String>,
        #[arg(long, name = "type")] task_type: Option<String>,
        #[arg(long)] agent: Option<String>,
        #[arg(long, default_value = "50")] limit: u32,
    },
    /// Show a task
    Get { id: String },
    /// Create a task
    Create {
        #[arg(long)] title: String,
        #[arg(long)] description: Option<String>,
        #[arg(long)] project: Option<String>,
        #[arg(long, default_value = "2")] priority: u8,
        #[arg(long, name = "type", default_value = "work")] task_type: String,
    },
    /// Mark a task complete
    Complete {
        id: String,
        #[arg(long)] output: Option<String>,
    },
    /// Cancel a task
    Cancel { id: String },
    /// Claim a task for an agent
    Claim {
        id: String,
        #[arg(long)] agent: String,
    },
    /// Release a claimed task
    Unclaim { id: String },
}

// ── projects subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
enum ProjectCmd {
    /// List projects
    List {
        #[arg(long)] status: Option<String>,
        #[arg(long)] q: Option<String>,
        #[arg(long, default_value = "50")] limit: u32,
    },
    /// Show a project
    Get { id: String },
    /// Create a project
    Create {
        #[arg(long)] name: String,
        #[arg(long)] description: Option<String>,
        #[arg(long)] repo: Option<String>,
    },
    /// Archive or delete a project
    Delete {
        id: String,
        #[arg(long)] hard: bool,
    },
}

// ── agents subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum AgentCmd {
    /// List agents
    List {
        #[arg(long)] online: bool,
    },
    /// Show an agent
    Get { name: String },
}

// ── bus subcommands ───────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum BusCmd {
    /// Stream the bus live (tail -f style)
    Tail {
        /// Only show messages whose type contains this string
        #[arg(long)]
        filter: Option<String>,
        /// Print raw SSE lines
        #[arg(long)]
        raw: bool,
    },
    /// Send a message to the bus
    Send {
        /// Message type
        #[arg(long, name = "type")] msg_type: String,
        /// Optional JSON body (merged with type)
        #[arg(long)] body: Option<String>,
    },
    /// Query recent messages
    Messages {
        #[arg(long)] msg_type: Option<String>,
        #[arg(long, default_value = "50")] limit: u32,
    },
}

// ── queue subcommands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
enum QueueCmd {
    /// List queue items
    List,
    /// Show a queue item
    Get { id: String },
}

// ── HTTP client ───────────────────────────────────────────────────────────────

struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    fn new(base: &str, token: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(&format!("Bearer {}", token))
            .context("invalid token characters")?;
        headers.insert(AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self { base: base.trim_end_matches('/').to_string(), http })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let resp = self.http.get(self.url(path))
            .send().await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            bail!("HTTP {status}: {}", body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown"));
        }
        Ok(body)
    }

    async fn get_qs(&self, path: &str, qs: &[(&str, &str)]) -> Result<Value> {
        let resp = self.http.get(self.url(path))
            .query(qs)
            .send().await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            bail!("HTTP {status}: {}", body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown"));
        }
        Ok(body)
    }

    async fn post(&self, path: &str, payload: &Value) -> Result<Value> {
        let resp = self.http.post(self.url(path))
            .header(CONTENT_TYPE, "application/json")
            .json(payload)
            .send().await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            bail!("HTTP {status}: {}", body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown"));
        }
        Ok(body)
    }

    async fn put(&self, path: &str, payload: &Value) -> Result<Value> {
        let resp = self.http.put(self.url(path))
            .header(CONTENT_TYPE, "application/json")
            .json(payload)
            .send().await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            bail!("HTTP {status}: {}", body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown"));
        }
        Ok(body)
    }

    async fn delete(&self, path: &str) -> Result<Value> {
        let resp = self.http.delete(self.url(path)).send().await?;
        let status = resp.status();
        let body: Value = resp.json().await?;
        if !status.is_success() {
            bail!("HTTP {status}: {}", body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown"));
        }
        Ok(body)
    }

    /// Raw SSE stream — returns the reqwest::Response with chunked body
    async fn sse_stream(&self, path: &str) -> Result<reqwest::Response> {
        let resp = self.http.get(self.url(path))
            .header("Accept", "text/event-stream")
            .send().await?;
        if !resp.status().is_success() {
            bail!("HTTP {}: could not open SSE stream", resp.status());
        }
        Ok(resp)
    }
}

// ── Token resolution ──────────────────────────────────────────────────────────

fn resolve_token(flag: Option<String>) -> Result<String> {
    // 1. CLI flag / ACC_TOKEN env (clap already merged them into flag)
    if let Some(t) = flag {
        return Ok(t);
    }
    // 2. Read ~/.acc/.env
    let env_path = dirs_or_home().join(".acc").join(".env");
    if env_path.exists() {
        let text = std::fs::read_to_string(&env_path)?;
        // Prefer ACC_TOKEN, then ACC_AGENT_TOKEN
        for key in &["ACC_TOKEN", "ACC_AGENT_TOKEN"] {
            if let Some(val) = parse_dotenv(&text, key) {
                return Ok(val);
            }
        }
    }
    bail!(
        "No API token found. Set ACC_TOKEN env var, pass --token, or add ACC_TOKEN to ~/.acc/.env"
    )
}

fn dirs_or_home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn parse_dotenv(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() { continue; }
        if let Some(rest) = line.strip_prefix(key) {
            if let Some(val) = rest.strip_prefix('=') {
                let val = val.trim_matches('"').trim_matches('\'').trim();
                if !val.is_empty() { return Some(val.to_string()); }
            }
        }
    }
    None
}

// ── Formatting helpers ────────────────────────────────────────────────────────

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

fn n(v: &Value, key: &str) -> String {
    v.get(key).map(|x| x.to_string()).unwrap_or_default()
}

fn print_task(t: &Value) {
    let id    = &s(t, "id")[..s(t, "id").len().min(16)];
    let typ   = format!("{:<12}", s(t, "task_type"));
    let pri   = n(t, "priority");
    let stat  = format!("{:<10}", s(t, "status"));
    let title = s(t, "title");
    let agent = s(t, "claimed_by");
    let agent_part = if agent.is_empty() { String::new() } else { format!(" [{agent}]") };
    println!("{id}  {typ}  p{pri}  {stat}  {title}{agent_part}");
}

fn print_task_detail(t: &Value) {
    println!("id:          {}", s(t, "id"));
    println!("title:       {}", s(t, "title"));
    println!("type:        {}", s(t, "task_type"));
    println!("status:      {}", s(t, "status"));
    println!("priority:    {}", n(t, "priority"));
    println!("project:     {}", s(t, "project_id"));
    println!("phase:       {}", s(t, "phase"));
    if !s(t, "claimed_by").is_empty() {
        println!("claimed_by:  {}", s(t, "claimed_by"));
        println!("expires:     {}", s(t, "claim_expires_at"));
    }
    if !s(t, "completed_by").is_empty() {
        println!("completed:   {} by {}", s(t, "completed_at"), s(t, "completed_by"));
    }
    if !s(t, "review_result").is_empty() {
        println!("review:      {}", s(t, "review_result"));
    }
    let desc = s(t, "description");
    if !desc.is_empty() {
        println!("\n{desc}");
    }
}

fn print_project(p: &Value) {
    let id    = &s(p, "id")[..s(p, "id").len().min(16)];
    let name  = s(p, "name");
    let stat  = format!("{:<10}", s(p, "status"));
    let repo  = s(p, "repo");
    let repo_part = if repo.is_empty() { String::new() } else { format!("  ({repo})") };
    println!("{id}  {stat}  {name}{repo_part}");
}

fn print_agent(a: &Value) {
    let name   = format!("{:<16}", s(a, "name"));
    let raw_node = s(a, "node");
    let node_str = if raw_node.is_empty() { s(a, "host") } else { raw_node };
    let node   = format!("{:<20}", node_str);
    let status = if a.get("online").and_then(|v| v.as_bool()).unwrap_or(false) {
        "online "
    } else {
        "offline"
    };
    println!("{name}  {status}  {node}");
}

fn fallback(a: String, b: String) -> String {
    if a.is_empty() { b } else { a }
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let token = resolve_token(cli.token)?;
    let client = Client::new(&cli.hub, &token)?;
    let json_out = cli.json;

    match cli.command {
        Cmd::Tasks { sub } => run_tasks(&client, sub, json_out).await,
        Cmd::Projects { sub } => run_projects(&client, sub, json_out).await,
        Cmd::Agents { sub } => run_agents(&client, sub, json_out).await,
        Cmd::Bus { sub } => run_bus(&client, sub, json_out).await,
        Cmd::Queue { sub } => run_queue(&client, sub, json_out).await,
    }
}

// ── Tasks ─────────────────────────────────────────────────────────────────────

async fn run_tasks(c: &Client, sub: TaskCmd, json_out: bool) -> Result<()> {
    match sub {
        TaskCmd::List { status, project, task_type, agent, limit } => {
            let mut qs: Vec<(&str, String)> = vec![("limit", limit.to_string())];
            if let Some(s) = &status    { qs.push(("status", s.clone())); }
            if let Some(p) = &project   { qs.push(("project", p.clone())); }
            if let Some(t) = &task_type { qs.push(("task_type", t.clone())); }
            if let Some(a) = &agent     { qs.push(("agent", a.clone())); }

            // reqwest wants &[(&str, &str)] — build it from refs
            let qs_ref: Vec<(&str, &str)> = qs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let resp = c.get_qs("/api/tasks", &qs_ref).await?;
            let tasks = resp["tasks"].as_array().cloned().unwrap_or_default();

            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
                return Ok(());
            }
            if tasks.is_empty() {
                println!("(no tasks)");
                return Ok(());
            }
            for t in &tasks { print_task(t); }
            println!("\n{} task(s)", tasks.len());
        }

        TaskCmd::Get { id } => {
            let resp = c.get(&format!("/api/tasks/{id}")).await?;
            let task = resp.get("task").unwrap_or(&resp);
            if json_out {
                println!("{}", serde_json::to_string_pretty(task)?);
            } else {
                print_task_detail(task);
            }
        }

        TaskCmd::Create { title, description, project, priority, task_type } => {
            let mut body = json!({
                "title": title,
                "priority": priority,
                "task_type": task_type,
            });
            if let Some(d) = description { body["description"] = json!(d); }
            if let Some(p) = project     { body["project_id"]  = json!(p); }
            let resp = c.post("/api/tasks", &body).await?;
            let id = resp.get("id")
                .or_else(|| resp.get("task").and_then(|t| t.get("id")))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("created {id}");
            }
        }

        TaskCmd::Complete { id, output } => {
            let body = json!({ "output": output.unwrap_or_default() });
            let resp = c.put(&format!("/api/tasks/{id}/complete"), &body).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("completed {id}");
            }
        }

        TaskCmd::Cancel { id } => {
            let resp = c.delete(&format!("/api/tasks/{id}")).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("cancelled {id}");
            }
        }

        TaskCmd::Claim { id, agent } => {
            let body = json!({ "agent": agent });
            let resp = c.put(&format!("/api/tasks/{id}/claim"), &body).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("claimed {id} by {agent}");
            }
        }

        TaskCmd::Unclaim { id } => {
            let resp = c.put(&format!("/api/tasks/{id}/unclaim"), &json!({})).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("unclaimed {id}");
            }
        }
    }
    Ok(())
}

// ── Projects ──────────────────────────────────────────────────────────────────

async fn run_projects(c: &Client, sub: ProjectCmd, json_out: bool) -> Result<()> {
    match sub {
        ProjectCmd::List { status, q, limit } => {
            let mut qs: Vec<(&str, String)> = vec![("limit", limit.to_string())];
            if let Some(s) = &status { qs.push(("status", s.clone())); }
            if let Some(s) = &q      { qs.push(("q", s.clone())); }
            let qs_ref: Vec<(&str, &str)> = qs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let resp = c.get_qs("/api/projects", &qs_ref).await?;
            let projects = resp.as_array()
                .cloned()
                .or_else(|| resp["projects"].as_array().cloned())
                .unwrap_or_default();

            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
                return Ok(());
            }
            if projects.is_empty() { println!("(no projects)"); return Ok(()); }
            for p in &projects { print_project(p); }
            println!("\n{} project(s)", projects.len());
        }

        ProjectCmd::Get { id } => {
            let resp = c.get(&format!("/api/projects/{id}")).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let p = resp.get("project").unwrap_or(&resp);
                println!("id:      {}", s(p, "id"));
                println!("name:    {}", s(p, "name"));
                println!("status:  {}", s(p, "status"));
                println!("repo:    {}", s(p, "repo"));
                let desc = s(p, "description");
                if !desc.is_empty() { println!("\n{desc}"); }
            }
        }

        ProjectCmd::Create { name, description, repo } => {
            let mut body = json!({ "name": name });
            if let Some(d) = description { body["description"] = json!(d); }
            if let Some(r) = repo        { body["repo"]        = json!(r); }
            let resp = c.post("/api/projects", &body).await?;
            let id = resp.get("id")
                .or_else(|| resp.get("project").and_then(|p| p.get("id")))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("created {id}");
            }
        }

        ProjectCmd::Delete { id, hard } => {
            let path = if hard {
                format!("/api/projects/{id}?hard=true")
            } else {
                format!("/api/projects/{id}")
            };
            let resp = c.delete(&path).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("deleted {id}");
            }
        }
    }
    Ok(())
}

// ── Agents ────────────────────────────────────────────────────────────────────

async fn run_agents(c: &Client, sub: AgentCmd, json_out: bool) -> Result<()> {
    match sub {
        AgentCmd::List { online } => {
            let path = if online { "/api/agents?online=true" } else { "/api/agents" };
            let resp = c.get(path).await?;
            let agents = resp.as_array()
                .cloned()
                .or_else(|| resp["agents"].as_array().cloned())
                .unwrap_or_default();

            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
                return Ok(());
            }
            if agents.is_empty() { println!("(no agents)"); return Ok(()); }
            for a in &agents { print_agent(a); }
            println!("\n{} agent(s)", agents.len());
        }

        AgentCmd::Get { name } => {
            let resp = c.get(&format!("/api/agents/{name}")).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let a = resp.get("agent").unwrap_or(&resp);
                println!("name:       {}", s(a, "name"));
                let status = if a.get("online").and_then(|v| v.as_bool()).unwrap_or(false) {
                    "online"
                } else { "offline" };
                println!("status:     {status}");
                println!("node:       {}", fallback(s(a, "node"), s(a, "host")));
                println!("last_seen:  {}", s(a, "lastSeen"));
                let caps = a.get("capabilities").cloned().unwrap_or(json!({}));
                if let Some(obj) = caps.as_object() {
                    let names: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
                    if !names.is_empty() {
                        println!("caps:       {}", names.join(", "));
                    }
                }
            }
        }
    }
    Ok(())
}

// ── Bus ───────────────────────────────────────────────────────────────────────

async fn run_bus(c: &Client, sub: BusCmd, json_out: bool) -> Result<()> {
    match sub {
        BusCmd::Tail { filter, raw } => {
            use futures_util::StreamExt;
            use tokio_util::io::StreamReader;
            use tokio::io::AsyncBufReadExt;
            use std::io;

            eprintln!("Connecting to {}/api/bus/stream …", c.base);
            let resp = c.sse_stream("/api/bus/stream").await?;
            eprintln!("Connected. Streaming (Ctrl-C to stop)\n");

            let stream = resp.bytes_stream().map(|r| {
                r.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            });
            let reader = StreamReader::new(stream);
            let mut lines = tokio::io::BufReader::new(reader).lines();

            let mut buf = String::new();
            while let Some(line) = lines.next_line().await? {
                if raw {
                    println!("{line}");
                    continue;
                }
                if line.starts_with("data:") {
                    let data = line["data:".len()..].trim();
                    buf.push_str(data);
                } else if line.is_empty() && !buf.is_empty() {
                    if let Ok(msg) = serde_json::from_str::<Value>(&buf) {
                        let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("?");
                        if filter.as_deref().map(|f| msg_type.contains(f)).unwrap_or(true) {
                            if json_out {
                                println!("{}", serde_json::to_string(&msg)?);
                            } else {
                                print_bus_message(&msg);
                            }
                        }
                    }
                    buf.clear();
                }
                // ':' keep-alive comments are ignored
            }
        }

        BusCmd::Send { msg_type, body } => {
            let mut payload = if let Some(b) = body {
                serde_json::from_str::<Value>(&b)
                    .context("--body must be valid JSON")?
            } else {
                json!({})
            };
            payload["type"] = json!(msg_type);
            let resp = c.post("/api/bus/send", &payload).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("sent");
            }
        }

        BusCmd::Messages { msg_type, limit } => {
            let mut qs = vec![("limit", limit.to_string())];
            if let Some(t) = &msg_type { qs.push(("type", t.clone())); }
            let qs_ref: Vec<(&str, &str)> = qs.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let resp = c.get_qs("/api/bus/messages", &qs_ref).await?;
            let msgs = resp.as_array()
                .cloned()
                .or_else(|| resp["messages"].as_array().cloned())
                .unwrap_or_default();
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
                return Ok(());
            }
            if msgs.is_empty() { println!("(no messages)"); return Ok(()); }
            for m in &msgs { print_bus_message(m); }
            println!("\n{} message(s)", msgs.len());
        }
    }
    Ok(())
}

fn print_bus_message(m: &Value) {
    let ts   = fallback(s(m, "ts"), s(m, "created_at"));
    let ts   = if ts.len() > 19 { &ts[..19] } else { &ts };
    let typ  = format!("{:<30}", s(m, "type"));
    // Print a brief summary: type + first non-type key as hint
    let hint: String = m.as_object()
        .map(|obj| {
            obj.iter()
                .filter(|(k, _)| k.as_str() != "type" && k.as_str() != "ts" && k.as_str() != "seq")
                .take(2)
                .map(|(k, v)| {
                    let val = v.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| v.to_string());
                    let val = if val.len() > 40 { format!("{}…", &val[..40]) } else { val };
                    format!("{k}={val}")
                })
                .collect::<Vec<_>>()
                .join("  ")
        })
        .unwrap_or_default();
    println!("{ts}  {typ}  {hint}");
}

// ── Queue ─────────────────────────────────────────────────────────────────────

async fn run_queue(c: &Client, sub: QueueCmd, json_out: bool) -> Result<()> {
    match sub {
        QueueCmd::List => {
            let resp = c.get("/api/queue").await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
                return Ok(());
            }
            let items = resp["items"].as_array()
                .cloned()
                .or_else(|| resp.as_array().cloned())
                .unwrap_or_default();
            if items.is_empty() { println!("(empty queue)"); return Ok(()); }
            for item in &items {
                let id    = &s(item, "id")[..s(item, "id").len().min(20)];
                let stat  = format!("{:<12}", s(item, "status"));
                let title = fallback(s(item, "title"), s(item, "task"));
                println!("{id}  {stat}  {title}");
            }
            println!("\n{} item(s)", items.len());
        }

        QueueCmd::Get { id } => {
            let resp = c.get(&format!("/api/item/{id}")).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                let item = resp.get("item").unwrap_or(&resp);
                println!("id:      {}", s(item, "id"));
                println!("status:  {}", s(item, "status"));
                println!("title:   {}", fallback(s(item, "title"), s(item, "task")));
                println!("created: {}", s(item, "created_at"));
            }
        }
    }
    Ok(())
}
