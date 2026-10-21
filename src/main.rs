use clap::{Parser, Subcommand};
use occp_proxy::{Config, CopilotProxy, DeviceFlowAuth, GitHubDomain, TokenStorage};
use pingora::prelude::*;
use pingora::proxy::http_proxy_service;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Parser)]
#[command(name = "occp-proxy")]
#[command(about = "GitHub Copilot authentication and API proxy server")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Address to bind to
        #[arg(short, long, default_value = "127.0.0.1")]
        bind: String,

        /// API key for proxy authentication
        #[arg(long, env = "OCCP_PROXY_API_KEY")]
        api_key: Option<String>,
    },

    /// Authenticate with GitHub Copilot
    Login {
        /// GitHub Enterprise domain (omit for github.com)
        #[arg(value_name = "DOMAIN")]
        domain: Option<String>,
    },

    /// Remove saved authentication
    Logout,

    /// Show current authentication status
    Status,

    /// Show configuration directory
    Config,
}

fn init_logging(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level)).init();
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    match cli.command {
        Commands::Serve {
            port,
            bind,
            api_key,
        } => {
            // Run synchronously - Pingora manages its own runtime
            cmd_serve(port, bind, api_key)?;
        }
        Commands::Login { domain } => {
            // Only login needs async runtime for OAuth flow
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(cmd_login(domain))?;
        }
        Commands::Logout => {
            cmd_logout()?;
        }
        Commands::Status => {
            cmd_status()?;
        }
        Commands::Config => {
            cmd_config()?;
        }
    }

    Ok(())
}

fn cmd_serve(port: u16, bind: String, api_key: Option<String>) -> anyhow::Result<()> {
    let storage = TokenStorage::new()?;

    // Load auth state
    let auth_state = storage.load()?;
    if auth_state.is_none() {
        eprintln!("Not authenticated. Run 'occp-proxy login' first.");
        std::process::exit(1);
    }

    let config = Config {
        bind_address: bind.clone(),
        port,
        api_key,
    };

    let auth_state = Arc::new(RwLock::new(auth_state));
    let proxy = CopilotProxy::new(auth_state, config)?;

    // Create Pingora server
    let mut server = Server::new(None)?;
    server.bootstrap();

    let mut service = http_proxy_service(&server.configuration, proxy);
    let addr = format!("{}:{}", bind, port);
    service.add_tcp(&addr);

    log::info!("Starting proxy server on {}", addr);

    server.add_service(service);
    server.run_forever();
}

async fn cmd_login(domain: Option<String>) -> anyhow::Result<()> {
    let storage = TokenStorage::new()?;

    // Check if already authenticated
    if let Some(existing) = storage.load()? {
        println!(
            "Already authenticated with {}",
            existing.domain.auth_domain()
        );
        println!("Run 'occp-proxy logout' first to re-authenticate.");
        return Ok(());
    }

    let github_domain = match domain {
        Some(d) => {
            // Normalize domain (remove protocol and trailing slash)
            let normalized = d
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/')
                .to_string();
            GitHubDomain::Enterprise { domain: normalized }
        }
        None => GitHubDomain::Public,
    };

    let auth_flow = DeviceFlowAuth::new(github_domain);
    let auth_state = auth_flow.authenticate().await?;

    storage.save(&auth_state)?;

    println!("\n=== Authentication Successful ===");
    println!("Domain: {}", auth_state.domain.auth_domain());
    println!("API URL: {}", auth_state.domain.api_base_url());
    println!("\nYou can now start the proxy server with 'occp-proxy serve'");

    Ok(())
}

fn cmd_logout() -> anyhow::Result<()> {
    let storage = TokenStorage::new()?;
    storage.clear()?;
    println!("Authentication cleared.");
    Ok(())
}

fn cmd_status() -> anyhow::Result<()> {
    let storage = TokenStorage::new()?;

    match storage.load()? {
        Some(auth) => {
            println!("=== Authentication Status ===");
            println!("Authenticated: Yes");
            println!("Domain: {}", auth.domain.auth_domain());
            println!("API URL: {}", auth.domain.api_base_url());
            println!("Authenticated at: {}", auth.authenticated_at);
        }
        None => {
            println!("Not authenticated.");
            println!("Run 'occp-proxy login' to authenticate.");
        }
    }

    Ok(())
}

fn cmd_config() -> anyhow::Result<()> {
    let storage = TokenStorage::new()?;
    println!(
        "Configuration directory: {}",
        storage.config_dir().display()
    );
    Ok(())
}
