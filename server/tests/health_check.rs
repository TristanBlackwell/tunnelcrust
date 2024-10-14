use helpers::spawn_app;

mod helpers;

#[tokio::test]
async fn health_check_works() {
    let test_application = spawn_app().await;

    let response = test_application
        .api_client
        .get(format!(
            "http://127.0.0.1:{}/health-check",
            test_application.server.port()
        ))
        .send()
        .await
        .expect("Failed to execute request");

    assert!(response.status().is_success());

    let text = response
        .text()
        .await
        .expect("Failed to read text from response");

    assert_eq!(text, "Alive and kickin!");
}
