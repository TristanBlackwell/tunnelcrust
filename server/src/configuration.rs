#[derive(serde::Deserialize)]
pub struct Settings {
    pub port: u16,
}

/// Reads the configuration for the application from a `/configuration/settings.yaml` file.
pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    let base_path = std::env::current_dir().expect("Failed to determine the current directory");
    let configuration_directory = base_path.join("configuration");

    let settings = config::Config::builder()
        .add_source(config::File::from(
            configuration_directory.join("settings.yaml"),
        ))
        .add_source(
            config::Environment::with_prefix("TUNNELCRUST")
                .prefix_separator("_")
                .separator("_"),
        )
        .build()?;

    settings.try_deserialize::<Settings>()
}
