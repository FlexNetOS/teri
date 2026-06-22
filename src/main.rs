use clap::{Parser, Subcommand};
use teri::{Config, Result, TeriError, init_logging};

#[derive(Parser)]
#[command(name = "teri", version, about = "Swarm Intelligence Prediction Engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest seed material and launch a simulation world
    Run {
        #[arg(short, long)]
        seed: String,
        #[arg(short, long)]
        query: String,
        #[arg(short, long, default_value_t = 100)]
        agents: usize,
    },
    /// Start the REST API server
    Serve {
        /// Bind address (e.g. "0.0.0.0:8080"). When not set, FLASK_HOST (default "0.0.0.0")
        /// and FLASK_PORT (default 5001) are used — faithful to MiroFish's env contract.
        #[arg(short, long)]
        addr: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    // FIX-1.1: Parse CLI FIRST so --help/--version work keyless. Config is loaded lazily per-command.
    let cli = Cli::parse();

    match &cli.command {
        Commands::Run { .. } => run_cmd().await,
        Commands::Serve { .. } => serve_cmd().await,
    }
}

async fn run_cmd() -> Result<()> {
    // Lazy config load — only when actually needed (FIX-1.2: envctl injection seam).
    let cli = Cli::parse();
    let Commands::Run { seed, query, agents } = cli.command else { unreachable!() };

    let config = match Config::load() {
        Ok(c) => c,
        Err(e) if e.config_missing() => {
            // FIX-1.2: Friendly guidance toward envctl injection.
            eprintln!(
                "⚠️  teri: configuration unavailable — key may not be set.\n\
                 ℹ️  If using envctl: `envctl run -- teri run --seed ...`\n\
                 ℹ️  Otherwise set LLM_API_KEY or create a teri config file.\n\nError: {e}"
            );
            return Err(e);
        }
        Err(e) => return Err(e),
    };

    init_logging(&config.logging.level)?;

    // Backend honesty guard — fail-closed before any sim work. Refuses
    // unreachable backends, empty model lists, and canned stub output.
    preflight_backend(&config).await?;

    // Create data directories for persistence layer
    let memory_dir = std::path::Path::new(&config.persistence.memory_db_path)
        .parent()
        .ok_or_else(|| TeriError::Config("Invalid memory DB path".to_string()))?;
    std::fs::create_dir_all(memory_dir)
        .map_err(|e| TeriError::Config(format!("Failed to create memory dir: {e}")))?;

    let graph_dir = std::path::Path::new(&config.persistence.graph_db_path);
    std::fs::create_dir_all(graph_dir)
        .map_err(|e| TeriError::Config(format!("Failed to create graph dir: {e}")))?;

    tracing::info!("Starting simulation: seed={seed}, agents={agents}, query={query}");
    tracing::info!("Query: {query}");
    tracing::info!("Configuration loaded successfully");
    Err(TeriError::Unknown("Pipeline not yet implemented".to_string()))
}

async fn serve_cmd() -> Result<()> {
    let cli = Cli::parse();
    let Commands::Serve { addr } = cli.command else { unreachable!() };

    // FIX-1.2 style: friendly guidance toward envctl when config is missing.
    let config = match Config::load() {
        Ok(c) => c,
        Err(e) if e.config_missing() => {
            eprintln!(
                "teri: configuration unavailable — key may not be set.\n\
                 If using envctl: `envctl run -- teri serve`\n\
                 Otherwise set LLM_API_KEY or create a teri config file.\n\nError: {e}"
            );
            return Err(e);
        }
        Err(e) => return Err(e),
    };

    // init_logging once — process-global (faithful to create_app calling setup_logger
    // once at app-factory time; teri does it in the entrypoint instead).
    init_logging(&config.logging.level)?;

    // Backend honesty guard — serve refuses to BOOT against a stub/unreachable
    // backend. The API handlers drive the LLM, so a stub backend would make the
    // whole server fabricate output. Fail-closed, same guard as `run`.
    preflight_backend(&config).await?;

    // Delegate to teri::server::serve which carries the full U-002/U-003 logic.
    teri::server::serve(config, addr.as_deref()).await
}

/// Backend honesty guard shared by `run` and `serve`. Drives the configured
/// backend through `preflight::verify_backend` (GET /models → 1-token probe),
/// refusing unreachable backends, empty model lists, and canned stub output.
/// Never weakened — a swarm on canned text fabricates predictions, not insight.
async fn preflight_backend(config: &Config) -> Result<()> {
    let identity = teri::preflight::verify_backend(&config.llm).await?;
    tracing::info!(
        models = ?identity.models,
        "Backend honesty guard passed — real inference backend confirmed."
    );
    Ok(())
}
