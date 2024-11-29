use rand::seq::SliceRandom;

// creates a string of two lowercase names separated
// by a dash.
pub fn generate_random_subdomain() -> String {
    let words = vec![
        "app", "api", "dev", "test", "stage", "prod", "beta", "demo", "lab", "srv", "web", "admin",
        "portal", "dash", "auth", "cdn", "cache", "proxy", "edge", "data", "db", "mail", "docs",
        "support", "internal", "metrics",
    ];

    let mut rng = rand::thread_rng();
    let subdomain_parts: Vec<&str> = words.choose_multiple(&mut rng, 2).copied().collect();

    subdomain_parts.join("-").to_lowercase()
}
