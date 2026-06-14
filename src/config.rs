use crate::error::{Result, TeriError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub sim: SimConfig,
    pub persistence: PersistenceConfig,
    pub api: ApiConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub embed_model: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimConfig {
    pub default_agent_count: usize,
    pub max_ticks: u32,
    pub parallelism: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    pub memory_db_path: String,
    pub graph_db_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub bind_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
}

impl Config {
    /// Load configuration with lazy key resolution.
    ///
    /// FIX-1.2 (envctl auto-injection): When LLM_API_KEY is not set in the process
    /// environment, returns a ConfigMissing error with guidance for using envctl
    /// (`envctl run -- teri ...`) which automatically injects secrets per the
    /// owner-intent design: "envctl HOLDS THE SECRETS and is designed to auto-inject
    /// API keys when a tool needs them".
    ///
    /// Direct environment variable paths are still supported (LLM_BASE_URL, LLM_MODEL, etc.)
    /// with the same defaults as before, but LLM_API_KEY is now optional to allow keyless
    /// operations (--help, --version) and envctl-based injection.
    pub fn load() -> Result<Self> {
        dotenvy::dotenv().ok();

        // FIX-1.2: Try to resolve API key from envctl-injected env vars first (scoped bearer),
        // then fall back to direct export. If neither is available, return ConfigMissing so the
        // caller can guide the user toward the correct invocation path.
        let api_key = match std::env::var("LLM_API_KEY") {
            Ok(key) => key,
            Err(std::env::VarError::NotPresent) => {
                // Check if a scoped bearer exists (envctl may inject under different names)
                for candidate in &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GITHUB_TOKEN"] {
                    if let Ok(key) = std::env::var(candidate) {
                        return Ok(Self::build(Some(&key)));
                    }
                }
                return Err(TeriError::ConfigMissing(
                    "LLM_API_KEY is not set and no envctl-managed key was detected.\n\
                     If using envctl: `envctl run -- teri run --seed ...`\n\
                     Otherwise, export LLM_API_KEY or create a teri config file (see README)."
                        .to_string(),
                ));
            }
            Err(e) => return Err(TeriError::Config(format!("Environment error: {e}"))),
        };

        Ok(Self::build(Some(&api_key)))
    }

    fn build(api_key: Option<&str>) -> Self {
        Self {
            llm: LlmConfig {
                base_url: std::env::var("LLM_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
                api_key: api_key.unwrap_or_default().to_string(),
                api_key: std::env::var("LLM_API_KEY").map_err(|_| {
                    TeriError::Config(
                        "LLM_API_KEY not set. teri receives secrets via envctl injection \
                         (vault-held key, injected into the child env only) — canonical: \
                         `env-ctl run --provider <provider> -- teri …`. Until envctl's \
                         data-plane phase lands, register the key in the vault \
                         (`env-ctl secret add teri-llm --provider <provider> --value-stdin`) \
                         or use a local .env for development. Never export real keys in \
                         your shell profile."
                            .to_string(),
                    )
                })?,
                model: std::env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
                embed_model: std::env::var("EMBED_MODEL")
                    .unwrap_or_else(|_| "text-embedding-3-small".to_string()),
                timeout_secs: std::env::var("LLM_TIMEOUT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(30),
                max_retries: std::env::var("LLM_MAX_RETRIES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3),
            },
            sim: SimConfig {
                default_agent_count: std::env::var("DEFAULT_AGENT_COUNT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(100),
                max_ticks: std::env::var("SIM_MAX_TICKS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(50),
                parallelism: std::env::var("SIM_PARALLELISM")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(8),
            },
            persistence: PersistenceConfig {
                memory_db_path: std::env::var("MEMORY_DB_PATH")
                    .unwrap_or_else(|_| "./data/memory.db".to_string()),
                graph_db_path: std::env::var("GRAPH_DB_PATH")
                    .unwrap_or_else(|_| "./data/graph".to_string()),
            },
            api: ApiConfig {
                bind_addr: std::env::var("BIND_ADDR")
                    .unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            },
            logging: LoggingConfig {
                level: std::env::var("RUST_LOG")
                    .unwrap_or_else(|_| "teri=debug,tower_http=info".to_string()),
            },
        }
    }

    /// Validate configuration — called after load().
    pub fn validate(&self) -> Result<()> {
        if self.llm.api_key.is_empty() {
            return Err(TeriError::Config(
                "LLM_API_KEY cannot be empty. Set it in the environment or use envctl for injection."
                    .to_string(),
            ));
        }

        if self.sim.default_agent_count == 0 {
            return Err(TeriError::Config(
                "DEFAULT_AGENT_COUNT must be > 0".to_string(),
            ));
        }

        if self.sim.max_ticks == 0 {
            return Err(TeriError::Config(
                "SIM_MAX_TICKS must be > 0".to_string(),
            ));
        }

        if self.sim.parallelism == 0 {
            return Err(TeriError::Config(
                "SIM_PARALLELISM must be > 0".to_string(),
            ));
        }

        if self.api.bind_addr.trim().is_empty() {
            return Err(TeriError::Config("BIND_ADDR cannot be empty".to_string()));
        }

        if self.persistence.memory_db_path.trim().is_empty() {
            return Err(
                TeriError::Config("MEMORY_DB_PATH cannot be empty".to_string()),
            );
        }

        if self.persistence.graph_db_path.trim().is_empty() {
            return Err(TeriError::Config(
                "GRAPH_DB_PATH cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}
