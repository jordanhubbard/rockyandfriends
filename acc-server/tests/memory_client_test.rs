mod helpers;

use acc_client::model::{MemorySearchRequest, MemoryStoreRequest};

async fn client_for_router() -> (acc_client::Client, tokio::task::JoinHandle<()>) {
    let ts = helpers::TestServer::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = ts.app.clone();

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
        drop(ts);
    });

    let client = acc_client::Client::new(format!("http://{addr}"), helpers::TEST_TOKEN).unwrap();
    (client, server)
}

#[tokio::test]
async fn test_memory_client_uses_current_search_route() {
    let (client, server) = client_for_router().await;

    let err = client
        .memory()
        .search(&MemorySearchRequest {
            query: "".to_string(),
            limit: Some(1),
            collection: None,
        })
        .await
        .unwrap_err();

    assert_eq!(err.status_code(), Some(400));
    server.abort();
}

#[tokio::test]
async fn test_memory_client_uses_current_ingest_route() {
    let (client, server) = client_for_router().await;

    let err = client
        .memory()
        .store(&MemoryStoreRequest {
            text: "".to_string(),
            metadata: None,
            collection: None,
        })
        .await
        .unwrap_err();

    assert_eq!(err.status_code(), Some(400));
    server.abort();
}
