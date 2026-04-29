//! Integration tests for project + agent registry endpoints.

use acc_client::{
    model::{
        AgentCapabilitiesRequest, AgentCapacity, AgentExecutor, AgentRegistrationRequest,
        AgentSession,
    },
    Client,
};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn client_for(server: &MockServer) -> Client {
    Client::new(server.uri(), "t").unwrap()
}

#[tokio::test]
async fn projects_list_handles_wrapped_envelope_with_total() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "projects": [{"id": "proj-1", "name": "demo", "status": "active"}],
            "total": 1,
            "offset": 0
        })))
        .mount(&server)
        .await;
    let client = client_for(&server).await;
    let v = client.projects().list().send().await.unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].name, "demo");
}

#[tokio::test]
async fn project_create_handles_ok_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/projects"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "project": {"id": "proj-2", "name": "new", "status": "active"}
        })))
        .mount(&server)
        .await;
    let client = client_for(&server).await;
    let p = client
        .projects()
        .create(&acc_client::model::CreateProjectRequest {
            name: "new".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(p.id, "proj-2");
}

#[tokio::test]
async fn agents_list_filters_by_online() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/agents"))
        .and(query_param("online", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "agents": [
                {"name": "natasha", "online": true, "onlineStatus": "online", "gpu": true, "gpu_temp_c": 48.0},
                {"name": "boris",   "online": true, "onlineStatus": "online"}
            ]
        })))
        .mount(&server)
        .await;
    let client = client_for(&server).await;
    let agents = client.agents().list().online(true).send().await.unwrap();
    assert_eq!(agents.len(), 2);
    // GPU telemetry rode in via `extra`
    let natasha = agents.iter().find(|a| a.name == "natasha").unwrap();
    assert!(natasha.extra.contains_key("gpu_temp_c"));
}

#[tokio::test]
async fn agent_register_sends_canonical_executor_session_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/agents/register"))
        .and(header("authorization", "Bearer t"))
        .and(body_partial_json(json!({
            "name": "codex-node",
            "host": "desk",
            "type": "partial",
            "tool_capabilities": ["bash", "read_file"],
            "executors": [{"executor": "codex_cli", "ready": true, "auth_state": "ready"}],
            "sessions": [{"name": "main", "executor": "codex_cli", "state": "idle"}],
            "capacity": {"tasks_in_flight": 1, "estimated_free_slots": 2}
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "ok": true,
            "agent": {
                "name": "codex-node",
                "host": "desk",
                "type": "partial",
                "tool_capabilities": ["bash", "read_file"],
                "executors": [{"type": "codex_cli", "ready": true, "auth_state": "ready"}],
                "sessions": [{"name": "main", "executor": "codex_cli", "state": "idle"}],
                "capacity": {"tasks_in_flight": 1, "estimated_free_slots": 2}
            }
        })))
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let agent = client
        .agents()
        .register(&AgentRegistrationRequest {
            name: "codex-node".into(),
            host: Some("desk".into()),
            agent_type: Some("partial".into()),
            tool_capabilities: vec!["bash".into(), "read_file".into()],
            executors: vec![AgentExecutor {
                executor: "codex_cli".into(),
                ready: Some(true),
                auth_state: Some("ready".into()),
                ..Default::default()
            }],
            sessions: vec![AgentSession {
                name: "main".into(),
                executor: Some("codex_cli".into()),
                state: Some("idle".into()),
                ..Default::default()
            }],
            capacity: Some(AgentCapacity {
                tasks_in_flight: Some(1),
                estimated_free_slots: Some(2),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(agent.name, "codex-node");
    assert_eq!(agent.agent_type.as_deref(), Some("partial"));
    assert_eq!(agent.executors[0].executor, "codex_cli");
    assert_eq!(agent.sessions[0].name, "main");
    assert_eq!(
        agent.capacity.unwrap().estimated_free_slots,
        Some(2),
        "capacity should round-trip as typed telemetry"
    );
}

#[tokio::test]
async fn agent_put_capabilities_decodes_capabilities_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/agents/codex-node/capabilities"))
        .and(header("authorization", "Bearer t"))
        .and(body_partial_json(json!({
            "capabilities": ["bash", "codex_cli"]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "name": "codex-node",
            "capabilities": ["bash", "codex_cli"]
        })))
        .mount(&server)
        .await;

    let client = client_for(&server).await;
    let caps = client
        .agents()
        .put_capabilities(
            "codex-node",
            &AgentCapabilitiesRequest {
                capabilities: vec!["bash".into(), "codex_cli".into()],
            },
        )
        .await
        .unwrap();

    assert_eq!(caps, vec!["bash", "codex_cli"]);
}
