use server::{configuration::get_configuration, server::Server};
use std::sync::Arc;

pub struct TestApplication {
    pub server: Arc<Server>,
    pub api_client: reqwest::Client,
}

pub async fn spawn_app() -> TestApplication {
    let configuration = {
        let mut config = get_configuration().expect("Failed to read configuration");
        config.port = 0;

        config
    };

    let server = Arc::new(
        Server::build(configuration)
            .await
            .expect("Failed to build server"),
    );

    let server_clone = server.clone();
    // Spawn the server on a background task
    tokio::spawn(async move {
        server_clone.run_until_stopped().await;
    });

    TestApplication {
        server: server.clone(),
        api_client: reqwest::Client::new(),
    }
}
