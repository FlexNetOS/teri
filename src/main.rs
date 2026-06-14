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
        #[arg(short, long, default_value = "0.0.0.0:8080")]
        addr: String,
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

    // FIX-1.3: GGUF/stub backend guard — preflight before sim run.
    let is_stub = teri::preflight_check_backend(&config.llm).await.map_err(|e| {
        TeriError::Config(format!("Backend probe failed: {e}"))
    })?;
    if is_stub {
        return Err(TeriError::Config(
            "GGUF/stub backend detected — simulation would produce canned text, not intelligence.\n\
             Set a real LLM endpoint (LLM_BASE_URL with an OpenAI-compatible API) to run simulations."
                .to_string(),
        ));
    }

    init_logging(&config.logging.level)?;

    // Create data directories for persistence layer
    let memory_dir = std::path::Path::new(&config.persistence.memory_db_path)
        .parent()
        .ok_or_else(|| TeriError::Config("Invalid memory DB path".to_string()))?;
    std::fs::create_dir_all(memory_dir)
        .map_err(|e| TeriError::Config(format!("Failed to create memory dir: {e}")))?;

    let graph_dir = std::path::Path::new(&config.persistence.graph_db_path);
    std::fs::create_dir_all(graph_dir)
        .map_err(|e| TeriError::Config(format!("Failed to create graph dir: {e}")))?;

    tracing::info!(
        "Starting simulation: seed={seed}, agents={agents}, query={query}"
    );
    tracing::info!("Query: {query}");
    tracing::info!("Configuration loaded successfully");
    Err(TeriError::Unknown("Pipeline not yet implemented".to_string()))
}

async fn serve_cmd() -> Result<()> {
    let cli = Cli::parse();
    let Commands::Serve { addr } = cli.command else { unreachable!() };

    let config = Config::load().map_err(|e| {
        TeriError::Config(format!("Configuration error: {e}"))
    })?;

    init_logging(&config.logging.level)?;

    tracing::info!("Starting API server on {addr}");
    Err(TeriError::Unknown("API server not yet implemented".to_string()))
}
