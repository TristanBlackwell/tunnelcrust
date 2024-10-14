use helpers::spawn_app;
use tokio_tungstenite::connect_async;

mod helpers;

#[tokio::test]
async fn server_requests_upgrades_from_client() {
    let test_application = spawn_app().await;

    let response = test_application
        .api_client
        .get(format!(
            "http://127.0.0.1:{}",
            test_application.server.port()
        ))
        .send()
        .await
        .expect("Failed to execute request");

    assert_eq!(response.status().as_u16(), 426);

    let headers = response.headers();

    assert_eq!(headers.get("Connection").unwrap(), "upgrade");
    assert_eq!(headers.get("Upgrade").unwrap(), "websocket");
}

#[tokio::test]
async fn client_can_connect_successfully() {
    let test_application = spawn_app().await;

    let (_, _) = connect_async(format!("ws://localhost:{}", test_application.server.port()))
        .await
        .expect("Failed to connect");

    assert_eq!(test_application.server.clients_len().await, 1);
}
