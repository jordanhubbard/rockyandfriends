use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use acc_model::{AgentCapacity, AgentExecutor, AgentSession, HeartbeatRequest};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::session_discovery::{self, DiscoverySnapshot};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryDisk {
    #[serde(default)]
    executors: Vec<AgentExecutor>,
    #[serde(default)]
    sessions: Vec<AgentSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capacity: Option<AgentCapacity>,
}

#[derive(Debug, Clone, Default)]
struct RegistryState {
    executors: Vec<AgentExecutor>,
    sessions: Vec<AgentSession>,
    capacity: AgentCapacity,
    last_refresh: Option<Instant>,
}

pub struct SessionRegistry {
    path: PathBuf,
    state: RwLock<RegistryState>,
}

impl SessionRegistry {
    fn load(cfg: &Config) -> Arc<Self> {
        let path = cfg.session_registry_file();
        let disk = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<RegistryDisk>(&s).ok())
            .unwrap_or_default();
        let has_disk_snapshot =
            !disk.executors.is_empty() || !disk.sessions.is_empty() || disk.capacity.is_some();
        let capacity = disk.capacity.unwrap_or_default();
        Arc::new(Self {
            path,
            state: RwLock::new(RegistryState {
                executors: disk.executors,
                sessions: disk.sessions,
                capacity,
                last_refresh: has_disk_snapshot.then(Instant::now),
            }),
        })
    }

    async fn refresh_if_stale(&self, cfg: &Config) {
        let stale = {
            let state = self.state.read().await;
            state
                .last_refresh
                .map(|t| t.elapsed() > Duration::from_secs(30))
                .unwrap_or(true)
        };
        if stale {
            self.refresh(cfg).await;
        }
    }

    async fn refresh(&self, cfg: &Config) {
        let snapshot = session_discovery::discover(cfg).await;
        let capacity = build_capacity(cfg, &snapshot.sessions);
        {
            let mut state = self.state.write().await;
            state.executors = snapshot.executors.clone();
            state.sessions = snapshot.sessions.clone();
            state.capacity = capacity.clone();
            state.last_refresh = Some(Instant::now());
        }
        persist_registry(&self.path, &snapshot, &capacity);
    }

    async fn heartbeat_fragment(&self, cfg: &Config) -> HeartbeatFragment {
        self.refresh_if_stale(cfg).await;
        let state = self.state.read().await;
        HeartbeatFragment {
            executors: state.executors.clone(),
            sessions: state.sessions.clone(),
            free_session_slots: state.capacity.free_session_slots,
            max_sessions: state.capacity.max_sessions,
            session_spawn_denied_reason: state.capacity.session_spawn_denied_reason.clone(),
        }
    }

    async fn snapshot(&self, cfg: &Config) -> HeartbeatFragment {
        self.heartbeat_fragment(cfg).await
    }

    async fn upsert_session(&self, cfg: &Config, session: AgentSession) {
        let mut state = self.state.write().await;
        if let Some(pos) = state.sessions.iter().position(|s| s.name == session.name) {
            state.sessions[pos] = session.clone();
        } else {
            state.sessions.push(session.clone());
        }
        if let Some(executor) = session.executor.as_deref().filter(|s| !s.is_empty()) {
            if !state
                .executors
                .iter()
                .any(|entry| entry.executor == executor)
            {
                state.executors.push(AgentExecutor {
                    executor: executor.to_string(),
                    ready: Some(true),
                    auth_state: Some(
                        session
                            .auth_state
                            .clone()
                            .unwrap_or_else(|| "ready".to_string()),
                    ),
                    installed: Some(true),
                    ..Default::default()
                });
            }
        }
        state.capacity = build_capacity(cfg, &state.sessions);
        state.last_refresh = Some(Instant::now());
        let snapshot = DiscoverySnapshot {
            executors: state.executors.clone(),
            sessions: state.sessions.clone(),
        };
        let capacity = state.capacity.clone();
        drop(state);
        persist_registry(&self.path, &snapshot, &capacity);
    }

    async fn admit_session_spawn(&self, cfg: &Config, executor: &str) -> Result<(), String> {
        self.refresh_if_stale(cfg).await;
        if let Some(reason) = {
            let state = self.state.read().await;
            session_spawn_denial_reason(cfg, &state.sessions, executor)
        } {
            self.record_spawn_denial(reason.clone()).await;
            return Err(reason);
        }
        Ok(())
    }

    async fn record_spawn_denial(&self, reason: String) {
        let mut state = self.state.write().await;
        state.capacity.session_spawn_denied_reason = Some(reason.clone());
        let snapshot = DiscoverySnapshot {
            executors: state.executors.clone(),
            sessions: state.sessions.clone(),
        };
        let capacity = state.capacity.clone();
        drop(state);
        persist_registry(&self.path, &snapshot, &capacity);
    }
}

#[derive(Debug, Clone, Default)]
pub struct HeartbeatFragment {
    pub executors: Vec<AgentExecutor>,
    pub sessions: Vec<AgentSession>,
    pub free_session_slots: Option<u32>,
    pub max_sessions: Option<u32>,
    pub session_spawn_denied_reason: Option<String>,
}

pub async fn augment_heartbeat(cfg: &Config, req: &mut HeartbeatRequest) {
    let revision = workspace_revision(cfg).unwrap_or_else(|| "unknown".to_string());
    req.ccc_version = Some(revision.clone());
    req.workspace_revision = Some(revision);
    req.runtime_version = Some(env!("CARGO_PKG_VERSION").to_string());
    apply_task_capacity_defaults(cfg, req);

    let fragment = snapshot(cfg).await;
    req.executors = fragment.executors;
    req.sessions = fragment.sessions;
    req.free_session_slots = fragment.free_session_slots;
    req.max_sessions = fragment.max_sessions;
    req.session_spawn_denied_reason = fragment.session_spawn_denied_reason;
}

pub async fn snapshot(cfg: &Config) -> HeartbeatFragment {
    shared(cfg).snapshot(cfg).await
}

#[allow(dead_code)]
pub async fn upsert_session(cfg: &Config, session: AgentSession) {
    shared(cfg).upsert_session(cfg, session).await;
}

pub async fn admit_session_spawn(cfg: &Config, executor: &str) -> Result<(), String> {
    shared(cfg).admit_session_spawn(cfg, executor).await
}

fn apply_task_capacity_defaults(cfg: &Config, req: &mut HeartbeatRequest) {
    let in_flight = req.tasks_in_flight.unwrap_or(0);
    req.tasks_in_flight.get_or_insert(in_flight);
    req.estimated_free_slots
        .get_or_insert_with(|| cfg.max_tasks_per_agent().saturating_sub(in_flight));
}

fn shared(cfg: &Config) -> Arc<SessionRegistry> {
    static CELL: OnceLock<Arc<SessionRegistry>> = OnceLock::new();
    CELL.get_or_init(|| SessionRegistry::load(cfg)).clone()
}

fn workspace_revision(cfg: &Config) -> Option<String> {
    let workspace = cfg.acc_dir.join("workspace");
    if !workspace.join(".git").exists() {
        return None;
    }
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&workspace)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|rev| rev.trim().to_string())
        .filter(|rev| !rev.is_empty())
}

fn build_capacity(cfg: &Config, sessions: &[AgentSession]) -> AgentCapacity {
    let max_sessions = cfg.max_cli_sessions();
    let used_sessions = sessions
        .iter()
        .filter(|s| s.state.as_deref() != Some("dead"))
        .count() as u32;
    let free_session_slots = max_sessions.saturating_sub(used_sessions);
    let session_spawn_denied_reason = if free_session_slots == 0 {
        Some("session_limit_reached".to_string())
    } else {
        memory_pressure_reason(cfg, available_memory_mb())
    };

    AgentCapacity {
        free_session_slots: Some(free_session_slots),
        max_sessions: Some(max_sessions),
        session_spawn_denied_reason,
        ..Default::default()
    }
}

fn session_spawn_denial_reason(
    cfg: &Config,
    sessions: &[AgentSession],
    executor: &str,
) -> Option<String> {
    let live_sessions = sessions
        .iter()
        .filter(|s| s.state.as_deref() != Some("dead"))
        .count() as u32;
    if live_sessions >= cfg.max_cli_sessions() {
        return Some("session_limit_reached".to_string());
    }

    let executor_sessions = sessions
        .iter()
        .filter(|s| s.state.as_deref() != Some("dead") && s.executor.as_deref() == Some(executor))
        .count() as u32;
    let max_for_executor = cfg.max_cli_sessions_per_executor(executor);
    if executor_sessions >= max_for_executor {
        return Some(format!("executor_session_limit_reached:{executor}"));
    }

    memory_pressure_reason(cfg, available_memory_mb())
}

fn memory_pressure_reason(cfg: &Config, available_memory_mb: Option<u64>) -> Option<String> {
    available_memory_mb.and_then(|free_mb| {
        if free_mb < cfg.session_min_free_memory_mb() {
            Some(format!("memory_pressure:{free_mb}mb"))
        } else {
            None
        }
    })
}

fn persist_registry(path: &PathBuf, snapshot: &DiscoverySnapshot, capacity: &AgentCapacity) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let disk = RegistryDisk {
        executors: snapshot.executors.clone(),
        sessions: snapshot.sessions.clone(),
        capacity: Some(capacity.clone()),
    };
    if let Ok(data) = serde_json::to_vec_pretty(&disk) {
        let _ = std::fs::write(path, data);
    }
}

fn available_memory_mb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemAvailable:") {
                let kb = rest.split_whitespace().next()?.parse::<u64>().ok()?;
                return Some(kb / 1024);
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("vm_stat").output().ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let page_size = 4096u64;
        let mut free_pages = 0u64;
        for line in stdout.lines() {
            if line.starts_with("Pages free:") || line.starts_with("Pages speculative:") {
                let value = line
                    .split(':')
                    .nth(1)?
                    .trim()
                    .trim_end_matches('.')
                    .replace('.', "");
                free_pages += value.parse::<u64>().ok()?;
            }
        }
        Some((free_pages * page_size) / (1024 * 1024))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn test_cfg(acc_dir: std::path::PathBuf) -> Config {
        Config {
            acc_dir,
            acc_url: "http://example.test".into(),
            acc_token: "tok".into(),
            agent_name: "agent".into(),
            agentbus_token: "bus".into(),
            pair_programming: true,
            host: "host".into(),
            ssh_user: "user".into(),
            ssh_host: "host".into(),
            ssh_port: 22,
        }
    }

    fn session(name: &str, state: &str) -> AgentSession {
        AgentSession {
            name: name.to_string(),
            state: Some(state.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn build_capacity_counts_non_dead_sessions() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ACC_MAX_CLI_SESSIONS", "3");
        let cfg = test_cfg(std::env::temp_dir());
        let capacity = build_capacity(
            &cfg,
            &[
                session("a", "busy"),
                session("b", "dead"),
                session("c", "idle"),
            ],
        );
        assert_eq!(capacity.max_sessions, Some(3));
        assert_eq!(capacity.free_session_slots, Some(1));
        std::env::remove_var("ACC_MAX_CLI_SESSIONS");
    }

    #[test]
    fn task_capacity_defaults_preserve_explicit_keepalive_values() {
        let cfg = test_cfg(std::env::temp_dir());
        let mut req = HeartbeatRequest {
            tasks_in_flight: Some(2),
            estimated_free_slots: Some(9),
            ..Default::default()
        };

        apply_task_capacity_defaults(&cfg, &mut req);

        assert_eq!(req.tasks_in_flight, Some(2));
        assert_eq!(req.estimated_free_slots, Some(9));
    }

    #[test]
    fn task_capacity_defaults_fill_missing_fields() {
        let cfg = test_cfg(std::env::temp_dir());
        let mut req = HeartbeatRequest {
            tasks_in_flight: Some(1),
            ..Default::default()
        };

        apply_task_capacity_defaults(&cfg, &mut req);

        assert_eq!(req.tasks_in_flight, Some(1));
        assert_eq!(req.estimated_free_slots, Some(1));
    }

    #[tokio::test]
    async fn registry_reloads_persisted_snapshot_before_refresh() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path().to_path_buf());
        let path = cfg.session_registry_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&RegistryDisk {
                executors: vec![AgentExecutor {
                    executor: "codex_cli".into(),
                    ready: Some(true),
                    auth_state: Some("ready".into()),
                    ..Default::default()
                }],
                sessions: vec![AgentSession {
                    name: "proj-main".into(),
                    executor: Some("codex_cli".into()),
                    project_id: Some("proj-1".into()),
                    state: Some("idle".into()),
                    ..Default::default()
                }],
                capacity: Some(AgentCapacity {
                    free_session_slots: Some(1),
                    max_sessions: Some(4),
                    ..Default::default()
                }),
            })
            .unwrap(),
        )
        .unwrap();

        let registry = SessionRegistry::load(&cfg);
        let fragment = registry.snapshot(&cfg).await;

        assert_eq!(fragment.executors[0].executor, "codex_cli");
        assert_eq!(fragment.sessions[0].name, "proj-main");
        assert_eq!(fragment.free_session_slots, Some(1));
    }

    #[tokio::test]
    async fn upsert_session_persists_spawned_session_state() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path().to_path_buf());
        let registry = SessionRegistry::load(&cfg);

        registry
            .upsert_session(
                &cfg,
                AgentSession {
                    name: "spawned".into(),
                    executor: Some("claude_cli".into()),
                    project_id: Some("proj-2".into()),
                    state: Some("idle".into()),
                    auth_state: Some("ready".into()),
                    ..Default::default()
                },
            )
            .await;

        let persisted: RegistryDisk =
            serde_json::from_str(&std::fs::read_to_string(cfg.session_registry_file()).unwrap())
                .unwrap();

        assert_eq!(persisted.sessions[0].name, "spawned");
        assert_eq!(persisted.executors[0].executor, "claude_cli");
        assert!(persisted
            .capacity
            .and_then(|capacity| capacity.free_session_slots)
            .is_some());
    }

    #[tokio::test]
    async fn admission_denies_when_total_session_limit_reached() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ACC_MAX_CLI_SESSIONS", "2");
        std::env::set_var("ACC_SESSION_MIN_FREE_MEMORY_MB", "0");
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path().to_path_buf());
        let registry = SessionRegistry::load(&cfg);
        for idx in 0..cfg.max_cli_sessions() {
            registry
                .upsert_session(
                    &cfg,
                    AgentSession {
                        name: format!("session-{idx}"),
                        executor: Some("claude_cli".into()),
                        state: Some("idle".into()),
                        ..Default::default()
                    },
                )
                .await;
        }

        let err = registry
            .admit_session_spawn(&cfg, "codex_cli")
            .await
            .unwrap_err();

        assert_eq!(err, "session_limit_reached");
        std::env::remove_var("ACC_MAX_CLI_SESSIONS");
        std::env::remove_var("ACC_SESSION_MIN_FREE_MEMORY_MB");
    }

    #[tokio::test]
    async fn admission_denies_when_executor_limit_reached() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ACC_MAX_SESSIONS_PER_EXECUTOR", "1");
        std::env::set_var("ACC_SESSION_MIN_FREE_MEMORY_MB", "0");
        let tmp = tempfile::tempdir().unwrap();
        let cfg = test_cfg(tmp.path().to_path_buf());
        let registry = SessionRegistry::load(&cfg);
        registry
            .upsert_session(
                &cfg,
                AgentSession {
                    name: "codex-1".into(),
                    executor: Some("codex_cli".into()),
                    state: Some("idle".into()),
                    ..Default::default()
                },
            )
            .await;

        let err = registry
            .admit_session_spawn(&cfg, "codex_cli")
            .await
            .unwrap_err();

        assert_eq!(err, "executor_session_limit_reached:codex_cli");
        std::env::remove_var("ACC_MAX_SESSIONS_PER_EXECUTOR");
        std::env::remove_var("ACC_SESSION_MIN_FREE_MEMORY_MB");
    }

    #[test]
    fn memory_pressure_reason_reports_low_headroom() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("ACC_SESSION_MIN_FREE_MEMORY_MB", "2048");
        let cfg = test_cfg(std::env::temp_dir());

        assert_eq!(
            memory_pressure_reason(&cfg, Some(128)).as_deref(),
            Some("memory_pressure:128mb")
        );
        assert!(memory_pressure_reason(&cfg, Some(4096)).is_none());
        std::env::remove_var("ACC_SESSION_MIN_FREE_MEMORY_MB");
    }
}
