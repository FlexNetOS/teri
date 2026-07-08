//! `ComputeWorld` — the **execution-effect digital twin** (world-type #2 on teri's `SimEngine`).
//!
//! ## What it is (the real-to-sim-to-real move, applied to digital execution)
//! teri's `SocialWorld` predicts *social* outcomes. `ComputeWorld` is its sibling overlay
//! (installed via [`crate::sim::SimEngine::with_compute`], mirroring `with_social`): a typed,
//! provenance-tracked **graph twin** of a compute environment, where an action is a command
//! and [`ComputeWorld::apply`] **deduces the command's effect before it runs** — filesystem
//! mutations, likely exit, risk — with provenance + confidence. It does **not** execute anything.
//!
//! This is what replaces sandbox/VM/docker: you *predict* the effect in the twin first; a real
//! hermetic cell only executes the predicted-safe path; real outcomes feed back to calibrate the
//! predictor (closing the "reality gap," exactly as teri's social calibration loop already does).
//!
//! ## Method (Sherlock deduction = the prediction method)
//! `deduce_effect` is a rule-based **baseline** deducer: observe the command + world state →
//! deduce the effect from a rulebook → attach a `rationale` (the deduction, "because …") and a
//! `confidence`. Well-known patterns deduce with high confidence; an unknown command deduces
//! **low** confidence and says so — no fabrication (teri's honesty-guard doctrine). A learned /
//! LLM / worldgraph-OccWorld predictor layers on top of this baseline later; the baseline is the
//! part that does not change regardless of the model.
//!
//! ## Lineage
//! - Graph + provenance model: `ruvnet/worldgraph` (typed property graph + `SemanticProvenance`
//!   = evidence + model + calibration on every belief).
//! - Constraints-not-advice output: RFC-001 DevWorld ("simulation writes constraints, not advice";
//!   failed trajectories become failure capsules — "failure is terrain").

use std::collections::BTreeMap;
use std::path::PathBuf;

use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

/// A node in the compute twin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputeNode {
    /// A file (predicted to exist after the effect, or observed).
    File { path: String, exists: bool },
    /// A directory.
    Dir { path: String, exists: bool },
    /// A process the command spawns.
    Process { command: String },
    /// An environment variable.
    EnvVar { key: String },
}

/// A typed relation edge in the twin (what the command does to a node).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputeRelation {
    Reads,
    Writes,
    Creates,
    Deletes,
    Spawns,
    Mutates,
}

/// Filesystem kind for a create.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsKind {
    File,
    Dir,
}

/// One predicted filesystem change (the concrete, checkable prediction).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsChange {
    Create { path: String, kind: FsKind },
    Write { path: String },
    Delete { path: String },
    Read { path: String },
}

/// The predicted process outcome.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExitPrediction {
    /// Deduced to exit 0.
    Success,
    /// Deduced to fail, with the deduced reason.
    Failure { reason: String },
    /// Effect is not deducible from the baseline rulebook (defer to a learned predictor / real run).
    Unknown,
}

/// Reversibility/blast-radius of the action — feeds the execution gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Risk {
    Low,
    Medium,
    High,
    /// Irreversible (e.g. `rm -rf`) — the gate must require explicit authority.
    Irreversible,
}

/// `worldgraph`-style `SemanticProvenance`: every predicted effect carries where it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectProvenance {
    /// The evidence the deduction rests on (matched tokens, world-state facts).
    pub evidence: Vec<String>,
    /// The model that produced the prediction (here: the deterministic baseline rulebook).
    pub model: String,
    /// Calibration weight in `[0, ∞)`, updated by the reality-gap loop (starts neutral at 1.0).
    pub calibration: f32,
}

/// A command action whose effect the twin **deduces** (never executes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeAction {
    pub command: String,
    pub cwd: String,
}

/// A deduced effect — the prediction, not the execution. This is the `SimulationReport`
/// row a `ComputeWorld` rollout emits (constraints, not advice — RFC-001).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PredictedEffect {
    pub command: String,
    pub fs_changes: Vec<FsChange>,
    pub predicted_exit: ExitPrediction,
    pub risk: Risk,
    /// Deductive certainty in `[0.0, 1.0]`.
    pub confidence: f32,
    /// The Sherlock deduction in words ("mkdir creates a directory node; exits 0 unless it exists").
    pub rationale: String,
    pub provenance: EffectProvenance,
}

impl PredictedEffect {
    /// True when the effect mutates the filesystem (a create/write/delete is predicted).
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        self.fs_changes
            .iter()
            .any(|c| !matches!(c, FsChange::Read { .. }))
    }
}

/// The compute twin: a typed, provenance-tracked graph of a compute environment.
///
/// Mirrors `SocialWorld`. The graph is the durable structure (worldgraph-style); `index` maps a
/// path to its node for O(log n) lookup during deduction.
#[derive(Debug)]
pub struct ComputeWorld {
    pub root: PathBuf,
    graph: DiGraph<ComputeNode, ComputeRelation>,
    index: BTreeMap<String, NodeIndex>,
}

impl ComputeWorld {
    /// A fresh twin rooted at `root`.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            graph: DiGraph::new(),
            index: BTreeMap::new(),
        }
    }

    /// **Deduce** the effect of `action` and record its predicted mutations into the twin graph.
    /// Does NOT run the command.
    pub fn apply(&mut self, action: &ComputeAction) -> PredictedEffect {
        let effect = deduce_effect(&action.command);
        self.record(&effect);
        effect
    }

    /// Fold a deduced effect's changes into the twin graph (predicted nodes + relation edges).
    fn record(&mut self, effect: &PredictedEffect) {
        let proc = self.node(ComputeNode::Process {
            command: effect.command.clone(),
        });
        for change in &effect.fs_changes {
            let (path, node, rel) = match change {
                FsChange::Create { path, kind } => (
                    path.clone(),
                    match kind {
                        FsKind::File => ComputeNode::File { path: path.clone(), exists: true },
                        FsKind::Dir => ComputeNode::Dir { path: path.clone(), exists: true },
                    },
                    ComputeRelation::Creates,
                ),
                FsChange::Write { path } => (
                    path.clone(),
                    ComputeNode::File { path: path.clone(), exists: true },
                    ComputeRelation::Writes,
                ),
                FsChange::Delete { path } => (
                    path.clone(),
                    ComputeNode::File { path: path.clone(), exists: false },
                    ComputeRelation::Deletes,
                ),
                FsChange::Read { path } => (
                    path.clone(),
                    ComputeNode::File { path: path.clone(), exists: true },
                    ComputeRelation::Reads,
                ),
            };
            let target = self.node(node);
            self.graph.add_edge(proc, target, rel);
            let _ = path;
        }
    }

    /// Intern a node by its key (path/command), returning the existing index if present.
    fn node(&mut self, node: ComputeNode) -> NodeIndex {
        let key = node_key(&node);
        if let Some(&idx) = self.index.get(&key) {
            return idx;
        }
        let idx = self.graph.add_node(node);
        self.index.insert(key, idx);
        idx
    }

    /// Number of nodes in the twin (files/dirs/procs/env observed or predicted).
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Number of predicted/observed relation edges.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

/// A stable key for interning a node.
fn node_key(node: &ComputeNode) -> String {
    match node {
        ComputeNode::File { path, .. } => format!("file:{path}"),
        ComputeNode::Dir { path, .. } => format!("dir:{path}"),
        ComputeNode::Process { command } => format!("proc:{command}"),
        ComputeNode::EnvVar { key } => format!("env:{key}"),
    }
}

/// The deterministic **baseline deducer**. Observes the command tokens and deduces the effect
/// from a rulebook, attaching the deduction rationale + a confidence. Unknown → low confidence,
/// `Unknown` exit, no fabricated filesystem changes (honesty guard).
#[must_use]
pub fn deduce_effect(command: &str) -> PredictedEffect {
    let cmd = command.trim();
    let tokens: Vec<&str> = cmd.split_whitespace().collect();

    // Shell redirect (`> file` / `>> file`) is a write regardless of the program.
    let redirect_target = redirect_target(&tokens);

    let program = tokens.first().copied().unwrap_or("");
    let args: Vec<&str> = tokens
        .iter()
        .skip(1)
        .copied()
        .filter(|t| !t.starts_with('-') && *t != ">" && *t != ">>")
        .collect();

    let base = match program {
        "mkdir" => effect(
            cmd,
            args.iter().map(|p| FsChange::Create { path: (*p).to_string(), kind: FsKind::Dir }).collect(),
            ExitPrediction::Success,
            Risk::Low,
            0.9,
            "mkdir creates directory nodes; exits 0 unless a target already exists (without -p)",
            vec![format!("program=mkdir args={args:?}")],
        ),
        "touch" => effect(
            cmd,
            args.iter().map(|p| FsChange::Create { path: (*p).to_string(), kind: FsKind::File }).collect(),
            ExitPrediction::Success,
            Risk::Low,
            0.9,
            "touch creates (or updates) file nodes; exits 0 for a writable path",
            vec![format!("program=touch args={args:?}")],
        ),
        "rm" => {
            let irreversible = tokens.iter().any(|t| t.contains("r") && t.starts_with('-'))
                && tokens.iter().any(|t| t.contains('f') && t.starts_with('-'));
            effect(
                cmd,
                args.iter().map(|p| FsChange::Delete { path: (*p).to_string() }).collect(),
                ExitPrediction::Success,
                if irreversible { Risk::Irreversible } else { Risk::High },
                0.85,
                if irreversible {
                    "rm -rf recursively and forcibly deletes — IRREVERSIBLE; the gate must require explicit authority"
                } else {
                    "rm deletes file nodes; high blast radius"
                },
                vec![format!("program=rm irreversible={irreversible} args={args:?}")],
            )
        }
        "rmdir" => effect(
            cmd,
            args.iter().map(|p| FsChange::Delete { path: (*p).to_string() }).collect(),
            ExitPrediction::Success,
            Risk::Medium,
            0.8,
            "rmdir removes empty directory nodes; fails if non-empty",
            vec![format!("program=rmdir args={args:?}")],
        ),
        "mv" if args.len() >= 2 => {
            let src = args[0].to_string();
            let dst = args[args.len() - 1].to_string();
            effect(
                cmd,
                vec![FsChange::Delete { path: src }, FsChange::Create { path: dst, kind: FsKind::File }],
                ExitPrediction::Success,
                Risk::Medium,
                0.8,
                "mv deletes the source node and creates the destination node",
                vec![format!("program=mv args={args:?}")],
            )
        }
        "cp" if args.len() >= 2 => {
            let src = args[0].to_string();
            let dst = args[args.len() - 1].to_string();
            effect(
                cmd,
                vec![FsChange::Read { path: src }, FsChange::Create { path: dst, kind: FsKind::File }],
                ExitPrediction::Success,
                Risk::Low,
                0.8,
                "cp reads the source and creates the destination node",
                vec![format!("program=cp args={args:?}")],
            )
        }
        "cat" | "ls" | "head" | "tail" | "grep" | "find" | "wc" | "pwd" | "echo"
            if redirect_target.is_none() =>
        {
            effect(
                cmd,
                args.iter().map(|p| FsChange::Read { path: (*p).to_string() }).collect(),
                ExitPrediction::Success,
                Risk::Low,
                0.85,
                "read-only inspection command; no filesystem mutation predicted",
                vec![format!("program={program} readonly=true")],
            )
        }
        "true" => effect(cmd, vec![], ExitPrediction::Success, Risk::Low, 0.99, "`true` always exits 0", vec!["builtin=true".into()]),
        "false" => effect(
            cmd,
            vec![],
            ExitPrediction::Failure { reason: "`false` always exits non-zero".into() },
            Risk::Low,
            0.99,
            "`false` always exits 1",
            vec!["builtin=false".into()],
        ),
        "cargo" => effect(
            cmd,
            vec![FsChange::Read { path: "src".into() }, FsChange::Write { path: "target".into() }],
            ExitPrediction::Unknown,
            Risk::Low,
            0.5,
            "cargo reads sources and writes target/; exit is not deducible without building — a learned predictor or a real run resolves it",
            vec![format!("program=cargo sub={:?}", tokens.get(1))],
        ),
        _ => effect(
            cmd,
            vec![],
            ExitPrediction::Unknown,
            Risk::Medium,
            0.2,
            "unknown command — not deducible from the baseline rulebook; deferring to a learned predictor or real observation (no fabricated effect)",
            vec![format!("program={program:?} unknown=true")],
        ),
    };

    // A redirect writes its target regardless of the program above.
    if let Some(target) = redirect_target {
        let mut e = base;
        e.fs_changes.push(FsChange::Write { path: target.clone() });
        e.rationale = format!("{}; shell redirect writes `{}`", e.rationale, target);
        e.provenance.evidence.push(format!("redirect_target={target}"));
        e.confidence = e.confidence.max(0.6);
        return e;
    }
    base
}

/// Find a `>`/`>>` redirect target in the token stream, if any.
fn redirect_target(tokens: &[&str]) -> Option<String> {
    tokens
        .iter()
        .position(|t| *t == ">" || *t == ">>")
        .and_then(|i| tokens.get(i + 1))
        .map(|t| (*t).to_string())
}

/// Small constructor for a [`PredictedEffect`] with the baseline model tag + neutral calibration.
fn effect(
    command: &str,
    fs_changes: Vec<FsChange>,
    predicted_exit: ExitPrediction,
    risk: Risk,
    confidence: f32,
    rationale: &str,
    evidence: Vec<String>,
) -> PredictedEffect {
    PredictedEffect {
        command: command.to_string(),
        fs_changes,
        predicted_exit,
        risk,
        confidence,
        rationale: rationale.to_string(),
        provenance: EffectProvenance {
            evidence,
            model: "baseline-rulebook/v0".to_string(),
            calibration: 1.0,
        },
    }
}

/// The container installed via [`crate::sim::SimEngine::with_compute`], mirroring `SocialWorldSet`.
/// Keyed by a **cell/target id** (analogous to `SocialWorldSet`'s per-`Platform` keying).
#[derive(Debug, Default)]
pub struct ComputeWorldSet {
    worlds: Vec<(String, ComputeWorld)>,
}

impl ComputeWorldSet {
    /// Build a set with one twin per `(cell_id, root)` pair.
    #[must_use]
    pub fn new(cells: impl IntoIterator<Item = (String, PathBuf)>) -> Self {
        Self {
            worlds: cells.into_iter().map(|(id, root)| (id, ComputeWorld::new(root))).collect(),
        }
    }

    /// Mutable access to a cell's twin.
    pub fn world_mut(&mut self, cell: &str) -> Option<&mut ComputeWorld> {
        self.worlds.iter_mut().find(|(id, _)| id == cell).map(|(_, w)| w)
    }

    /// Shared access to a cell's twin.
    #[must_use]
    pub fn world(&self, cell: &str) -> Option<&ComputeWorld> {
        self.worlds.iter().find(|(id, _)| id == cell).map(|(_, w)| w)
    }

    /// The cell ids in the set.
    pub fn cells(&self) -> impl Iterator<Item = &str> + '_ {
        self.worlds.iter().map(|(id, _)| id.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(command: &str) -> ComputeAction {
        ComputeAction { command: command.to_string(), cwd: ".".to_string() }
    }

    #[test]
    fn mkdir_is_deduced_as_a_low_risk_dir_create() {
        let e = deduce_effect("mkdir -p src/sim");
        assert_eq!(e.fs_changes, vec![FsChange::Create { path: "src/sim".into(), kind: FsKind::Dir }]);
        assert_eq!(e.predicted_exit, ExitPrediction::Success);
        assert_eq!(e.risk, Risk::Low);
        assert!(e.confidence >= 0.9);
        assert!(e.is_mutating());
        assert!(!e.rationale.is_empty());
    }

    #[test]
    fn rm_rf_is_deduced_irreversible() {
        let e = deduce_effect("rm -rf build");
        assert_eq!(e.risk, Risk::Irreversible, "rm -rf must be flagged irreversible for the gate");
        assert_eq!(e.fs_changes, vec![FsChange::Delete { path: "build".into() }]);
    }

    #[test]
    fn read_only_command_predicts_no_mutation() {
        let e = deduce_effect("cat Cargo.toml");
        assert!(!e.is_mutating(), "cat is read-only");
        assert_eq!(e.fs_changes, vec![FsChange::Read { path: "Cargo.toml".into() }]);
    }

    #[test]
    fn redirect_is_deduced_as_a_write_regardless_of_program() {
        let e = deduce_effect("echo hello > out.txt");
        assert!(e.fs_changes.contains(&FsChange::Write { path: "out.txt".into() }));
        assert!(e.is_mutating());
    }

    #[test]
    fn unknown_command_is_low_confidence_and_not_fabricated() {
        // Honesty guard: the baseline must NOT invent an effect it cannot deduce.
        let e = deduce_effect("frobnicate --quux");
        assert_eq!(e.predicted_exit, ExitPrediction::Unknown);
        assert!(e.fs_changes.is_empty(), "no fabricated filesystem changes");
        assert!(e.confidence <= 0.3, "unknown => low confidence");
    }

    #[test]
    fn false_builtin_is_deduced_to_fail() {
        let e = deduce_effect("false");
        assert!(matches!(e.predicted_exit, ExitPrediction::Failure { .. }));
    }

    #[test]
    fn apply_records_predicted_mutations_into_the_twin_graph() {
        let mut world = ComputeWorld::new(PathBuf::from("/tmp/cell"));
        let e = world.apply(&act("mkdir data"));
        assert!(e.is_mutating());
        // one process node + one dir node, one Creates edge
        assert_eq!(world.node_count(), 2);
        assert_eq!(world.edge_count(), 1);
        // interning: applying the same action again does not duplicate nodes
        world.apply(&act("mkdir data"));
        assert_eq!(world.node_count(), 2, "nodes are interned by key");
    }

    #[test]
    fn world_set_keys_by_cell_id() {
        let mut set = ComputeWorldSet::new([("cell-a".to_string(), PathBuf::from("/a"))]);
        assert!(set.world("cell-a").is_some());
        assert!(set.world("missing").is_none());
        set.world_mut("cell-a").unwrap().apply(&act("touch x"));
        assert_eq!(set.world("cell-a").unwrap().node_count(), 2);
        assert_eq!(set.cells().collect::<Vec<_>>(), vec!["cell-a"]);
    }
}
