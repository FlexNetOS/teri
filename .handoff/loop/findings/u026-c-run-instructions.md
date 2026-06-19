# DECISION-U026-2 — `RunInstructions` emits NATIVE teri run-guidance

**Unit:** U-026 sub-cycle (c) — `GET /api/simulation/:simulation_id` (`get_simulation`) +
the carry-forward gate logged on the U-023 ledger row for `get_run_instructions` (S-680).
**Class:** EXTEND-X (complete the partially-ported feature with teri-native guidance). NOT `[≠]`.
**Triggered by:** U-023 carry-forward gate (verbatim): *"when U-026 API route lands, teri MUST emit
NATIVE run-guidance (teri run/SimEngine::run), not just substrate_note, or the 'how to run'
contract downgrades."* The `get_simulation` handler embeds `run_instructions` when status==READY
(`simulation.py:771-772`), so the gate fires HERE.

---

## Decision

Replace the `substrate_note`-only `RunInstructions` with a NATIVE analog of Python's
`{simulation_dir, scripts_dir, config_file, commands{...}, instructions}`. teri's real native
run path in the SERVER context is the HTTP endpoint **`POST /api/simulation/{id}/start`** (U-026
sub-cycle g; `SimulationRunner::start_simulation`, `simulation_runner.rs:1054`), invoked in-process
against `SimEngine`. There is NO `teri run-simulation` CLI verb (main.rs only has `teri run` =
single-seed sim + `teri serve` = API server; neither runs a *prepared* simulation by id), so the
guidance MUST be the HTTP start call, not a shell command. Python's per-platform
`python run_*.py --config` strings become per-platform `POST /api/simulation/{id}/start` invocations
with the platform in the JSON body — the exact teri-native analog.

The `[≠]` is NARROWED to only: `scripts_dir` (teri has no scripts dir) and the literal
`python run_*.py`/`conda activate MiroFish` strings (genuinely inexpressible — no Python, no conda).
Everything else is now native-expressed.

---

## New `RunInstructions` shape (Rust struct)

```rust
/// S-680. Native teri run-guidance for a prepared (READY) simulation.
/// Python returned Python-script subprocess commands; teri returns the native
/// in-process invocation: POST /api/simulation/{id}/start (SimulationRunner→SimEngine).
#[derive(Debug, Clone)]
pub struct RunInstructions {
    /// Port of Python `"simulation_dir"`. Absolute path to the sim data dir.
    pub simulation_dir: PathBuf,

    /// Port of Python `"config_file"`. Path to simulation_config.json.
    pub config_file: PathBuf,

    /// NATIVE analog of Python `"commands"`. Per-platform native invocation strings.
    /// Keys: "twitter", "reddit", "parallel" (same three as Python).
    /// Values: the HTTP start call carrying that platform, e.g.
    ///   `POST /api/simulation/{id}/start  {"platform":"twitter"}`
    pub commands: RunCommands,

    /// NATIVE analog of Python `"instructions"`. Human-readable description of the
    /// in-process SimEngine run path (no conda, no scripts).
    pub instructions: String,

    /// RETAINED (folded, not dropped). The [≠]-substrate marker: documents that
    /// scripts_dir / python-script / conda commands are inexpressible in teri.
    /// Kept as a field so existing S-680 test coverage stays green and the gap
    /// stays self-documenting in the API payload.
    pub substrate_note: &'static str,
}

/// Per-platform native start invocations (mirrors Python commands{twitter,reddit,parallel}).
#[derive(Debug, Clone)]
pub struct RunCommands {
    pub twitter: String,
    pub reddit: String,
    pub parallel: String,
}
```

**Builder (`get_run_instructions`)** — additive change, same signature
`pub fn get_run_instructions(&self, simulation_id: &str) -> Result<RunInstructions>`:

```rust
let endpoint = format!("POST /api/simulation/{simulation_id}/start");
let mk = |platform: &str| format!(r#"{endpoint}  body: {{"platform":"{platform}"}}"#);
Ok(RunInstructions {
    simulation_dir: sim_dir,
    config_file,
    commands: RunCommands {
        twitter:  mk("twitter"),
        reddit:   mk("reddit"),
        parallel: mk("parallel"),
    },
    instructions: format!(
        "teri runs this prepared simulation in-process via SimEngine (no Python scripts, \
         no conda env). Start it through the running API server:\n\
         1. Ensure `teri serve` is running.\n\
         2. POST /api/simulation/{simulation_id}/start with JSON body \
         {{\"platform\": \"twitter\"|\"reddit\"|\"parallel\", \"max_rounds\": <opt int>, \
         \"enable_graph_memory_update\": <opt bool>, \"force\": <opt bool>}}.\n\
         The default platform is \"parallel\". The runner drives SimEngine directly; \
         no subprocess is spawned."
    ),
    substrate_note: "MiroFish's Python OASIS subprocess commands \
        (run_twitter_simulation.py, run_reddit_simulation.py, run_parallel_simulation.py) \
        and `conda activate MiroFish` are inexpressible in teri's substrate (no Python scripts, \
        no conda). teri runs the SimEngine in-process via the /start endpoint above.",
})
```

`max_rounds` / `enable_graph_memory_update` / `force` / `graph_id` are documented in `instructions`
(they are optional body params on the start route, `simulation.py:1502-1505`) — NOT separate struct
fields, because Python only exposed a flat per-platform command. Keeping the body params in prose
matches Python's `instructions`-string treatment and avoids inventing structure Python didn't have.

---

## `run_instructions` JSON key contract (for the parity gate)

When `state.status == READY`, the `get_simulation` handler sets `result["run_instructions"]` to a
`serde_json::Value::Object` built with `serde_json::Map` (the crate already uses the **preserve_order**
feature for `to_dict`, `simulation_manager.rs:301-303` — REUSE it, do NOT add `#[derive(Serialize)]`
with implicit ordering). Add a `to_dict(&self) -> Value` method on `RunInstructions` mirroring the
`SimulationState::to_dict` pattern. **Exact key names + order** (native analog of Python's
`{simulation_dir, scripts_dir, config_file, commands, instructions}`):

```text
run_instructions = {
  "simulation_dir": <abs path string>,            // PORTED (Python key, same name)
  "config_file":    <abs path string>,            // PORTED (Python key, same name)
  "commands": {                                   // NATIVE-EXPRESSED (Python key, native values)
      "twitter":  "POST /api/simulation/{id}/start  body: {\"platform\":\"twitter\"}",
      "reddit":   "POST /api/simulation/{id}/start  body: {\"platform\":\"reddit\"}",
      "parallel": "POST /api/simulation/{id}/start  body: {\"platform\":\"parallel\"}"
  },
  "instructions": <native run-path prose string>, // NATIVE-EXPRESSED (Python key, native value)
  "substrate_note": <[≠]-gap explanation string>  // NATIVE-ADDED (documents the scripts_dir/conda gap)
}
```

- Inner `commands` order: `twitter`, `reddit`, `parallel` — IDENTICAL to Python (`simulation.py`/`:517-521`).
- DROPPED key vs Python: **`scripts_dir`** — `[≠]`-substrate (teri has no scripts dir). This is the
  ONLY dropped key; the parity gate should accept its absence as a justified `[≠]`.
- Outer order: `simulation_dir, config_file, commands, instructions, substrate_note`. (Python order
  was `simulation_dir, scripts_dir, config_file, commands, instructions`; with `scripts_dir` removed
  and `substrate_note` appended, this is the native order.)

Gate assertion: GET a READY sim → response `data.run_instructions` is an object with exactly the 5
keys above (no `scripts_dir`); `commands` has exactly twitter/reddit/parallel; each command value
references `POST /api/simulation/{id}/start` and the correct platform; `instructions` mentions
SimEngine/in-process and is non-empty; `substrate_note` is non-empty and names the inexpressible
Python commands. For a non-READY sim, `data` has NO `run_instructions` key (matches `simulation.py:771`).

---

## Blast radius

`get_run_instructions` is a verified U-023 symbol; the change is **additive** (struct gains fields;
existing fields `simulation_dir`/`config_file`/`substrate_note` retained → all current callers still
compile). Grep-confirmed references (no production callers exist yet — the route layer is still
sub-cycle skeleton):

| Caller / referent | File:line | Kind | Impact |
|---|---|---|---|
| `get_run_instructions` def | `simulation_manager.rs:1614` | builder | EDIT: populate new `commands`/`instructions` fields; rewrite `substrate_note` (narrowed). |
| `RunInstructions` struct | `simulation_manager.rs:744` | type | EDIT: add `commands: RunCommands`, `instructions: String`; add `RunCommands` struct; add `to_dict`. |
| `get_run_instructions_structural_fields` test | `simulation_manager.rs:2032-2056` | TEST | UPDATE (allowed — matching a no-downgrade improvement): keep the existing `simulation_dir`/`config_file`/`substrate_note` asserts (still valid); ADD asserts for `commands.{twitter,reddit,parallel}` containing the platform + `/start`, and non-empty `instructions`. No coverage deleted. |
| `get_simulation` handler | `api/simulation.rs` (sub-cycle c, NEW) | PROD caller | the ONLY production caller: when status==READY, calls `get_run_instructions`, `.to_dict()`, inserts under `"run_instructions"` into the `to_dict()` map. |
| module doc note | `simulation_manager.rs:18, 714-763, 1588-1613` | docs | UPDATE: change "partial / [≠]" framing to "native-expressed; [≠] narrowed to scripts_dir + python/conda literals". |

No other crate references `RunInstructions` (single-file grep clean). Risk: **LOW** (additive, one
new prod caller, one test to extend). No signature change → no transitive recompile fan-out.

---

## Residual `[≠]` flags (NARROWED)

- `- [≠] scripts_dir` — teri has no `backend/scripts/` dir; genuinely inexpressible. Dropped key.
- `- [≠] python run_*.py / conda activate literals` — no Python interpreter, no conda env in teri's
  substrate; these literal command strings cannot execute. Captured/explained in `substrate_note`,
  replaced by the native `commands` map. NOT a feature skip — the *capability* ("how to run this
  prepared sim") is fully expressed via the native start endpoint.

Everything else (`simulation_dir`, `config_file`, the per-platform `commands` map, the `instructions`
string) is now NATIVE-EXPRESSED. The carry-forward gate is SATISFIED.
