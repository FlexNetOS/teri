use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireKind {
    Repo,
    PullRequest,
    ExternalUrl,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WireSelection {
    RequiredAdd,
    SelectedOptional,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TeriSurface {
    Seed,
    Graph,
    Agent,
    Sim,
    Report,
    Api,
    Frontend,
    Backend,
    Docs,
    Observability,
    Optimizer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceWire {
    pub id: &'static str,
    pub kind: WireKind,
    pub selection: WireSelection,
    pub canonical_ref: &'static str,
    pub display_name: &'static str,
    pub license_status: &'static str,
    pub maturity_status: &'static str,
    pub teri_surfaces: &'static [TeriSurface],
    pub wire_role: &'static str,
    pub adoption_gate: &'static str,
    pub non_goals: &'static [&'static str],
    pub evidence_paths: &'static [&'static str],
    pub open_questions: &'static [&'static str],
}

pub const SOURCE_WIRES: &[SourceWire] = &[
    SourceWire {
        id: "brain_in_the_fish",
        kind: WireKind::Repo,
        selection: WireSelection::RequiredAdd,
        canonical_ref: "fabio-rovai/brain-in-the-fish",
        display_name: "Brain in the Fish",
        license_status: "FACT: MIT license in LICENSE.",
        maturity_status: "FACT: Rust workspace with CLI, MCP, tests, ontology and scoring modules present.",
        teri_surfaces: &[TeriSurface::Seed, TeriSurface::Report, TeriSurface::Docs],
        wire_role: "Evidence scoring, source-quote verification, ontology-backed claim gates, and report rubric vocabulary.",
        adoption_gate: "L2 registry + validation now; L3 only after an offline scorer seam proves source-quote verification without weakening the honesty guard.",
        non_goals: &[
            "No code import or runtime dependency in this PR.",
            "No claim that BITF verdicts are already executed inside Teri reports.",
        ],
        evidence_paths: &[
            "brain-in-the-fish/README.md",
            "brain-in-the-fish/LICENSE",
            "brain-in-the-fish/crates/core/src/scoring.rs",
            "brain-in-the-fish/crates/core/src/verify.rs",
            "brain-in-the-fish/crates/core/tests/pipeline_test.rs",
        ],
        open_questions: &[
            "QUESTION: Which Teri report seam should host future quote-verification penalties?",
            "QUESTION: Should ontology-backed claim gates run during report generation, seed intake, or both?",
        ],
    },
    SourceWire {
        id: "mirofish_guide",
        kind: WireKind::Repo,
        selection: WireSelection::RequiredAdd,
        canonical_ref: "MOZARTINOS/mirofish-guide",
        display_name: "MiroFish Guide",
        license_status: "FACT: MIT license in LICENSE.",
        maturity_status: "FACT: Operator playbook repo with SKILL.md and targeted references for workflow, debugging, report audit, evaluation, and model/proxy guidance.",
        teri_surfaces: &[
            TeriSurface::Seed,
            TeriSurface::Graph,
            TeriSurface::Agent,
            TeriSurface::Sim,
            TeriSurface::Report,
            TeriSurface::Docs,
        ],
        wire_role: "Stage-by-stage operator crosswalk from source material through report audit and runtime forensics.",
        adoption_gate: "L2 registry + validation now; L3 only after specific runbook checks are promoted into Teri scripts or tests.",
        non_goals: &[
            "Do not dump the whole guide into Teri.",
            "Do not treat guide observations as code-confirmed Teri facts without local verification.",
        ],
        evidence_paths: &[
            "mirofish-guide/README.md",
            "mirofish-guide/SKILL.md",
            "mirofish-guide/references/workflow.md",
            "mirofish-guide/references/debugging.md",
            "mirofish-guide/references/report-audit.md",
            "mirofish-guide/references/evaluation-rubric.md",
            "mirofish-guide/references/model-proxy-guidance.md",
        ],
        open_questions: &[
            "QUESTION: Which MiroFish operator checks should become future Teri automated gates?",
            "QUESTION: Which runtime-forensics heuristics map cleanly onto Teri's artifact layout?",
        ],
    },
    SourceWire {
        id: "mirofish_pr_325",
        kind: WireKind::PullRequest,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "666ghj/MiroFish#325",
        display_name: "MiroFish PR #325",
        license_status: "QUESTION: PR metadata does not establish a distinct license beyond the upstream repository.",
        maturity_status: "FACT: Open upstream delta, not merged truth; mergeStateStatus was DIRTY when inspected.",
        teri_surfaces: &[
            TeriSurface::Seed,
            TeriSurface::Graph,
            TeriSurface::Frontend,
            TeriSurface::Backend,
            TeriSurface::Docs,
        ],
        wire_role: "Adoption-gate source for Vertex/Gemini routing, CSV/XLSX ingestion, tabular-to-narrative conversion, English localization, and bug-class parity.",
        adoption_gate: "L1 documented crosswalk now; any L3+ adoption requires local implementation proof in Teri, not citation of an upstream open PR.",
        non_goals: &[
            "Do not copy code from the PR.",
            "Do not treat the PR as merged upstream behavior.",
        ],
        evidence_paths: &[
            "MiroFish#325 PR metadata",
            "MiroFish#325 changed file list",
            "MiroFish#325 summary and test plan",
        ],
        open_questions: &[
            "QUESTION: Which parts of CSV/XLSX ingestion belong in Teri seed intake versus frontend affordances?",
            "QUESTION: Does Teri need Vertex-style provider auth, or only provider-routing parity abstractions?",
        ],
    },
    SourceWire {
        id: "bettafish",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "666ghj/BettaFish",
        display_name: "BettaFish",
        license_status: "FACT: GPL-2.0 license text in LICENSE.",
        maturity_status: "FACT: Multi-agent public/private analysis stack with Query/Media/Insight/Report/Forum engines in the repo tree.",
        teri_surfaces: &[
            TeriSurface::Agent,
            TeriSurface::Sim,
            TeriSurface::Report,
            TeriSurface::Docs,
        ],
        wire_role: "Fish-lineage predecessor for agent forum coordination, public/private analysis, and report assembly questions.",
        adoption_gate: "L1 crosswalk only in this PR; any deeper adoption must prove parity or justified divergence in current Teri code.",
        non_goals: &[
            "Do not claim BettaFish behavior is already present in Teri unless local code proves it.",
            "Do not vendor its Python engine or model assets.",
        ],
        evidence_paths: &[
            "BettaFish/README.md",
            "BettaFish/README-EN.md",
            "BettaFish/LICENSE",
            "BettaFish/InsightEngine/",
            "BettaFish/ReportEngine/",
            "BettaFish/ForumEngine/",
        ],
        open_questions: &[
            "QUESTION: Which BettaFish forum-collaboration patterns still matter now that Teri centers MiroFish parity?",
            "QUESTION: Is there a future Teri seam for private-data analysis analogous to BettaFish InsightEngine?",
        ],
    },
    SourceWire {
        id: "inferrs",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "ericcurtin/inferrs",
        display_name: "inferrs",
        license_status: "FACT: Apache-2.0 license in LICENSE.",
        maturity_status: "FACT: LLM inference server with OpenAI-compatible, Anthropic-compatible, and Ollama-compatible APIs across multiple hardware backends.",
        teri_surfaces: &[TeriSurface::Backend, TeriSurface::Api, TeriSurface::Docs],
        wire_role: "Primary local-backend compatibility reference for OpenAI-compatible defaults, endpoint surfaces, and no-downgrade verification.",
        adoption_gate: "L2 registry + validation now; L3 optional offline fixtures only when Teri test surfaces can prove endpoint compatibility without live network dependencies.",
        non_goals: &[
            "Do not assume a local model is available in CI.",
            "Do not switch Teri defaults to an unverified route.",
        ],
        evidence_paths: &[
            "inferrs/README.md",
            "inferrs/LICENSE",
            "inferrs/inferrs/src/server.rs",
            "inferrs/tests/server_integration.rs",
            "Teri README.md inferrs verification section",
            "Teri RUNBOOK.md inferrs backend section",
        ],
        open_questions: &[
            "QUESTION: Should Teri future tests emulate inferrs OpenAI endpoints with local fixtures or richer mock servers?",
            "QUESTION: How should Teri expose backend compatibility matrices without overselling route reliability?",
        ],
    },
    SourceWire {
        id: "cellm",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "FlexNetOS/cellm",
        display_name: "cellm",
        license_status: "FACT: dual MIT or Apache-2.0 licensing via LICENSE-MIT and LICENSE-APACHE.",
        maturity_status: "FACT: research-stage mobile-native inference engine with scheduler, cache, SDK, WASM, benchmark docs, and mobile bindings.",
        teri_surfaces: &[TeriSurface::Backend, TeriSurface::Frontend, TeriSurface::Docs],
        wire_role: "First-party research lane for mobile/offline inference, paged KV cache, multi-session scheduling, and WebGPU/WASM feasibility.",
        adoption_gate: "L1/L2 only here; any L3+ use in Teri needs a tiny compile-safe seam and platform-specific proof outside this issue.",
        non_goals: &[
            "Do not add cellm as a dependency in this PR.",
            "Do not claim mobile runtime integration is already proven in Teri.",
        ],
        evidence_paths: &[
            "cellm/README.md",
            "cellm/docs/project_architecture.md",
            "cellm/docs/wasm-backend.md",
            "cellm/docs/benchmarks/README.md",
            "cellm/crates/cellm-scheduler/",
            "cellm/crates/cellm-wasm/",
        ],
        open_questions: &[
            "QUESTION: Is a future Teri mobile adapter better as a standalone service seam or an embedded runtime seam?",
            "QUESTION: Which cellm benchmark envelopes matter for Teri's report-heavy interactive workflows?",
        ],
    },
    SourceWire {
        id: "web_rwkv",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "cryscan/web-rwkv",
        display_name: "web-rwkv",
        license_status: "FACT: dual MIT or Apache-2.0 per the repository license notice.",
        maturity_status: "FACT: pure WebGPU RWKV inference engine with WASM support, examples, hooks, and browser runtime guidance.",
        teri_surfaces: &[TeriSurface::Frontend, TeriSurface::Backend, TeriSurface::Docs],
        wire_role: "Browser/offline inference feasibility wire for WebGPU and WASM execution.",
        adoption_gate: "L1/L2 now; L3 only after a bounded frontend experiment proves Teri can consume it safely.",
        non_goals: &[
            "Do not present web-rwkv as an OpenAI API server.",
            "Do not treat research/browser feasibility as production backend readiness.",
        ],
        evidence_paths: &[
            "web-rwkv/README.md",
            "web-rwkv/src/lib.rs",
            "web-rwkv/crates/web-rwkv-wasm/README.md",
            "web-rwkv/examples/chat.rs",
        ],
        open_questions: &[
            "QUESTION: Would a Teri frontend experiment consume RWKV through wasm directly or a separate bridge layer?",
            "QUESTION: How should Teri represent browser-only inference lanes without confusing them with current backend defaults?",
        ],
    },
    SourceWire {
        id: "cluaiz",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "cluaiz/cluaiz",
        display_name: "cluaiz",
        license_status: "FACT: Apache-2.0 license and README statement.",
        maturity_status: "FACT: marked Industrial Alpha / Research Phase, with OpenAI-compatible REST server, FFI-driven local orchestration, CEL, plugins, skills, and MCP flows.",
        teri_surfaces: &[TeriSurface::Backend, TeriSurface::Api, TeriSurface::Docs],
        wire_role: "Comparison point for local inference orchestration, OpenAI-compatible serving, native plugin/MCP design, and hardware-aware scheduling.",
        adoption_gate: "L2 registry + validation now; L3 only after a clearly bounded interoperability experiment.",
        non_goals: &[
            "Do not add install scripts or runtime dependencies from cluaiz in this PR.",
            "Do not overstate alpha-stage guarantees as stable production behavior.",
        ],
        evidence_paths: &[
            "cluaiz/README.md",
            "cluaiz/LICENSE",
            "cluaiz/docs/reference/terminal-commands.md",
            "cluaiz/docs/engine/api.md",
        ],
        open_questions: &[
            "QUESTION: Which cluaiz endpoint patterns are genuinely useful for Teri versus just parallel local-AI architecture ideas?",
            "QUESTION: Should future Teri endpoint comparisons include CEL/plugin execution contracts or stay inference-only?",
        ],
    },
    SourceWire {
        id: "splitrail",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "Piebald-AI/splitrail",
        display_name: "Splitrail",
        license_status: "FACT: MIT license in LICENSE.",
        maturity_status: "FACT: shipping token/cost tracker with MCP server, CLI, cloud upload option, and analyzers for multiple coding agents.",
        teri_surfaces: &[TeriSurface::Observability, TeriSurface::Docs],
        wire_role: "Optional observability reference for agent usage, token-cost telemetry, and MCP stats queries.",
        adoption_gate: "L1/L2 only in this PR; any telemetry integration must remain opt-in and local-first.",
        non_goals: &[
            "Do not configure cloud upload or API tokens in this PR.",
            "Do not upload Teri usage data automatically.",
        ],
        evidence_paths: &[
            "splitrail/README.md",
            "splitrail/LICENSE",
            "splitrail/src/analyzers/codex_cli.rs",
            "splitrail MCP server README section",
        ],
        open_questions: &[
            "QUESTION: Should Teri ever expose Splitrail-style local usage summaries, or only reference the tool externally?",
            "QUESTION: If telemetry is added later, which fields are safe to persist without leaking prompts or secrets?",
        ],
    },
    SourceWire {
        id: "openevolve",
        kind: WireKind::Repo,
        selection: WireSelection::SelectedOptional,
        canonical_ref: "algorithmicsuperintelligence/openevolve@80945ed82886d5c4ff2f3d22436765d50cb61266",
        display_name: "OpenEvolve",
        license_status: "FACT: Apache-2.0 license in LICENSE.",
        maturity_status: "FACT: evolutionary coding framework with evaluator-driven optimization, reproducibility claims, and many examples; not bounded to Teri's safety envelope.",
        teri_surfaces: &[TeriSurface::Optimizer, TeriSurface::Docs],
        wire_role: "Research-only optimizer reference for future evaluator-gated prompt, kernel, rubric, or policy experiments.",
        adoption_gate: "L0/L1 only in this PR; any future use must stay isolated, reproducible, benchmark-gated, and operator-approved.",
        non_goals: &[
            "Do not introduce autonomous code mutation into Teri.",
            "Do not run OpenEvolve as part of normal Teri validation.",
        ],
        evidence_paths: &[
            "openevolve README.md @ 80945ed82886d5c4ff2f3d22436765d50cb61266",
            "openevolve examples/README.md @ 80945ed82886d5c4ff2f3d22436765d50cb61266",
        ],
        open_questions: &[
            "QUESTION: Which Teri evaluation seams would be safe candidates for future optimizer experiments?",
            "QUESTION: How should any optimizer lane be isolated from default runtime and report paths?",
        ],
    },
    SourceWire {
        id: "tinystories_burn_charlm",
        kind: WireKind::Deferred,
        selection: WireSelection::Deferred,
        canonical_ref: "SHA888/tinystories-burn-charlm",
        display_name: "SHA888 tinystories-burn-charlm",
        license_status: "QUESTION: Deferred; not inspected in this issue.",
        maturity_status: "QUESTION: Deferred because the issue selected narrower source wires first.",
        teri_surfaces: &[],
        wire_role: "Deferred small-model research source.",
        adoption_gate: "Deferred: revisit only if a later issue asks for char-level or tiny-story model experiments.",
        non_goals: &["Not selected for issue 86."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &[
            "QUESTION: What concrete Teri surface would this improve beyond the selected backend/browser lanes?",
        ],
    },
    SourceWire {
        id: "orbisfati",
        kind: WireKind::Deferred,
        selection: WireSelection::Deferred,
        canonical_ref: "SHA888/orbisfati",
        display_name: "SHA888 orbisfati",
        license_status: "QUESTION: Deferred; not inspected in this issue.",
        maturity_status: "QUESTION: Deferred because the issue prioritized narrower selected sources.",
        teri_surfaces: &[],
        wire_role: "Deferred broad research source.",
        adoption_gate: "Deferred until a later issue asks for it with a concrete Teri target surface.",
        non_goals: &["Not selected for issue 86."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &["QUESTION: Which Teri seam would justify inspecting this repo later?"],
    },
    SourceWire {
        id: "kekawa",
        kind: WireKind::Deferred,
        selection: WireSelection::Deferred,
        canonical_ref: "SHA888/kekawa",
        display_name: "SHA888 kekawa",
        license_status: "QUESTION: Deferred; not inspected in this issue.",
        maturity_status: "QUESTION: Deferred because the issue prioritized more directly mapped sources.",
        teri_surfaces: &[],
        wire_role: "Deferred repo placeholder.",
        adoption_gate: "Deferred until it is tied to a specific acceptance gate.",
        non_goals: &["Not selected for issue 86."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &["QUESTION: What unique evidence would this add beyond selected sources?"],
    },
    SourceWire {
        id: "sha888_profile",
        kind: WireKind::ExternalUrl,
        selection: WireSelection::Deferred,
        canonical_ref: "https://github.com/SHA888",
        display_name: "SHA888 profile link",
        license_status: "QUESTION: Profile link, not a single repo license surface.",
        maturity_status: "QUESTION: Deferred because a broad profile is not a concrete source wire.",
        teri_surfaces: &[],
        wire_role: "Deferred broad-profile link.",
        adoption_gate: "Deferred unless narrowed to a specific repo with a specific Teri seam.",
        non_goals: &["Do not treat a profile page as a source wire."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &["QUESTION: Which concrete repo under this profile would matter later?"],
    },
    SourceWire {
        id: "arxiv_2603_16642v1",
        kind: WireKind::ExternalUrl,
        selection: WireSelection::Deferred,
        canonical_ref: "https://arxiv.org/html/2603.16642v1",
        display_name: "arXiv 2603.16642v1",
        license_status: "QUESTION: Deferred web paper; not inspected in this GitHub-scoped issue.",
        maturity_status: "QUESTION: Deferred non-GitHub source.",
        teri_surfaces: &[],
        wire_role: "Deferred paper reference.",
        adoption_gate: "Deferred until a later issue explicitly expands into paper/blog sourcing.",
        non_goals: &["Out of scope for this GitHub issue-driven wire."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &[
            "QUESTION: Does this paper map to a later Teri benchmark or research lane?",
        ],
    },
    SourceWire {
        id: "haqq_legal_ai_blog",
        kind: WireKind::ExternalUrl,
        selection: WireSelection::Deferred,
        canonical_ref: "https://haqq.ai/blog/legal-ai-72-agent-simulation-predictions",
        display_name: "HAQQ legal AI blog",
        license_status: "QUESTION: Deferred web article; not inspected in this issue.",
        maturity_status: "QUESTION: Deferred non-GitHub source.",
        teri_surfaces: &[],
        wire_role: "Deferred blog article.",
        adoption_gate: "Deferred until a later issue explicitly asks for blog/paper sourcing.",
        non_goals: &["Out of scope for this GitHub issue-driven wire."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &[
            "QUESTION: Is there any unique evidence here that a repo source would not cover?",
        ],
    },
    SourceWire {
        id: "amadad_mirofish",
        kind: WireKind::Deferred,
        selection: WireSelection::Deferred,
        canonical_ref: "amadad/mirofish",
        display_name: "amadad/mirofish",
        license_status: "QUESTION: Deferred fork; not inspected in this issue.",
        maturity_status: "QUESTION: Deferred because the issue selected upstream/source repos first.",
        teri_surfaces: &[],
        wire_role: "Deferred fork/demo repo.",
        adoption_gate: "Deferred unless it contains unique evidence missing from selected sources.",
        non_goals: &["Do not silently prefer a fork over the selected upstream sources."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &[
            "QUESTION: Does this fork contain any unique evidence that would justify later review?",
        ],
    },
    SourceWire {
        id: "intercom_gtm_mirofish_demo",
        kind: WireKind::Deferred,
        selection: WireSelection::Deferred,
        canonical_ref: "intercom/gtm-mirofish-demo",
        display_name: "intercom/gtm-mirofish-demo",
        license_status: "QUESTION: Deferred demo repo; not inspected in this issue.",
        maturity_status: "QUESTION: Deferred because demo repos were explicitly de-prioritized.",
        teri_surfaces: &[],
        wire_role: "Deferred demo reference.",
        adoption_gate: "Deferred unless a later issue proves it contains unique, necessary evidence.",
        non_goals: &["Not selected for issue 86."],
        evidence_paths: &["Issue 86 deferred source list"],
        open_questions: &[
            "QUESTION: Is there any demo-specific operator evidence worth revisiting later?",
        ],
    },
];

const REQUIRED_IDS: &[&str] = &["brain_in_the_fish", "mirofish_guide"];
const SELECTED_OPTIONAL_IDS: &[&str] = &[
    "mirofish_pr_325",
    "bettafish",
    "inferrs",
    "cellm",
    "web_rwkv",
    "cluaiz",
    "splitrail",
    "openevolve",
];

pub fn all_source_wires() -> &'static [SourceWire] {
    SOURCE_WIRES
}

pub fn active_source_wires() -> Vec<&'static SourceWire> {
    SOURCE_WIRES
        .iter()
        .filter(|wire| wire.selection != WireSelection::Deferred)
        .collect()
}

pub fn get_source_wire(id: &str) -> Option<&'static SourceWire> {
    SOURCE_WIRES.iter().find(|wire| wire.id == id)
}

pub fn validate_source_wires() -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();

    for wire in SOURCE_WIRES {
        if !ids.insert(wire.id) {
            errors.push(format!("duplicate wire id: {}", wire.id));
        }
        if wire.selection != WireSelection::Deferred {
            if wire.adoption_gate.trim().is_empty() {
                errors.push(format!("wire {} is missing an adoption gate", wire.id));
            }
            if wire.evidence_paths.is_empty() {
                errors.push(format!("wire {} has no evidence paths", wire.id));
            }
        }
    }

    for required in REQUIRED_IDS {
        if get_source_wire(required).is_none() {
            errors.push(format!("missing required wire: {required}"));
        }
    }

    for selected in SELECTED_OPTIONAL_IDS {
        if get_source_wire(selected).is_none() {
            errors.push(format!("missing selected optional wire: {selected}"));
        }
    }

    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

pub fn format_wire_list(include_deferred: bool) -> String {
    let wires = SOURCE_WIRES
        .iter()
        .filter(|wire| include_deferred || wire.selection != WireSelection::Deferred);
    let mut lines = Vec::new();
    for wire in wires {
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            wire.id,
            selection_name(wire.selection),
            wire.canonical_ref,
            wire.adoption_gate
        ));
    }
    lines.join("\n")
}

pub fn format_wire_details(wire: &SourceWire) -> String {
    let surfaces = wire.teri_surfaces.iter().map(surface_name).collect::<Vec<_>>().join(", ");
    let non_goals = if wire.non_goals.is_empty() {
        "- none".to_string()
    } else {
        wire.non_goals
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let evidence = wire
        .evidence_paths
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n");
    let questions = if wire.open_questions.is_empty() {
        "- none".to_string()
    } else {
        wire.open_questions
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "id: {}\ndisplay_name: {}\nkind: {}\nselection: {}\ncanonical_ref: {}\nlicense_status: {}\nmaturity_status: {}\nteri_surfaces: {}\nwire_role: {}\nadoption_gate: {}\nnon_goals:\n{}\nevidence_paths:\n{}\nopen_questions:\n{}",
        wire.id,
        wire.display_name,
        kind_name(wire.kind),
        selection_name(wire.selection),
        wire.canonical_ref,
        wire.license_status,
        wire.maturity_status,
        surfaces,
        wire.wire_role,
        wire.adoption_gate,
        non_goals,
        evidence,
        questions
    )
}

fn kind_name(kind: WireKind) -> &'static str {
    match kind {
        WireKind::Repo => "repo",
        WireKind::PullRequest => "pull_request",
        WireKind::ExternalUrl => "external_url",
        WireKind::Deferred => "deferred",
    }
}

fn selection_name(selection: WireSelection) -> &'static str {
    match selection {
        WireSelection::RequiredAdd => "required_add",
        WireSelection::SelectedOptional => "selected_optional",
        WireSelection::Deferred => "deferred",
    }
}

fn surface_name(surface: &TeriSurface) -> &'static str {
    match surface {
        TeriSurface::Seed => "seed",
        TeriSurface::Graph => "graph",
        TeriSurface::Agent => "agent",
        TeriSurface::Sim => "sim",
        TeriSurface::Report => "report",
        TeriSurface::Api => "api",
        TeriSurface::Frontend => "frontend",
        TeriSurface::Backend => "backend",
        TeriSurface::Docs => "docs",
        TeriSurface::Observability => "observability",
        TeriSurface::Optimizer => "optimizer",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_and_selected_wires_are_present() {
        validate_source_wires().expect("source wire registry should validate");
    }

    #[test]
    fn selected_wires_have_evidence_paths() {
        for wire in SOURCE_WIRES {
            if wire.selection != WireSelection::Deferred {
                assert!(
                    !wire.evidence_paths.is_empty(),
                    "wire {} should have evidence paths",
                    wire.id
                );
            }
        }
    }

    #[test]
    fn list_hides_deferred_by_default() {
        let list = format_wire_list(false);
        assert!(list.contains("brain_in_the_fish"));
        assert!(!list.contains("tinystories_burn_charlm"));
    }

    #[test]
    fn show_returns_expected_wire() {
        let wire = get_source_wire("splitrail").expect("splitrail wire exists");
        assert_eq!(wire.canonical_ref, "Piebald-AI/splitrail");
        assert!(format_wire_details(wire).contains("observability"));
    }
}
