mod cli;
mod embed;
mod routes;
mod server;
mod jobs;
mod term_gen;
pub mod runner;
pub mod pack;
pub mod ai;
pub mod prompts;
pub mod docx_gen;
pub mod doc_theme;
pub mod solve;
pub mod linux;
pub mod sloppy;


use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    let is_debug = args.contains(&"--debug".to_string());
    
    let default_level = if is_debug { "debug" } else { "info" };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_level.into()),
        )
        .init();

    cli::run().await
}
