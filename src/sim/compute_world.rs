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

/// The twin's authoritative knowledge of one path (the oracle Holmesian elimination consults).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PathState {
    kind: FsKind,
    exists: bool,
}

/// The compute twin: a typed, provenance-tracked graph of a compute environment.
///
/// Mirrors `SocialWorld`. The graph is the durable provenance structure (worldgraph-style);
/// `index` maps a path to its node for O(log n) lookup; `state` is the queryable existence
/// oracle that Phase-2 elimination reasons over ("when you have eliminated the impossible…").
#[derive(Debug)]
pub struct ComputeWorld {
    pub root: PathBuf,
    graph: DiGraph<ComputeNode, ComputeRelation>,
    index: BTreeMap<String, NodeIndex>,
    state: BTreeMap<String, PathState>,
}

impl ComputeWorld {
    /// A fresh twin rooted at `root`.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            graph: DiGraph::new(),
            index: BTreeMap::new(),
            state: BTreeMap::new(),
        }
    }

    /// **Deduce** the effect of `action`, then **eliminate** impossible outcomes against the
    /// twin's known state, then record the (surviving) predicted mutations. Does NOT run anything.
    ///
    /// This is the deductive loop made literal: Observe (the command) → Deduce (the rulebook) →
    /// Eliminate (prune what the twin proves impossible) → record the truth that remains.
    pub fn apply(&mut self, action: &ComputeAction) -> PredictedEffect {
        let base = deduce_effect(&action.command);
        let effect = self.eliminate(base);
        self.record(&effect);
        effect
    }

    /// **Holmesian state elimination.** Consult the twin's `state` oracle to reject impossible
    /// outcomes and refine confidence. Positive knowledge only — an unknown path leaves the
    /// prediction untouched (honesty guard: eliminate the *impossible*, never invent facts).
    fn eliminate(&self, mut effect: PredictedEffect) -> PredictedEffect {
        for change in &effect.fs_changes {
            // A delete/read/copy-source requires its target to exist.
            let precondition_path = match change {
                FsChange::Delete { path } | FsChange::Read { path } => Some(path),
                _ => None,
            };
            let Some(path) = precondition_path else { continue };
            let Some(st) = self.state.get(path) else { continue };
            if st.exists {
                // Precondition confirmed present → the success path survives; nudge confidence up.
                effect.confidence = (effect.confidence + 0.05).min(0.99);
                effect.provenance.evidence.push(format!("twin_state:{path}=present"));
            } else {
                // The impossible is eliminated: the target is known-absent, so this cannot succeed.
                effect.predicted_exit = ExitPrediction::Failure {
                    reason: format!("`{path}` is known-absent in the twin"),
                };
                effect.confidence = effect.confidence.max(0.9);
                effect.rationale = format!(
                    "{}; ELIMINATED — `{}` was already deleted/absent in the twin, so the operation cannot succeed",
                    effect.rationale, path
                );
                effect.provenance.evidence.push(format!("twin_state:{path}=absent"));
                effect.provenance.model = format!("{}+holmesian-elimination", effect.provenance.model);
            }
        }
        effect
    }

    /// Fold a deduced effect into the twin: always record the *attempt* as provenance (nodes +
    /// relation edges), but update the authoritative `state` oracle only when the op is predicted
    /// to succeed — a predicted `Failure` (e.g. an eliminated delete) leaves reality unchanged.
    fn record(&mut self, effect: &PredictedEffect) {
        let proc = self.node(ComputeNode::Process {
            command: effect.command.clone(),
        });
        let succeeds = !matches!(effect.predicted_exit, ExitPrediction::Failure { .. });
        for change in &effect.fs_changes {
            let (node, rel) = match change {
                FsChange::Create { path, kind } => (
                    match kind {
                        FsKind::File => ComputeNode::File { path: path.clone(), exists: true },
                        FsKind::Dir => ComputeNode::Dir { path: path.clone(), exists: true },
                    },
                    ComputeRelation::Creates,
                ),
                FsChange::Write { path } => (
                    ComputeNode::File { path: path.clone(), exists: true },
                    ComputeRelation::Writes,
                ),
                FsChange::Delete { path } => (
                    ComputeNode::File { path: path.clone(), exists: false },
                    ComputeRelation::Deletes,
                ),
                FsChange::Read { path } => (
                    ComputeNode::File { path: path.clone(), exists: true },
                    ComputeRelation::Reads,
                ),
            };
            let target = self.node(node);
            self.graph.add_edge(proc, target, rel);

            if succeeds {
                match change {
                    FsChange::Create { path, kind } => {
                        self.state.insert(path.clone(), PathState { kind: *kind, exists: true });
                    }
                    FsChange::Write { path } => {
                        self.state.insert(path.clone(), PathState { kind: FsKind::File, exists: true });
                    }
                    FsChange::Delete { path } => {
                        let kind = self.state.get(path).map_or(FsKind::File, |s| s.kind);
                        self.state.insert(path.clone(), PathState { kind, exists: false });
                    }
                    FsChange::Read { .. } => {}
                }
            }
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
    fn deleting_a_known_absent_path_is_eliminated_to_failure() {
        // Holmes: eliminate the impossible. Create x, delete it, delete it again —
        // the second delete targets a path the twin KNOWS is gone, so it cannot succeed.
        let mut world = ComputeWorld::new(PathBuf::from("/tmp/cell"));
        world.apply(&act("touch x")); // x now present in the twin
        let first_rm = world.apply(&act("rm x"));
        assert_eq!(first_rm.predicted_exit, ExitPrediction::Success, "first rm succeeds");

        let second_rm = world.apply(&act("rm x"));
        assert!(
            matches!(second_rm.predicted_exit, ExitPrediction::Failure { .. }),
            "deleting a known-absent path must be eliminated to Failure"
        );
        assert!(second_rm.confidence >= 0.9, "elimination is high-confidence");
        assert!(second_rm.rationale.contains("ELIMINATED"));
        assert!(second_rm.provenance.model.contains("holmesian-elimination"));
    }

    #[test]
    fn deleting_a_known_present_path_survives_with_raised_confidence() {
        let mut world = ComputeWorld::new(PathBuf::from("/tmp/cell"));
        world.apply(&act("touch data.bin"));
        let rm = world.apply(&act("rm data.bin"));
        assert_eq!(rm.predicted_exit, ExitPrediction::Success);
        // baseline rm confidence is 0.85; a confirmed-present precondition nudges it up.
        assert!(rm.confidence > 0.85, "confirmed precondition raises confidence");
        assert!(rm.provenance.evidence.iter().any(|e| e.contains("present")));
    }

    #[test]
    fn reading_a_known_absent_path_is_eliminated() {
        let mut world = ComputeWorld::new(PathBuf::from("/tmp/cell"));
        world.apply(&act("touch note.txt"));
        world.apply(&act("rm note.txt"));
        let read = world.apply(&act("cat note.txt"));
        assert!(
            matches!(read.predicted_exit, ExitPrediction::Failure { .. }),
            "reading a known-absent path cannot succeed"
        );
    }

    #[test]
    fn elimination_does_not_fabricate_for_unknown_paths() {
        // Honesty guard: a path the twin has never seen must NOT be eliminated.
        let mut world = ComputeWorld::new(PathBuf::from("/tmp/cell"));
        let rm = world.apply(&act("rm never-seen.txt"));
        assert_eq!(
            rm.predicted_exit,
            ExitPrediction::Success,
            "unknown path stays at the baseline prediction; the impossible is not fabricated"
        );
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
