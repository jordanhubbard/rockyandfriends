use clap::Parser;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(
    name = "github-sync",
    about = "Two-way GitHub ↔ beads ↔ fleet task sync"
)]
struct Args {
    /// Run once and exit (default)
    #[arg(long)]
    once: bool,
    /// Run as a polling daemon
    #[arg(long)]
    daemon: bool,
    /// No writes (dry-run mode)
    #[arg(long)]
    dry_run: bool,
    /// Backfill structured GitHub metadata onto already-linked beads issues
    #[arg(long)]
    migrate_metadata: bool,
    /// Create both a GitHub issue and a linked beads issue
    #[arg(long)]
    mirror: bool,
    /// GitHub repo for --mirror, owner/repo
    #[arg(long)]
    mirror_repo: Option<String>,
    /// Issue title for --mirror
    #[arg(long)]
    title: Option<String>,
    /// Issue description/body for --mirror
    #[arg(long)]
    description: Option<String>,
    /// Issue labels for --mirror, comma-separated or repeated
    #[arg(long, value_delimiter = ',')]
    labels: Vec<String>,
    /// Beads issue type for --mirror
    #[arg(long, default_value = "task")]
    issue_type: String,
    /// Beads priority for --mirror
    #[arg(long, default_value = "2")]
    priority: String,
    /// owner/repo overrides (space-separated; falls back to GITHUB_REPOS env)
    #[arg(value_name = "REPO")]
    repos: Vec<String>,
}

fn dispatch_label() -> String {
    std::env::var("GITHUB_DISPATCH_LABEL").unwrap_or_else(|_| "agent-ready".to_string())
}

fn sync_interval() -> u64 {
    std::env::var("GITHUB_SYNC_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300)
}

fn data_dir() -> PathBuf {
    std::env::var("ACC_DATA_DIR")
        .or_else(|_| std::env::var("CCC_DATA_DIR"))
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            format!("{home}/.acc/data")
        })
        .into()
}

fn state_path() -> PathBuf {
    data_dir().join("github-sync-state.json")
}

// ── State ─────────────────────────────────────────────────────────────────

fn load_state(path: &PathBuf) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn save_state(path: &PathBuf, state: &Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let s = serde_json::to_string_pretty(state).unwrap_or_default() + "\n";
    if std::fs::write(&tmp, s).is_ok() {
        std::fs::rename(tmp, path).ok();
    }
}

// ── GitHub helpers ────────────────────────────────────────────────────────

fn gh_issue_list(repo: &str) -> Vec<Value> {
    let out = Command::new("gh")
        .args([
            "issue",
            "list",
            "--repo",
            repo,
            "--state",
            "all",
            "--limit",
            "200",
            "--json",
            "number,title,body,labels,state,url,author,createdAt,updatedAt",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut issues: Vec<Value> = serde_json::from_str(&text).unwrap_or_default();
            for issue in &mut issues {
                issue["repo"] = json!(repo);
            }
            issues
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            eprintln!("WARN gh issue list failed for {repo}: {}", err.trim());
            vec![]
        }
        Err(e) => {
            eprintln!("WARN gh CLI error for {repo}: {e}");
            vec![]
        }
    }
}

fn gh_issue_create(
    repo: &str,
    title: &str,
    body: &str,
    labels: &[String],
    dry_run: bool,
) -> Option<String> {
    if dry_run {
        println!(
            "  [dry-run] would create GitHub issue in {repo}: {}",
            title.chars().take(80).collect::<String>()
        );
        return Some(format!("https://github.com/{repo}/issues/0"));
    }

    let mut cmd = Command::new("gh");
    cmd.args([
        "issue", "create", "--repo", repo, "--title", title, "--body", body,
    ]);
    if !labels.is_empty() {
        cmd.args(["--label", &labels.join(",")]);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        eprintln!(
            "WARN gh issue create failed for {repo}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn gh_issue_edit_body(repo: &str, number: i64, body: &str, dry_run: bool) -> bool {
    if dry_run {
        println!(
            "  [dry-run] would edit body for {repo}#{number}: {}",
            body.chars().take(80).collect::<String>()
        );
        return true;
    }
    Command::new("gh")
        .args([
            "issue",
            "edit",
            &number.to_string(),
            "--repo",
            repo,
            "--body",
            body,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[allow(dead_code)]
fn gh_issue_comment(repo: &str, number: i64, body: &str, dry_run: bool) -> bool {
    if dry_run {
        println!(
            "  [dry-run] would comment on {repo}#{number}: {}",
            &body[..body.len().min(60)]
        );
        return true;
    }
    Command::new("gh")
        .args([
            "issue",
            "comment",
            &number.to_string(),
            "--repo",
            repo,
            "--body",
            body,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[allow(dead_code)]
fn gh_issue_close(repo: &str, number: i64, dry_run: bool) -> bool {
    if dry_run {
        println!("  [dry-run] would close {repo}#{number}");
        return true;
    }
    Command::new("gh")
        .args(["issue", "close", &number.to_string(), "--repo", repo])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── beads helpers ─────────────────────────────────────────────────────────

fn bd_bin() -> String {
    which::which("bd")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            format!("{home}/.local/bin/bd")
        })
}

fn bd(args: &[&str], dry_run: bool) -> (i32, String) {
    if dry_run
        && args
            .first()
            .map(|a| matches!(*a, "create" | "update" | "close"))
            .unwrap_or(false)
    {
        println!("  [dry-run] bd {}", args.join(" "));
        return (0, String::new());
    }
    let out = Command::new(bd_bin()).args(args).output();
    match out {
        Ok(o) => (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).trim().to_string(),
        ),
        Err(_) => (-1, String::new()),
    }
}

fn list_beads_issues() -> Vec<Value> {
    let (_, out) = bd(&["export"], false);
    let mut issues = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        if line.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                issues.push(v);
            }
        }
    }
    issues
}

// ── ACC fleet API helpers ─────────────────────────────────────────────────

async fn acc_get(
    client: &reqwest::Client,
    acc_url: &str,
    token: &str,
    path: &str,
) -> Option<Value> {
    let resp = client
        .get(format!("{acc_url}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .ok()?;
    resp.json::<Value>().await.ok()
}

async fn acc_post(
    client: &reqwest::Client,
    acc_url: &str,
    token: &str,
    path: &str,
    body: &Value,
    dry_run: bool,
) -> Option<Value> {
    if dry_run {
        println!(
            "  [dry-run] POST {path}: {}",
            body.to_string().chars().take(120).collect::<String>()
        );
        return Some(json!({"ok": true, "task": {"id": "dry-run-task"}}));
    }
    let resp = client
        .post(format!("{acc_url}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(body)
        .send()
        .await
        .ok()?;
    resp.json::<Value>().await.ok()
}

async fn find_project_for_repo(
    client: &reqwest::Client,
    acc_url: &str,
    token: &str,
    repo: &str,
) -> Option<String> {
    let resp = acc_get(client, acc_url, token, "/api/projects").await?;
    let projects: Vec<Value> = if resp.is_array() {
        serde_json::from_value(resp).unwrap_or_default()
    } else {
        resp.get("projects")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let repo_name = repo.split('/').next_back().unwrap_or(repo).to_lowercase();
    for p in &projects {
        if p.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .as_deref()
            == Some(&repo_name)
        {
            return p.get("id").and_then(|v| v.as_str()).map(str::to_owned);
        }
    }
    for p in &projects {
        if p.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_lowercase())
            .as_deref()
            == Some("acc")
        {
            return p.get("id").and_then(|v| v.as_str()).map(str::to_owned);
        }
    }
    projects
        .first()
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn update_beads_metadata(beads_id: &str, metadata: &Value, dry_run: bool) {
    bd(
        &[
            "update",
            beads_id,
            &format!("--metadata={}", compact_json(metadata)),
        ],
        dry_run,
    );
}

fn update_fleet_task_metadata(beads_id: &str, task_id: &str, dry_run: bool) {
    let (_, out) = bd(&["export"], false);
    for line in out.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let b: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if b.get("id").and_then(|v| v.as_str()) == Some(beads_id) {
            let (num, repo, _) = parse_link_meta(&b);
            if let (Some(number), Some(repo)) = (num, repo) {
                let url = b
                    .pointer("/metadata/github_url")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("https://github.com/{repo}/issues/{number}"));
                let labels: Vec<String> = b
                    .pointer("/metadata/github_labels")
                    .map(github_labels_from_value)
                    .unwrap_or_default();
                let metadata = github_metadata_from_parts(
                    &repo,
                    number,
                    &url,
                    &labels,
                    Some(task_id),
                    b.get("metadata"),
                );
                update_beads_metadata(beads_id, &metadata, dry_run);
            }
            return;
        }
    }
}

// ── Sync logic ────────────────────────────────────────────────────────────

fn github_issue_number_from_url(url: &str) -> Option<i64> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
}

fn github_labels_from_value(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| {
                    label
                        .get("name")
                        .and_then(|v| v.as_str())
                        .or_else(|| label.as_str())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn gh_link_metadata(issue: &Value, base: Option<&Value>, fleet_task_id: Option<&str>) -> Value {
    let mut obj = base
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let repo = issue
        .get("repo")
        .or_else(|| issue.get("github_repo"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let number = issue
        .get("number")
        .or_else(|| issue.get("github_number"))
        .and_then(|v| v.as_i64())
        .or_else(|| {
            issue
                .get("url")
                .or_else(|| issue.get("github_url"))
                .and_then(|v| v.as_str())
                .and_then(github_issue_number_from_url)
        })
        .unwrap_or(0);
    let url = issue
        .get("url")
        .or_else(|| issue.get("html_url"))
        .or_else(|| issue.get("github_url"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("https://github.com/{repo}/issues/{number}"));
    let labels = issue
        .get("labels")
        .map(github_labels_from_value)
        .unwrap_or_default();
    let author = issue
        .pointer("/author/login")
        .or_else(|| issue.get("author"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let fleet_value = fleet_task_id
        .filter(|s| !s.is_empty())
        .map(|s| json!(s))
        .or_else(|| obj.get("fleet_task_id").cloned())
        .unwrap_or(Value::Null);
    obj.insert("source".into(), json!("github"));
    obj.insert("github_number".into(), json!(number));
    obj.insert("github_repo".into(), json!(repo));
    obj.insert("github_url".into(), json!(url));
    obj.insert("fleet_task_id".into(), fleet_value);
    obj.insert("github_labels".into(), json!(labels));
    if !author.is_empty() {
        obj.insert("github_author".into(), json!(author));
    }
    Value::Object(obj)
}

fn github_metadata_from_parts(
    repo: &str,
    number: i64,
    url: &str,
    labels: &[String],
    fleet_task_id: Option<&str>,
    base: Option<&Value>,
) -> Value {
    gh_link_metadata(
        &json!({
            "repo": repo,
            "number": number,
            "url": url,
            "labels": labels,
        }),
        base,
        fleet_task_id,
    )
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn parse_link_meta(b: &Value) -> (Option<i64>, Option<String>, Option<String>) {
    // Check structured metadata
    if let Some(meta) = b.get("metadata").and_then(|v| v.as_object()) {
        let num = meta.get("github_number").and_then(|v| v.as_i64());
        let r = meta
            .get("github_repo")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let task = meta
            .get("fleet_task_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        if num.is_some() && r.is_some() {
            return (num, r, task);
        }
    }
    // Fall back to notes key=value
    let notes = b.get("notes").and_then(|v| v.as_str()).unwrap_or("");
    let mut num = None;
    let mut repo = None;
    let mut fleet_task_id = None;
    for token in notes.split_whitespace() {
        if let Some((k, v)) = token.split_once('=') {
            match k {
                "github_number" => num = v.parse::<i64>().ok(),
                "github_repo" => repo = Some(v.to_string()),
                "fleet_task_id" => fleet_task_id = Some(v.to_string()),
                _ => {}
            }
        }
    }
    // Fall back to [gh:repo#number] in title
    if num.is_none() || repo.is_none() {
        let title = b.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let re = regex_gh_key(title);
        if let Some((r, n)) = re {
            repo.get_or_insert(r);
            num.get_or_insert(n);
        }
    }
    (num, repo, fleet_task_id)
}

fn regex_gh_key(title: &str) -> Option<(String, i64)> {
    // Manual parse of [gh:repo#number]
    let start = title.find("[gh:")?;
    let rest = &title[start + 4..];
    let hash = rest.find('#')?;
    let end = rest.find(']')?;
    if hash >= end {
        return None;
    }
    let repo = rest[..hash].to_string();
    let num: i64 = rest[hash + 1..end].parse().ok()?;
    Some((repo, num))
}

#[allow(dead_code)]
fn is_already_synced(issue: &Value, existing: &[Value]) -> bool {
    let num = issue.get("number").and_then(|v| v.as_i64());
    let repo = issue.get("repo").and_then(|v| v.as_str());
    existing.iter().any(|b| {
        let (bn, br, _) = parse_link_meta(b);
        bn == num && br.as_deref() == repo
    })
}

fn find_synced<'a>(issue: &Value, existing: &'a [Value]) -> Option<&'a Value> {
    let num = issue.get("number").and_then(|v| v.as_i64());
    let repo = issue.get("repo").and_then(|v| v.as_str());
    existing.iter().find(|b| {
        let (bn, br, _) = parse_link_meta(b);
        bn == num && br.as_deref() == repo
    })
}

fn has_dispatch_label(issue: &Value, label: &str) -> bool {
    issue
        .get("labels")
        .and_then(|l| l.as_array())
        .map(|labels| {
            labels
                .iter()
                .any(|lb| lb.get("name").and_then(|v| v.as_str()) == Some(label))
        })
        .unwrap_or(false)
}

fn label_names(issue: &Value) -> Vec<String> {
    issue
        .get("labels")
        .and_then(|l| l.as_array())
        .map(|labels| {
            labels
                .iter()
                .filter_map(|lb| lb.get("name").and_then(|v| v.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn map_priority(labels: &[String]) -> i64 {
    for name in labels {
        match name.as_str() {
            "P0" | "critical" => return 0,
            "P1" | "bug" | "high" => return 1,
            "P2" | "enhancement" | "medium" => return 2,
            "P3" | "low" => return 3,
            "P4" | "backlog" => return 4,
            _ => {}
        }
    }
    2
}

fn build_fleet_task_payload(issue: &Value, beads_id: &str, project_id: &str) -> Value {
    let labels = label_names(issue);
    let desc = issue
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let url = issue.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
    let title = issue.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let repo = issue.get("repo").and_then(|v| v.as_str()).unwrap_or("");
    let gh_ref = format!("\n\nGitHub: {url}  |  Beads: {beads_id}");
    json!({
        "title": format!("{title} (#{number}, {beads_id})"),
        "description": desc + &gh_ref,
        "project_id": project_id,
        "task_type": "work",
        "source": "github",
        "phase": "build",
        "priority": map_priority(&labels),
        "metadata": {
            "source": "github",
            "github_number": number,
            "github_repo": repo,
            "github_url": url,
            "beads_id": beads_id,
            "github_labels": labels,
        },
    })
}

async fn sync_repo(
    repo: &str,
    state: &mut Value,
    existing_beads: &[Value],
    client: &reqwest::Client,
    acc_url: &str,
    token: &str,
    dry_run: bool,
) -> (i64, i64, i64) {
    let issues = gh_issue_list(repo);
    if issues.is_empty() {
        return (0, 0, 0);
    }

    let project_id = find_project_for_repo(client, acc_url, token, repo).await;
    let mut created = 0i64;
    let mut updated = 0i64;
    let mut fleet_created = 0i64;
    let mut newest_ts = state
        .get(repo)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let label = dispatch_label();

    for issue in &issues {
        let ts = issue
            .get("updatedAt")
            .or_else(|| issue.get("createdAt"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if ts > newest_ts.as_str() {
            newest_ts = ts.to_string();
        }

        let labels = label_names(issue);
        let priority = map_priority(&labels);
        let number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0);
        let existing = find_synced(issue, existing_beads);

        if issue.get("state").and_then(|v| v.as_str()).unwrap_or("") == "CLOSED" {
            if let Some(b) = existing {
                if b.get("status").and_then(|v| v.as_str()) == Some("open") {
                    let bid = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    println!("  closing beads {bid} (GH #{number} closed)");
                    bd(&["close", bid, "--reason=closed on GitHub"], dry_run);
                    updated += 1;
                }
            }
            continue;
        }

        let title = issue
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let body = issue.get("body").and_then(|v| v.as_str()).unwrap_or("");

        if existing.is_none() {
            println!(
                "  creating beads issue for {repo}#{number}: {}",
                title.chars().take(60).collect::<String>()
            );
            let metadata = gh_link_metadata(issue, None, None);
            let (rc, out) = bd(
                &[
                    "create",
                    &format!("--title={title}"),
                    &format!("--description={body}"),
                    "--type=feature",
                    &format!("--priority={priority}"),
                    &format!("--external-ref=gh:{repo}#{number}"),
                    &format!("--metadata={}", compact_json(&metadata)),
                ],
                dry_run,
            );
            if rc == 0 {
                created += 1;
                // Extract beads ID from output like "Created issue: CCC-xyz — ..."
                let beads_id = extract_beads_id(&out);
                if let Some(bid) = beads_id {
                    if has_dispatch_label(issue, &label) {
                        if let Some(ref pid) = project_id {
                            let payload = build_fleet_task_payload(issue, &bid, pid);
                            if let Some(resp) =
                                acc_post(client, acc_url, token, "/api/tasks", &payload, dry_run)
                                    .await
                            {
                                if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                                    let task_id = resp
                                        .pointer("/task/id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    println!("    → fleet task {task_id}");
                                    fleet_created += 1;
                                    let metadata = gh_link_metadata(issue, None, Some(task_id));
                                    update_beads_metadata(&bid, &metadata, dry_run);
                                }
                            }
                        }
                    }
                }
            }
        } else {
            let b = existing.unwrap();
            let existing_title = b.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let existing_description = b.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let bid = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let existing_metadata = b.get("metadata");
            let (_, _, existing_fleet_task_id) = parse_link_meta(b);
            let desired_metadata =
                gh_link_metadata(issue, existing_metadata, existing_fleet_task_id.as_deref());
            let mut needs_update = false;
            let mut args: Vec<String> = vec!["update".to_string(), bid.to_string()];
            if existing_title != title {
                args.push(format!("--title={title}"));
                needs_update = true;
            }
            if existing_description != body {
                args.push(format!("--description={body}"));
                needs_update = true;
            }
            if existing_metadata != Some(&desired_metadata) {
                args.push(format!("--metadata={}", compact_json(&desired_metadata)));
                needs_update = true;
            }
            if needs_update {
                println!("  updating beads {bid}");
                let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                bd(&refs, dry_run);
                updated += 1;
            }
            let existing_status = b.get("status").and_then(|v| v.as_str()).unwrap_or("open");
            let has_fleet = desired_metadata
                .get("fleet_task_id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .is_some();
            if !matches!(existing_status, "closed" | "cancelled")
                && has_dispatch_label(issue, &label)
                && !has_fleet
            {
                if let Some(ref pid) = project_id {
                    let payload = build_fleet_task_payload(issue, bid, pid);
                    if let Some(resp) =
                        acc_post(client, acc_url, token, "/api/tasks", &payload, dry_run).await
                    {
                        if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
                            let task_id = resp
                                .pointer("/task/id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            println!("    → fleet task {task_id} for existing beads {bid}");
                            fleet_created += 1;
                            update_fleet_task_metadata(bid, task_id, dry_run);
                        }
                    }
                }
            }
        }
    }

    if !newest_ts.is_empty() {
        state[repo] = json!(newest_ts);
    }
    (created, updated, fleet_created)
}

fn extract_beads_id(out: &str) -> Option<String> {
    // Match any prefix like ACC-xyz, CCC-xyz, etc.
    for word in out.split_whitespace() {
        if word.len() >= 5
            && word.contains('-')
            && word[..word.find('-').unwrap()]
                .chars()
                .all(|c| c.is_ascii_uppercase())
        {
            let parts: Vec<&str> = word.splitn(2, '-').collect();
            if parts.len() == 2 && parts[0].len() >= 2 && parts[0].len() <= 6 {
                return Some(
                    word.trim_end_matches(|c: char| !c.is_alphanumeric())
                        .to_string(),
                );
            }
        }
    }
    None
}

fn is_closed_status(status: &str) -> bool {
    matches!(status, "closed" | "completed" | "cancelled" | "canceled")
}

fn migrate_metadata(dry_run: bool) -> Value {
    let beads = list_beads_issues();
    let mut migrated = 0i64;
    let mut skipped = 0i64;
    for b in beads {
        let bid = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let (num, repo, task_id) = parse_link_meta(&b);
        let (Some(number), Some(repo)) = (num, repo) else {
            skipped += 1;
            continue;
        };
        let url = b
            .pointer("/metadata/github_url")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("https://github.com/{repo}/issues/{number}"));
        let labels = b
            .pointer("/metadata/github_labels")
            .map(github_labels_from_value)
            .unwrap_or_default();
        let metadata = github_metadata_from_parts(
            &repo,
            number,
            &url,
            &labels,
            task_id.as_deref(),
            b.get("metadata"),
        );
        if b.get("metadata") != Some(&metadata) {
            println!("  backfilling GitHub metadata on {bid}");
            update_beads_metadata(bid, &metadata, dry_run);
            migrated += 1;
        } else {
            skipped += 1;
        }
    }
    json!({"migrated": migrated, "skipped": skipped})
}

fn sync_closed_beads_to_github(existing_beads: &[Value], dry_run: bool) -> i64 {
    if std::env::var("GITHUB_AUTO_CLOSE").unwrap_or_default() != "true" {
        return 0;
    }
    let mut closed = 0i64;
    for b in existing_beads {
        let status = b.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if !is_closed_status(status) {
            continue;
        }
        let (Some(number), Some(repo), _) = parse_link_meta(b) else {
            continue;
        };
        if gh_issue_close(&repo, number, dry_run) {
            closed += 1;
        }
    }
    closed
}

async fn sync_completed_fleet_tasks(
    client: &reqwest::Client,
    acc_url: &str,
    token: &str,
    dry_run: bool,
) -> i64 {
    let Some(resp) = acc_get(
        client,
        acc_url,
        token,
        "/api/tasks?status=completed&limit=200",
    )
    .await
    else {
        return 0;
    };
    let tasks = resp
        .get("tasks")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let existing_beads = list_beads_issues();
    let mut closed = 0i64;
    for task in tasks {
        let meta = task.get("metadata").unwrap_or(&Value::Null);
        if meta.get("source").and_then(|v| v.as_str()) != Some("github") {
            continue;
        }
        let Some(beads_id) = meta
            .get("beads_id")
            .or_else(|| meta.get("bead_id"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let Some(bead) = existing_beads
            .iter()
            .find(|b| b.get("id").and_then(|v| v.as_str()) == Some(beads_id))
        else {
            continue;
        };
        let status = bead.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if is_closed_status(status) {
            continue;
        }
        let task_id = task.get("id").and_then(|v| v.as_str()).unwrap_or("task");
        let agent = task
            .get("completed_by")
            .and_then(|v| v.as_str())
            .unwrap_or("agent");
        let reason = format!("fleet task {task_id} completed by {agent}");
        let (rc, _) = bd(&["close", beads_id, &format!("--reason={reason}")], dry_run);
        if rc == 0 {
            closed += 1;
        }
    }
    closed
}

fn run_mirror(args: &Args, dry_run: bool) -> Value {
    let repo = args
        .mirror_repo
        .as_deref()
        .or_else(|| args.repos.first().map(String::as_str))
        .unwrap_or("");
    let title = args.title.as_deref().unwrap_or("");
    if repo.is_empty() || title.is_empty() {
        eprintln!("ERROR: --mirror requires --mirror-repo owner/repo and --title");
        return json!({"ok": false, "error": "missing mirror repo or title"});
    }
    let body = args.description.as_deref().unwrap_or("");
    let url = match gh_issue_create(repo, title, body, &args.labels, dry_run) {
        Some(url) => url,
        None => return json!({"ok": false, "error": "gh issue create failed"}),
    };
    let number = github_issue_number_from_url(&url).unwrap_or(0);
    let metadata = github_metadata_from_parts(repo, number, &url, &args.labels, None, None);
    let mut bd_args = vec![
        "create".to_string(),
        format!("--title={title}"),
        format!("--description={body}"),
        format!("--type={}", args.issue_type),
        format!("--priority={}", args.priority),
        format!("--external-ref=gh:{repo}#{number}"),
        format!("--metadata={}", compact_json(&metadata)),
    ];
    if !args.labels.is_empty() {
        bd_args.push(format!("--labels={}", args.labels.join(",")));
    }
    let refs: Vec<&str> = bd_args.iter().map(|s| s.as_str()).collect();
    let (rc, out) = bd(&refs, dry_run);
    if rc != 0 {
        return json!({"ok": false, "error": "bd create failed", "github_url": url});
    }
    let beads_id = extract_beads_id(&out).unwrap_or_else(|| "dry-run-bead".to_string());
    let linked_body = if body.trim().is_empty() {
        format!("ACC beads issue: {beads_id}")
    } else {
        format!("{body}\n\nACC beads issue: {beads_id}")
    };
    let _ = gh_issue_edit_body(repo, number, &linked_body, dry_run);
    json!({
        "ok": true,
        "repo": repo,
        "github_number": number,
        "github_url": url,
        "beads_id": beads_id,
    })
}

// ── Main ──────────────────────────────────────────────────────────────────

async fn run_once(
    repos: &[String],
    dry_run: bool,
    client: &reqwest::Client,
    acc_url: &str,
    token: &str,
) -> Value {
    if repos.is_empty() {
        eprintln!("WARN: no repos configured — set GITHUB_REPOS=owner/repo,...");
        return json!({});
    }

    let sp = state_path();
    let mut state = load_state(&sp);
    let existing_beads = list_beads_issues();
    let mut results = json!({});

    for repo in repos {
        println!("Syncing {repo}…");
        let (c, u, f) = sync_repo(
            repo,
            &mut state,
            &existing_beads,
            client,
            acc_url,
            token,
            dry_run,
        )
        .await;
        results[repo] = json!({"created": c, "updated": u, "fleet_tasks": f});
        println!("  {repo}: +{c} created, ~{u} updated, {f} fleet tasks");
    }
    let fleet_closed = sync_completed_fleet_tasks(client, acc_url, token, dry_run).await;
    let github_closed = sync_closed_beads_to_github(&existing_beads, dry_run);
    results["_fleet_to_beads"] = json!({"closed": fleet_closed});
    results["_beads_to_github"] = json!({"closed": github_closed});

    if !dry_run {
        save_state(&sp, &state);
        // Export beads JSONL for git backup
        if let Ok(exe) = std::env::current_exe() {
            let beads_dir = exe
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.join(".beads").join("issues.jsonl"));
            if let Some(path) = beads_dir {
                Command::new(bd_bin())
                    .args(["export", "--output", &path.to_string_lossy()])
                    .output()
                    .ok();
            }
        }
    }

    results
}

#[tokio::main]
async fn main() {
    acc_tools::load_acc_env();
    let mut args = Args::parse();

    if args.dry_run {
        unsafe { std::env::set_var("DRY_RUN", "true") };
    }

    let configured_repos: Vec<String> = std::env::var("GITHUB_REPOS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();

    if args.repos.is_empty() {
        args.repos = configured_repos;
    }

    if args.mirror {
        let result = run_mirror(&args, args.dry_run);
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
        return;
    }

    if args.migrate_metadata {
        let result = migrate_metadata(args.dry_run);
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
        return;
    }

    let acc_url = acc_tools::acc_url();
    let token = acc_tools::acc_token();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("build reqwest client");

    if args.daemon {
        println!(
            "github-sync daemon starting — interval={}s repos={:?}",
            sync_interval(),
            args.repos
        );
        loop {
            run_once(&args.repos, args.dry_run, &client, &acc_url, &token).await;
            tokio::time::sleep(std::time::Duration::from_secs(sync_interval())).await;
        }
    } else {
        let result = run_once(&args.repos, args.dry_run, &client, &acc_url, &token).await;
        println!(
            "{}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_metadata_schema_contains_required_link_fields() {
        let issue = json!({
            "repo": "jordanhubbard/ACC",
            "number": 42,
            "url": "https://github.com/jordanhubbard/ACC/issues/42",
            "labels": [{"name": "bug"}, {"name": "agent-ready"}],
            "author": {"login": "external-user"}
        });
        let metadata = gh_link_metadata(&issue, None, Some("task-abc"));

        assert_eq!(metadata["source"], "github");
        assert_eq!(metadata["github_number"], 42);
        assert_eq!(metadata["github_repo"], "jordanhubbard/ACC");
        assert_eq!(
            metadata["github_url"],
            "https://github.com/jordanhubbard/ACC/issues/42"
        );
        assert_eq!(metadata["fleet_task_id"], "task-abc");
        assert_eq!(metadata["github_labels"], json!(["bug", "agent-ready"]));
        assert_eq!(metadata["github_author"], "external-user");
    }

    #[test]
    fn parse_link_meta_accepts_legacy_notes_and_title_fallbacks() {
        let legacy = json!({
            "id": "ACC-x",
            "title": "legacy",
            "notes": "source=github github_number=7 github_repo=jordanhubbard/ACC fleet_task_id=task-7"
        });
        assert_eq!(
            parse_link_meta(&legacy),
            (
                Some(7),
                Some("jordanhubbard/ACC".to_string()),
                Some("task-7".to_string())
            )
        );

        let title_fallback = json!({"title": "Thing [gh:jordanhubbard/ACC#9]"});
        assert_eq!(
            parse_link_meta(&title_fallback),
            (Some(9), Some("jordanhubbard/ACC".to_string()), None)
        );
    }

    #[test]
    fn fleet_task_payload_marks_github_source() {
        let issue = json!({
            "repo": "jordanhubbard/ACC",
            "number": 42,
            "title": "Fix thing",
            "body": "Details",
            "url": "https://github.com/jordanhubbard/ACC/issues/42",
            "labels": [{"name": "agent-ready"}],
        });
        let payload = build_fleet_task_payload(&issue, "ACC-abc", "proj");
        assert_eq!(payload["source"], "github");
        assert_eq!(payload["metadata"]["source"], "github");
        assert_eq!(payload["metadata"]["beads_id"], "ACC-abc");
        assert_eq!(payload["metadata"]["github_labels"], json!(["agent-ready"]));
    }
}
