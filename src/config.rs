use crate::error::{Result, TeriError};
use serde::{Deserialize, Serialize};

/// All env-var-backed configuration for teri.
///
/// This struct is an extend-Y of the original teri `Config` with MiroFish `Config` fields merged
/// in (U-001 port).  All MiroFish fields are env-var-backed with the same env-var names and
/// same defaults as the Python source.  Nested sub-configs that teri already had are preserved
/// unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub sim: SimConfig,
    pub persistence: PersistenceConfig,
    pub api: ApiConfig,
    pub logging: LoggingConfig,
    /// MiroFish U-001: debug flag (env `FLASK_DEBUG`, default true).  In teri this drives
    /// optional verbose mode; the `FLASK_DEBUG` env name is preserved for source parity so that
    /// the same `.env` file works against both runtimes.
    pub debug: bool,
    /// MiroFish U-001: Zep API key (env `ZEP_API_KEY`, required by validate()).
    pub zep_api_key: Option<String>,
    /// MiroFish U-001: maximum HTTP upload body size in bytes (50 MB; not env-backed in source —
    /// constant 50 * 1024 * 1024).  Held as an inert field now; enforced by the axum server
    /// (U-002/U-003, not yet ported).
    pub max_content_length: u64,
    /// MiroFish U-001: filesystem path for uploaded files (env `UPLOAD_FOLDER`, default
    /// `"./uploads"`).  Inert until U-002/U-003 (axum file-upload handler) is ported.
    pub upload_folder: String,
    /// MiroFish U-001: allowed upload extensions set {pdf, md, txt, markdown}.  Inert until
    /// U-002/U-003; held as a sorted `Vec` because `HashSet` is not `Serialize + Clone` without
    /// extra derives.
    pub allowed_extensions: Vec<String>,
    /// MiroFish U-001: default text chunk size in characters (500; not env-backed in source).
    pub default_chunk_size: usize,
    /// MiroFish U-001: default text chunk overlap in characters (50; not env-backed in source).
    pub default_chunk_overlap: usize,
    /// MiroFish U-001 (S-015): OASIS max simulation rounds (env `OASIS_DEFAULT_MAX_ROUNDS`,
    /// default 10).
    pub oasis_default_max_rounds: u32,
    /// MiroFish U-001 (S-016): OASIS simulation data directory (env `OASIS_SIMULATION_DATA_DIR`,
    /// default `"./uploads/simulations"`).
    pub oasis_simulation_data_dir: String,
    /// MiroFish U-001 (S-017): OASIS Twitter action strings (fixed list, not env-backed).
    pub oasis_twitter_actions: Vec<String>,
    /// MiroFish U-001 (S-018): OASIS Reddit action strings (fixed list, not env-backed).
    pub oasis_reddit_actions: Vec<String>,
    /// MiroFish U-001 (S-019): Report-agent max tool calls (env `REPORT_AGENT_MAX_TOOL_CALLS`,
    /// default 5).
    pub report_agent_max_tool_calls: u32,
    /// MiroFish U-001 (S-020): Report-agent max reflection rounds (env
    /// `REPORT_AGENT_MAX_REFLECTION_ROUNDS`, default 2).
    pub report_agent_max_reflection_rounds: u32,
    /// MiroFish U-001 (S-021): Report-agent temperature (env `REPORT_AGENT_TEMPERATURE`,
    /// default 0.5).
    pub report_agent_temperature: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    /// Populated from env `LLM_MODEL_NAME` (MiroFish name) with fallback to `LLM_MODEL`
    /// (teri legacy name), then to default `"gpt-4o"`.
    ///
    /// **Default divergence note:** MiroFish defaults to `"gpt-4o-mini"` (config.py:33),
    /// teri defaults to `"gpt-4o"` (set by the architect).  teri's `"gpt-4o"` default is
    /// preserved because it was an explicit architect decision; MiroFish users wishing parity
    /// should set `LLM_MODEL_NAME=gpt-4o-mini` in their env.
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
        // MiroFish U-001 (S-017): Twitter action list — fixed, not env-backed.
        let oasis_twitter_actions = vec![
            "CREATE_POST".to_string(),
            "LIKE_POST".to_string(),
            "REPOST".to_string(),
            "FOLLOW".to_string(),
            "DO_NOTHING".to_string(),
            "QUOTE_POST".to_string(),
        ];

        // MiroFish U-001 (S-018): Reddit action list — fixed, not env-backed.
        // All 13 values preserved including TREND and REFRESH which are distinct downstream
        // (TREND survives FILTERED_ACTIONS and is recorded as an observable activity;
        // REFRESH is filtered — they cannot be collapsed).
        let oasis_reddit_actions = vec![
            "LIKE_POST".to_string(),
            "DISLIKE_POST".to_string(),
            "CREATE_POST".to_string(),
            "CREATE_COMMENT".to_string(),
            "LIKE_COMMENT".to_string(),
            "DISLIKE_COMMENT".to_string(),
            "SEARCH_POSTS".to_string(),
            "SEARCH_USER".to_string(),
            "TREND".to_string(),
            "REFRESH".to_string(),
            "DO_NOTHING".to_string(),
            "FOLLOW".to_string(),
            "MUTE".to_string(),
        ];

        // MiroFish U-001 (S-012): allowed upload extensions — fixed, not env-backed.
        let mut allowed_extensions = vec![
            "pdf".to_string(),
            "md".to_string(),
            "txt".to_string(),
            "markdown".to_string(),
        ];
        allowed_extensions.sort();

        // MiroFish U-001 (S-008 / S-007): model name — check LLM_MODEL_NAME (MiroFish env name)
        // first, then LLM_MODEL (teri legacy env name), then fall back to default.
        let model = std::env::var("LLM_MODEL_NAME")
            .or_else(|_| std::env::var("LLM_MODEL"))
            .unwrap_or_else(|_| "gpt-4o".to_string());

        Self {
            llm: LlmConfig {
                base_url: std::env::var("LLM_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
                api_key: api_key.unwrap_or_default().to_string(),
                model,
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
            // MiroFish U-001 fields below.
            debug: std::env::var("FLASK_DEBUG")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(true),
            zep_api_key: std::env::var("ZEP_API_KEY").ok(),
            // 50 MB — constant in MiroFish (50 * 1024 * 1024).
            max_content_length: 50 * 1024 * 1024,
            upload_folder: std::env::var("UPLOAD_FOLDER")
                .unwrap_or_else(|_| "./uploads".to_string()),
            allowed_extensions,
            default_chunk_size: 500,
            default_chunk_overlap: 50,
            oasis_default_max_rounds: std::env::var("OASIS_DEFAULT_MAX_ROUNDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            oasis_simulation_data_dir: std::env::var("OASIS_SIMULATION_DATA_DIR")
                .unwrap_or_else(|_| "./uploads/simulations".to_string()),
            oasis_twitter_actions,
            oasis_reddit_actions,
            report_agent_max_tool_calls: std::env::var("REPORT_AGENT_MAX_TOOL_CALLS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            report_agent_max_reflection_rounds: std::env::var(
                "REPORT_AGENT_MAX_REFLECTION_ROUNDS",
            )
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2),
            report_agent_temperature: std::env::var("REPORT_AGENT_TEMPERATURE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5),
        }
    }

    /// Validate configuration — called after load().
    ///
    /// Extended (U-001 port): now also validates `ZEP_API_KEY` to match MiroFish's contract.
    /// Returns `Err` on the first missing required field, preserving teri's existing short-circuit
    /// semantics.  For the MiroFish collect-all semantics (returns `Vec<String>` of all missing
    /// vars), use [`Config::validate_collect`].
    pub fn validate(&self) -> Result<()> {
        // Collect all errors first (matching MiroFish contract); then map to Err if non-empty.
        let errors = self.validate_collect();
        if !errors.is_empty() {
            return Err(TeriError::Config(errors.join("; ")));
        }

        if self.sim.default_agent_count == 0 {
            return Err(TeriError::Config("DEFAULT_AGENT_COUNT must be > 0".to_string()));
        }

        if self.sim.max_ticks == 0 {
            return Err(TeriError::Config("SIM_MAX_TICKS must be > 0".to_string()));
        }

        if self.sim.parallelism == 0 {
            return Err(TeriError::Config("SIM_PARALLELISM must be > 0".to_string()));
        }

        if self.api.bind_addr.trim().is_empty() {
            return Err(TeriError::Config("BIND_ADDR cannot be empty".to_string()));
        }

        if self.persistence.memory_db_path.trim().is_empty() {
            return Err(TeriError::Config("MEMORY_DB_PATH cannot be empty".to_string()));
        }

        if self.persistence.graph_db_path.trim().is_empty() {
            return Err(TeriError::Config("GRAPH_DB_PATH cannot be empty".to_string()));
        }

        Ok(())
    }

    /// MiroFish-parity validation (U-001 `Config.validate()` classmethod port).
    ///
    /// Collects ALL missing required variable errors into a `Vec<String>` and returns the list.
    /// An empty list means validation passed.  Non-empty means the caller should surface all
    /// errors and exit (code 1 in the MiroFish entrypoint, equivalent in teri's `run`/`serve`
    /// commands).
    ///
    /// Required vars: `LLM_API_KEY`, `ZEP_API_KEY`.
    ///
    /// This is the direct port of the Python `validate()` classmethod return contract.  The
    /// existing `validate()` method preserves teri's `Result<()>` API for callers already using
    /// it.
    pub fn validate_collect(&self) -> Vec<String> {
        let mut errors: Vec<String> = Vec::new();
        if self.llm.api_key.is_empty() {
            errors.push("LLM_API_KEY is not set".to_string());
        }
        if self.zep_api_key.as_deref().unwrap_or("").is_empty() {
            errors.push("ZEP_API_KEY is not set".to_string());
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Serializes tests that mutate the process-global environment (set_var/remove_var).
    // std::env mutation is process-wide; without this, parallel env-reading tests race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // --- S-004 / DEBUG field tests ---

    #[test]
    fn test_debug_default_is_true() {
        // FLASK_DEBUG not set → defaults to true (MiroFish config.py:25 default "True").
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("FLASK_DEBUG");
        unsafe { std::env::remove_var("FLASK_DEBUG") };
        let c = Config::build(Some("key"));
        assert!(c.debug, "debug should default to true when FLASK_DEBUG is unset");
        if let Ok(v) = prev {
            unsafe { std::env::set_var("FLASK_DEBUG", v) }
        }
    }

    #[test]
    fn test_debug_env_false() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("FLASK_DEBUG");
        unsafe { std::env::set_var("FLASK_DEBUG", "false") };
        let c = Config::build(Some("key"));
        assert!(!c.debug);
        match prev {
            Ok(v) => unsafe { std::env::set_var("FLASK_DEBUG", v) },
            Err(_) => unsafe { std::env::remove_var("FLASK_DEBUG") },
        }
    }

    // --- S-009 / ZEP_API_KEY tests ---

    #[test]
    fn test_zep_api_key_absent_gives_none() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("ZEP_API_KEY");
        unsafe { std::env::remove_var("ZEP_API_KEY") };
        let c = Config::build(Some("llm-key"));
        assert!(c.zep_api_key.is_none());
        if let Ok(v) = prev {
            unsafe { std::env::set_var("ZEP_API_KEY", v) }
        }
    }

    #[test]
    fn test_zep_api_key_env_set() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("ZEP_API_KEY");
        unsafe { std::env::set_var("ZEP_API_KEY", "zep-test-key") };
        let c = Config::build(Some("llm-key"));
        assert_eq!(c.zep_api_key.as_deref(), Some("zep-test-key"));
        match prev {
            Ok(v) => unsafe { std::env::set_var("ZEP_API_KEY", v) },
            Err(_) => unsafe { std::env::remove_var("ZEP_API_KEY") },
        }
    }

    // --- S-010 / MAX_CONTENT_LENGTH ---

    #[test]
    fn test_max_content_length_is_50mb() {
        let c = Config::build(Some("key"));
        assert_eq!(c.max_content_length, 50 * 1024 * 1024);
    }

    // --- S-011 / UPLOAD_FOLDER ---

    #[test]
    fn test_upload_folder_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("UPLOAD_FOLDER");
        unsafe { std::env::remove_var("UPLOAD_FOLDER") };
        let c = Config::build(Some("key"));
        assert_eq!(c.upload_folder, "./uploads");
        if let Ok(v) = prev {
            unsafe { std::env::set_var("UPLOAD_FOLDER", v) }
        }
    }

    #[test]
    fn test_upload_folder_env_override() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("UPLOAD_FOLDER");
        unsafe { std::env::set_var("UPLOAD_FOLDER", "/tmp/mirofish_uploads") };
        let c = Config::build(Some("key"));
        assert_eq!(c.upload_folder, "/tmp/mirofish_uploads");
        match prev {
            Ok(v) => unsafe { std::env::set_var("UPLOAD_FOLDER", v) },
            Err(_) => unsafe { std::env::remove_var("UPLOAD_FOLDER") },
        }
    }

    // --- S-012 / ALLOWED_EXTENSIONS ---

    #[test]
    fn test_allowed_extensions_exact_set() {
        let c = Config::build(Some("key"));
        let mut exts = c.allowed_extensions.clone();
        exts.sort();
        assert_eq!(exts, vec!["markdown", "md", "pdf", "txt"]);
    }

    // --- S-013 / DEFAULT_CHUNK_SIZE ---

    #[test]
    fn test_default_chunk_size_is_500() {
        let c = Config::build(Some("key"));
        assert_eq!(c.default_chunk_size, 500);
    }

    // --- S-014 / DEFAULT_CHUNK_OVERLAP ---

    #[test]
    fn test_default_chunk_overlap_is_50() {
        let c = Config::build(Some("key"));
        assert_eq!(c.default_chunk_overlap, 50);
    }

    // --- S-015 / OASIS_DEFAULT_MAX_ROUNDS ---

    #[test]
    fn test_oasis_max_rounds_default_10() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("OASIS_DEFAULT_MAX_ROUNDS");
        unsafe { std::env::remove_var("OASIS_DEFAULT_MAX_ROUNDS") };
        let c = Config::build(Some("key"));
        assert_eq!(c.oasis_default_max_rounds, 10);
        if let Ok(v) = prev {
            unsafe { std::env::set_var("OASIS_DEFAULT_MAX_ROUNDS", v) }
        }
    }

    #[test]
    fn test_oasis_max_rounds_env_override() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("OASIS_DEFAULT_MAX_ROUNDS");
        unsafe { std::env::set_var("OASIS_DEFAULT_MAX_ROUNDS", "25") };
        let c = Config::build(Some("key"));
        assert_eq!(c.oasis_default_max_rounds, 25);
        match prev {
            Ok(v) => unsafe { std::env::set_var("OASIS_DEFAULT_MAX_ROUNDS", v) },
            Err(_) => unsafe { std::env::remove_var("OASIS_DEFAULT_MAX_ROUNDS") },
        }
    }

    // --- S-016 / OASIS_SIMULATION_DATA_DIR ---

    #[test]
    fn test_oasis_simulation_data_dir_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("OASIS_SIMULATION_DATA_DIR");
        unsafe { std::env::remove_var("OASIS_SIMULATION_DATA_DIR") };
        let c = Config::build(Some("key"));
        assert_eq!(c.oasis_simulation_data_dir, "./uploads/simulations");
        if let Ok(v) = prev {
            unsafe { std::env::set_var("OASIS_SIMULATION_DATA_DIR", v) }
        }
    }

    // --- S-017 / OASIS_TWITTER_ACTIONS ---

    #[test]
    fn test_oasis_twitter_actions_exact_6() {
        let c = Config::build(Some("key"));
        assert_eq!(c.oasis_twitter_actions.len(), 6);
        assert!(c.oasis_twitter_actions.contains(&"CREATE_POST".to_string()));
        assert!(c.oasis_twitter_actions.contains(&"LIKE_POST".to_string()));
        assert!(c.oasis_twitter_actions.contains(&"REPOST".to_string()));
        assert!(c.oasis_twitter_actions.contains(&"FOLLOW".to_string()));
        assert!(c.oasis_twitter_actions.contains(&"DO_NOTHING".to_string()));
        assert!(c.oasis_twitter_actions.contains(&"QUOTE_POST".to_string()));
    }

    // --- S-018 / OASIS_REDDIT_ACTIONS ---

    #[test]
    fn test_oasis_reddit_actions_exact_13() {
        let c = Config::build(Some("key"));
        assert_eq!(c.oasis_reddit_actions.len(), 13);
        // All 13 preserved — including TREND and REFRESH as distinct entries.
        let expected = vec![
            "LIKE_POST",
            "DISLIKE_POST",
            "CREATE_POST",
            "CREATE_COMMENT",
            "LIKE_COMMENT",
            "DISLIKE_COMMENT",
            "SEARCH_POSTS",
            "SEARCH_USER",
            "TREND",
            "REFRESH",
            "DO_NOTHING",
            "FOLLOW",
            "MUTE",
        ];
        for action in &expected {
            assert!(
                c.oasis_reddit_actions.contains(&action.to_string()),
                "missing Reddit action: {action}"
            );
        }
        // Order is preserved (TREND before REFRESH, matching config.py:55-59).
        let trend_pos = c
            .oasis_reddit_actions
            .iter()
            .position(|a| a == "TREND")
            .unwrap();
        let refresh_pos = c
            .oasis_reddit_actions
            .iter()
            .position(|a| a == "REFRESH")
            .unwrap();
        assert!(
            trend_pos < refresh_pos,
            "TREND must precede REFRESH (source order preserved)"
        );
    }

    // --- S-019 / REPORT_AGENT_MAX_TOOL_CALLS ---

    #[test]
    fn test_report_agent_max_tool_calls_default_5() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("REPORT_AGENT_MAX_TOOL_CALLS");
        unsafe { std::env::remove_var("REPORT_AGENT_MAX_TOOL_CALLS") };
        let c = Config::build(Some("key"));
        assert_eq!(c.report_agent_max_tool_calls, 5);
        if let Ok(v) = prev {
            unsafe { std::env::set_var("REPORT_AGENT_MAX_TOOL_CALLS", v) }
        }
    }

    #[test]
    fn test_report_agent_max_tool_calls_env_override() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("REPORT_AGENT_MAX_TOOL_CALLS");
        unsafe { std::env::set_var("REPORT_AGENT_MAX_TOOL_CALLS", "10") };
        let c = Config::build(Some("key"));
        assert_eq!(c.report_agent_max_tool_calls, 10);
        match prev {
            Ok(v) => unsafe { std::env::set_var("REPORT_AGENT_MAX_TOOL_CALLS", v) },
            Err(_) => unsafe { std::env::remove_var("REPORT_AGENT_MAX_TOOL_CALLS") },
        }
    }

    // --- S-020 / REPORT_AGENT_MAX_REFLECTION_ROUNDS ---

    #[test]
    fn test_report_agent_max_reflection_rounds_default_2() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("REPORT_AGENT_MAX_REFLECTION_ROUNDS");
        unsafe { std::env::remove_var("REPORT_AGENT_MAX_REFLECTION_ROUNDS") };
        let c = Config::build(Some("key"));
        assert_eq!(c.report_agent_max_reflection_rounds, 2);
        if let Ok(v) = prev {
            unsafe { std::env::set_var("REPORT_AGENT_MAX_REFLECTION_ROUNDS", v) }
        }
    }

    // --- S-021 / REPORT_AGENT_TEMPERATURE ---

    #[test]
    fn test_report_agent_temperature_default_0_5() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("REPORT_AGENT_TEMPERATURE");
        unsafe { std::env::remove_var("REPORT_AGENT_TEMPERATURE") };
        let c = Config::build(Some("key"));
        assert!((c.report_agent_temperature - 0.5).abs() < 1e-9);
        if let Ok(v) = prev {
            unsafe { std::env::set_var("REPORT_AGENT_TEMPERATURE", v) }
        }
    }

    #[test]
    fn test_report_agent_temperature_env_override() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var("REPORT_AGENT_TEMPERATURE");
        unsafe { std::env::set_var("REPORT_AGENT_TEMPERATURE", "0.8") };
        let c = Config::build(Some("key"));
        assert!((c.report_agent_temperature - 0.8).abs() < 1e-9);
        match prev {
            Ok(v) => unsafe { std::env::set_var("REPORT_AGENT_TEMPERATURE", v) },
            Err(_) => unsafe { std::env::remove_var("REPORT_AGENT_TEMPERATURE") },
        }
    }

    // --- S-022 / validate_collect() tests (MiroFish contract: Vec<String>) ---

    #[test]
    fn test_validate_collect_both_missing() {
        // Both LLM_API_KEY (via empty api_key) and ZEP_API_KEY absent → 2 errors.
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_zep = std::env::var("ZEP_API_KEY");
        unsafe { std::env::remove_var("ZEP_API_KEY") };

        let mut c = Config::build(Some(""));
        c.zep_api_key = None;
        let errors = c.validate_collect();
        assert_eq!(errors.len(), 2, "expected 2 errors, got: {errors:?}");
        assert!(
            errors.iter().any(|e| e.contains("LLM_API_KEY")),
            "LLM_API_KEY error missing"
        );
        assert!(
            errors.iter().any(|e| e.contains("ZEP_API_KEY")),
            "ZEP_API_KEY error missing"
        );

        if let Ok(v) = prev_zep {
            unsafe { std::env::set_var("ZEP_API_KEY", v) }
        }
    }

    #[test]
    fn test_validate_collect_only_zep_missing() {
        let mut c = Config::build(Some("llm-key-present"));
        c.zep_api_key = None;
        let errors = c.validate_collect();
        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(errors[0].contains("ZEP_API_KEY"));
    }

    #[test]
    fn test_validate_collect_only_llm_missing() {
        let mut c = Config::build(Some(""));
        c.zep_api_key = Some("zep-key-present".to_string());
        let errors = c.validate_collect();
        assert_eq!(errors.len(), 1, "expected 1 error, got: {errors:?}");
        assert!(errors[0].contains("LLM_API_KEY"));
    }

    #[test]
    fn test_validate_collect_both_present_no_errors() {
        let mut c = Config::build(Some("llm-key"));
        c.zep_api_key = Some("zep-key".to_string());
        let errors = c.validate_collect();
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    // --- validate() integration: ZEP_API_KEY now enforced ---

    #[test]
    fn test_validate_returns_err_when_zep_missing() {
        let mut c = Config::build(Some("llm-key"));
        c.zep_api_key = None;
        let result = c.validate();
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("ZEP_API_KEY"), "error should mention ZEP_API_KEY: {msg}");
    }

    // --- LLM_MODEL_NAME alias test (S-008) ---

    #[test]
    fn test_llm_model_name_env_takes_precedence_over_llm_model() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_name = std::env::var("LLM_MODEL_NAME");
        let prev_model = std::env::var("LLM_MODEL");
        unsafe {
            std::env::set_var("LLM_MODEL_NAME", "gpt-4o-mini");
            std::env::set_var("LLM_MODEL", "gpt-4o");
        }
        let c = Config::build(Some("key"));
        assert_eq!(c.llm.model, "gpt-4o-mini", "LLM_MODEL_NAME should take precedence");
        match prev_name {
            Ok(v) => unsafe { std::env::set_var("LLM_MODEL_NAME", v) },
            Err(_) => unsafe { std::env::remove_var("LLM_MODEL_NAME") },
        }
        match prev_model {
            Ok(v) => unsafe { std::env::set_var("LLM_MODEL", v) },
            Err(_) => unsafe { std::env::remove_var("LLM_MODEL") },
        }
    }

    #[test]
    fn test_llm_model_fallback_when_model_name_absent() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_name = std::env::var("LLM_MODEL_NAME");
        let prev_model = std::env::var("LLM_MODEL");
        unsafe {
            std::env::remove_var("LLM_MODEL_NAME");
            std::env::set_var("LLM_MODEL", "my-custom-model");
        }
        let c = Config::build(Some("key"));
        assert_eq!(c.llm.model, "my-custom-model");
        match prev_name {
            Ok(v) => unsafe { std::env::set_var("LLM_MODEL_NAME", v) },
            Err(_) => unsafe { std::env::remove_var("LLM_MODEL_NAME") },
        }
        match prev_model {
            Ok(v) => unsafe { std::env::set_var("LLM_MODEL", v) },
            Err(_) => unsafe { std::env::remove_var("LLM_MODEL") },
        }
    }
}
