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
        /// FIX-2: write a verdict.json summary of the run to this path on completion.
        #[arg(short, long)]
        out: Option<String>,
    },
    /// Start the REST API server
    Serve {
        /// Bind address (e.g. "0.0.0.0:8080"). When not set, FLASK_HOST (default "0.0.0.0")
        /// and FLASK_PORT (default 5001) are used — faithful to MiroFish's env contract.
        #[arg(short, long)]
        addr: Option<String>,
    },
    /// Inspect the checked-in source wire registry
    Wires {
        #[command(subcommand)]
        command: WireCommands,
    },
}

#[derive(Subcommand)]
enum WireCommands {
    /// List wired sources
    List {
        #[arg(long)]
        include_deferred: bool,
        #[arg(long)]
        json: bool,
    },
    /// Show one wired source
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Validate the source wire registry
    Validate {
        #[arg(long)]
        json: bool,
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
        Commands::Wires { .. } => wires_cmd(),
    }
}

async fn run_cmd() -> Result<()> {
    // Lazy config load — only when actually needed (FIX-1.2: envctl injection seam).
    let cli = Cli::parse();
    let Commands::Run { seed, query, agents, out } = cli.command else { unreachable!() };

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

    // FIX-3: select the LLM adapter by config.llm.provider (openai-compatible / anthropic /
    // gemini). The whole in-process pipeline is monomorphized over this one concrete adapter.
    let llm = teri::api::build_provider_llm(&config);

    // FIX-1 (keystone): compose the full pipeline in-process —
    //   seed → graph build (real LLM extraction) → persona → sim (write-back) → report.
    // Reuses the exact service-layer functions the HTTP handlers call (see src/pipeline.rs).
    let outcome = teri::pipeline::run_pipeline(&config, llm, &seed, &query, agents).await?;

    tracing::info!(
        report_id = %outcome.report_id,
        graph_nodes = outcome.graph_node_count,
        graph_edges = outcome.graph_edge_count,
        agents = outcome.agents_generated,
        sim_status = outcome.sim_runner_status.as_str(),
        "Pipeline complete"
    );

    // FIX-2: write verdict.json summarizing the run.
    if let Some(out_path) = out {
        let verdict = outcome.to_verdict_json();
        let json = serde_json::to_string_pretty(&verdict)
            .map_err(|e| TeriError::Unknown(format!("failed to serialize verdict.json: {e}")))?;
        if let Some(parent) = std::path::Path::new(&out_path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                TeriError::Config(format!("failed to create verdict.json dir: {e}"))
            })?;
        }
        std::fs::write(&out_path, json.as_bytes())
            .map_err(|e| TeriError::Config(format!("failed to write verdict.json: {e}")))?;
        tracing::info!(out = %out_path, "Wrote verdict.json");
        println!("Wrote verdict.json to {out_path}");
    }

    println!(
        "teri run complete: report={} graph=({} nodes, {} edges) agents={} sim={}",
        outcome.report_id,
        outcome.graph_node_count,
        outcome.graph_edge_count,
        outcome.agents_generated,
        outcome.sim_runner_status.as_str()
    );
    Ok(())
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

fn wires_cmd() -> Result<()> {
    let cli = Cli::parse();
    let Commands::Wires { command } = cli.command else { unreachable!() };

    match command {
        WireCommands::List { include_deferred, json } => {
            if json {
                let wires: Vec<_> = teri::source_wires::all_source_wires()
                    .iter()
                    .filter(|wire| {
                        include_deferred
                            || wire.selection != teri::source_wires::WireSelection::Deferred
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&wires).map_err(|e| {
                        TeriError::Unknown(format!("failed to serialize source wires: {e}"))
                    })?
                );
            } else {
                println!("{}", teri::source_wires::format_wire_list(include_deferred));
            }
            Ok(())
        }
        WireCommands::Show { id, json } => {
            let wire = teri::source_wires::get_source_wire(&id)
                .ok_or_else(|| TeriError::Unknown(format!("unknown source wire id: {id}")))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(wire).map_err(|e| {
                        TeriError::Unknown(format!("failed to serialize source wire: {e}"))
                    })?
                );
            } else {
                println!("{}", teri::source_wires::format_wire_details(wire));
            }
            Ok(())
        }
        WireCommands::Validate { json } => match teri::source_wires::validate_source_wires() {
            Ok(()) => {
                if json {
                    println!(r#"{{"ok":true,"errors":[]}}"#);
                } else {
                    println!("source wire registry is valid");
                }
                Ok(())
            }
            Err(errors) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ok": false,
                            "errors": errors,
                        }))
                        .map_err(|e| {
                            TeriError::Unknown(format!(
                                "failed to serialize validation result: {e}"
                            ))
                        })?
                    );
                } else {
                    eprintln!("source wire registry is invalid:");
                    for error in errors {
                        eprintln!("- {error}");
                    }
                }
                Err(TeriError::Unknown("source wire registry validation failed".to_string()))
            }
        },
    }
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
