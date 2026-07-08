//! Shared prompt templates and a process-wide, pre-parsed template environment.
//!
//! Templates are embedded at compile time with `include_str!` and registered
//! once into a `minijinja::Environment` behind a `LazyLock`. Callers on hot
//! paths (report generation, chat) render through [`render_report_gen`] /
//! [`render_agent_chat`] instead of rebuilding an `Environment` and re-parsing
//! the template string on every invocation.

use minijinja::Environment;
use serde::Serialize;
use std::sync::LazyLock;

/// Report-synthesis prompt: few-shot, with significance-scoring guidelines.
pub const REPORT_GEN_TEMPLATE: &str = include_str!("../templates/report_gen.jinja");

/// Agent / simulation chat prompt (all context sections are optional).
pub const AGENT_CHAT_TEMPLATE: &str = include_str!("../templates/agent_chat.jinja");

const REPORT_GEN_NAME: &str = "report_gen";
const AGENT_CHAT_NAME: &str = "agent_chat";

/// Environment holding the parsed templates. Built once on first use; the
/// templates are compile-time constants, so registration cannot fail at runtime.
static ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    env.add_template(REPORT_GEN_NAME, REPORT_GEN_TEMPLATE)
        .expect("report_gen.jinja is a compile-time constant and must parse");
    env.add_template(AGENT_CHAT_NAME, AGENT_CHAT_TEMPLATE)
        .expect("agent_chat.jinja is a compile-time constant and must parse");
    env
});

/// Render the report-generation prompt with `ctx`.
pub fn render_report_gen<S: Serialize>(ctx: S) -> std::result::Result<String, minijinja::Error> {
    ENV.get_template(REPORT_GEN_NAME)?.render(ctx)
}

/// Render the agent/simulation chat prompt with `ctx`.
pub fn render_agent_chat<S: Serialize>(ctx: S) -> std::result::Result<String, minijinja::Error> {
    ENV.get_template(AGENT_CHAT_NAME)?.render(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    #[test]
    fn test_report_gen_template_registered_and_renders() {
        let rendered = render_report_gen(context! {
            query => "Will they cooperate?",
            total_ticks => 10,
            agent_count => 2,
            total_events => 5,
            key_events => Vec::<serde_json::Value>::new(),
            agents => Vec::<serde_json::Value>::new(),
        })
        .expect("report_gen should render");

        // Guidance blocks unique to report_gen.jinja (not the old inline template).
        assert!(rendered.contains("Will they cooperate?"));
        assert!(rendered.contains("## Guidelines"));
        assert!(rendered.contains("## Example Output"));
    }

    #[test]
    fn test_agent_chat_template_registered_and_renders() {
        let rendered = render_agent_chat(context! {
            message => "Hello there",
        })
        .expect("agent_chat should render");

        assert!(rendered.contains("Hello there"));
        assert!(!rendered.contains("## Simulation Context"));
    }
}
