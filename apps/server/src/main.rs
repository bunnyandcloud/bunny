mod api;
mod cdp_collector;
mod cli;
mod middleware;
mod novnc_proxy;
mod preview;
mod realtime;
mod recovery;
mod secrets_cli;
mod secrets_ops;
mod state;
mod terminals;
mod web_ui;
mod webrtc;
mod ws;

use anyhow::Result;
use bunny_core::config::BunnyConfig;
use clap::Parser;
use cli::Cli;
use state::AppState;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("bunny=info".parse()?))
        .init();

    let cli = Cli::parse();
    let config = load_config()?;
    let state = Arc::new(AppState::new(config)?);

    match cli.command {
        cli::Commands::Configure(opts) => cli::run_configure(&state, opts).await,
        cli::Commands::InitAuth => cli::run_init_auth(&state).await,
        cli::Commands::AuthStatus => cli::run_auth_status(&state).await,
        cli::Commands::User { command } => cli::run_user(&state, command).await,
        cli::Commands::Start(opts) => cli::run_start(state, opts).await,
        cli::Commands::Run(opts) => cli::run_run(state, opts).await,
        cli::Commands::Dev(opts) => cli::run_dev(state, opts).await,
        cli::Commands::Stop { session_id } => cli::run_stop(&state, session_id).await,
        cli::Commands::Doctor => cli::run_doctor().await,
        cli::Commands::Status => cli::run_status(&state).await,
        cli::Commands::Recover { session_id } => cli::run_recover(&state, session_id).await,
        cli::Commands::Reset { session_id } => cli::run_reset(&state, session_id).await,
        cli::Commands::Service { command } => cli::run_service(command).await,
        cli::Commands::Secrets(opts) => secrets_cli::run_secrets(&state, opts).await,
    }
}

fn load_config() -> Result<BunnyConfig> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let global = format!("{home}/.config/bunny/config.yaml");
    let paths: Vec<&str> = vec![&global, ".bunny.yaml"];
    BunnyConfig::load(&paths)
}
