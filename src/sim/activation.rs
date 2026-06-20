//! Time-based agent activation policy (U-028 cycle 3a).
//!
//! Faithful port of `TwitterSimulationRunner._get_active_agents_for_round`
//! (`run_twitter_simulation.py:462-529`; `run_reddit_simulation.py` has the identical structure) —
//! the per-round stochastic selection of which agents act, gated by the simulated time-of-day.
//!
//! The OASIS subprocess ran this each round to pick a subset of agents to `env.step`; teri
//! reimplements it natively (map-onto-substrate) as a policy consulted by the run loop. CYCLE 3b
//! wires it into `SimEngine::run` (additive `SimConfig.activation` seam, `None`-default-safe);
//! this cycle (3a) lands and parity-verifies the policy in isolation.
//!
//! # Fidelity note — the script's `.get` defaults, NOT the U-019 dataclass defaults
//! `_get_active_agents_for_round` reads the RAW `time_config` / `agent_configs` dicts with its OWN
//! `.get(key, default)` fallbacks (e.g. `peak_hours` default
//! `[9, 10, 11, 14, 15, 20, 21, 22]`, `off_peak_activity_multiplier` default `0.3`). Those differ
//! from the `TimeSimulationConfig` dataclass defaults (`peak_hours = [19,20,21,22]`,
//! `off_peak_activity_multiplier = 0.05`, `simulation_config.rs`). The two are different Python
//! components: the generator WRITES the dataclass values into the artifact, and the activation
//! READS them back — so in practice the keys are always present and the `.get` defaults never fire.
//! To be byte-faithful to THIS function, [`TimeActivationPolicy::from_config`] mirrors the script's
//! exact `.get` defaults (so a key-absent artifact behaves exactly as the Python script would).
//!
//! # RNG — DECISION-U028-2 (a seedable RNG for testability)
//! Python is unseeded (`random.uniform` / `random.random` / `random.sample`). teri uses a
//! `StdRng`: seeded from entropy in production (matching Python's run-to-run non-determinism —
//! `[≠] U028-RNG-SEQUENCE`) and from a fixed value in tests so the structure (multiplier select,
//! `active_hours` gating, `activity_level` threshold, `target_count` cap, no-duplicate sample) is
//! reproducible and differential-testable. The exact selected *multiset* under a given seed does
//! NOT byte-match Python (different RNG algorithms) — that sequence is the `[≠]`, not the
//! structure.

use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use serde_json::Value;
use std::sync::Mutex;

/// Per-agent activation parameters (the subset of `agent_configs` the policy consults).
struct AgentActivation {
    /// `cfg.get("agent_id", 0)` (`run_twitter_simulation.py:502`).
    agent_id: i64,
    /// `cfg.get("active_hours", list(range(8, 23)))` (`:503`) — hours the agent may act.
    active_hours: Vec<i64>,
    /// `cfg.get("activity_level", 0.5)` (`:504`) — P(act | in active hour).
    activity_level: f64,
}

/// Time-based agent activation policy — selects which agents act in a given simulated hour.
///
/// Port of `_get_active_agents_for_round` (`run_twitter_simulation.py:462-529`).
pub struct TimeActivationPolicy {
    /// `time_config.get("minutes_per_round", 30)` — for [`simulated_hour`].
    minutes_per_round: i64,
    /// `time_config.get("agents_per_hour_min", 5)` (`:483`).
    agents_per_hour_min: f64,
    /// `time_config.get("agents_per_hour_max", 20)` (`:484`).
    agents_per_hour_max: f64,
    /// `time_config.get("peak_hours", [9,10,11,14,15,20,21,22])` (`:487`).
    peak_hours: Vec<i64>,
    /// `time_config.get("off_peak_hours", [0,1,2,3,4,5])` (`:488`).
    off_peak_hours: Vec<i64>,
    /// `time_config.get("peak_activity_multiplier", 1.5)` (`:491`).
    peak_multiplier: f64,
    /// `time_config.get("off_peak_activity_multiplier", 0.3)` (`:493`).
    off_peak_multiplier: f64,
    /// The per-agent activation params (from `agent_configs`).
    agents: Vec<AgentActivation>,
    /// Seedable RNG (DECISION-U028-2). Interior-mutable so `active_agents` takes `&self`.
    rng: Mutex<StdRng>,
}

/// Select the activity multiplier for `hour` — pure, deterministic (no RNG).
///
/// Port of `run_twitter_simulation.py:490-495`: peak → `peak_mult`, off-peak → `off_peak_mult`,
/// otherwise `1.0`. Peak takes precedence over off-peak (the Python `if/elif` order).
pub(crate) fn select_multiplier(
    hour: i64,
    peak_hours: &[i64],
    off_peak_hours: &[i64],
    peak_mult: f64,
    off_peak_mult: f64,
) -> f64 {
    if peak_hours.contains(&hour) {
        peak_mult
    } else if off_peak_hours.contains(&hour) {
        off_peak_mult
    } else {
        1.0
    }
}

impl TimeActivationPolicy {
    /// Build a policy from a `simulation_config.json`-shaped value, mirroring the script's `.get`
    /// defaults (see the module fidelity note). `seed = Some(k)` makes the RNG reproducible for
    /// tests; `seed = None` seeds from entropy (production — matches Python's unseeded behavior).
    pub fn from_config(config: &Value, seed: Option<u64>) -> Self {
        let tc = config.get("time_config");
        let get_i = |k: &str, d: i64| -> i64 {
            tc.and_then(|t| t.get(k)).and_then(Value::as_i64).unwrap_or(d)
        };
        let get_f = |k: &str, d: f64| -> f64 {
            tc.and_then(|t| t.get(k)).and_then(Value::as_f64).unwrap_or(d)
        };
        let get_hours = |k: &str, d: Vec<i64>| -> Vec<i64> {
            tc.and_then(|t| t.get(k))
                .and_then(Value::as_array)
                .map_or(d, |a| a.iter().filter_map(Value::as_i64).collect())
        };

        let minutes_per_round = get_i("minutes_per_round", 30);
        let agents_per_hour_min = get_f("agents_per_hour_min", 5.0);
        let agents_per_hour_max = get_f("agents_per_hour_max", 20.0);
        let peak_hours = get_hours("peak_hours", vec![9, 10, 11, 14, 15, 20, 21, 22]);
        let off_peak_hours = get_hours("off_peak_hours", vec![0, 1, 2, 3, 4, 5]);
        let peak_multiplier = get_f("peak_activity_multiplier", 1.5);
        let off_peak_multiplier = get_f("off_peak_activity_multiplier", 0.3);

        // Per-agent configs. Default active_hours = list(range(8, 23)) = [8..=22].
        let default_active_hours: Vec<i64> = (8..23).collect();
        let agents = config
            .get("agent_configs")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .map(|cfg| AgentActivation {
                        agent_id: cfg.get("agent_id").and_then(Value::as_i64).unwrap_or(0),
                        active_hours: cfg
                            .get("active_hours")
                            .and_then(Value::as_array)
                            .map_or_else(
                                || default_active_hours.clone(),
                                |a| a.iter().filter_map(Value::as_i64).collect(),
                            ),
                        activity_level: cfg
                            .get("activity_level")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.5),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let rng = match seed {
            Some(k) => StdRng::seed_from_u64(k),
            None => StdRng::from_entropy(),
        };

        Self {
            minutes_per_round,
            agents_per_hour_min,
            agents_per_hour_max,
            peak_hours,
            off_peak_hours,
            peak_multiplier,
            off_peak_multiplier,
            agents,
            rng: Mutex::new(rng),
        }
    }

    /// Compute the simulated hour-of-day for a round.
    ///
    /// Port of `run_twitter_simulation.py:635-636`:
    /// `simulated_minutes = round * minutes_per_round; (simulated_minutes // 60) % 24`.
    pub fn simulated_hour(&self, round: u32) -> i64 {
        let simulated_minutes = round as i64 * self.minutes_per_round;
        (simulated_minutes / 60) % 24
    }

    /// Select the agent ids active in `current_hour`.
    ///
    /// Port of `_get_active_agents_for_round` (`run_twitter_simulation.py:462-529`), minus the
    /// `env.agent_graph.get_agent` lookup (that id→agent resolution is the run loop's job in
    /// CYCLE 3b — Python `:520-529`). Returns the selected agent ids (Python `selected_ids`).
    pub fn active_agents(&self, current_hour: i64) -> Vec<i64> {
        let multiplier = select_multiplier(
            current_hour,
            &self.peak_hours,
            &self.off_peak_hours,
            self.peak_multiplier,
            self.off_peak_multiplier,
        );

        let mut rng = self.rng.lock().expect("activation RNG mutex poisoned");

        // target_count = int(random.uniform(min, max) * multiplier).  Python `random.uniform(a, b)`
        // = a + (b - a) * random(); int() truncates toward zero.  Computed manually (not
        // `gen_range`) to match uniform exactly and to never panic when min == max.
        let u = self.agents_per_hour_min
            + (self.agents_per_hour_max - self.agents_per_hour_min) * rng.r#gen::<f64>();
        let target_count = (u * multiplier) as i64;

        // candidates: agents in an active hour that pass the activity_level coin flip.
        let mut candidates: Vec<i64> = Vec::new();
        for a in &self.agents {
            if !a.active_hours.contains(&current_hour) {
                continue; // Python `:507-508`
            }
            if rng.r#gen::<f64>() < a.activity_level {
                candidates.push(a.agent_id); // Python `:511-512`
            }
        }

        // selected = random.sample(candidates, min(target_count, len)) if candidates else [].
        if candidates.is_empty() {
            return Vec::new();
        }
        // `min(target_count, len)`; clamp the lower bound to 0 (Python would ValueError on a
        // negative sample size, but `int(uniform(min≥0, max) * mult≥0)` is never negative — this
        // is an unreachable-input guard, not a behavior change).
        let k = target_count.clamp(0, candidates.len() as i64) as usize;
        // random.sample = without-replacement selection; the teri-seeded order/subset is the
        // `[≠] U028-RNG-SEQUENCE` divergence (structure preserved, exact multiset is teri's RNG).
        candidates.choose_multiple(&mut *rng, k).copied().collect()
    }
}

/// Wire [`TimeActivationPolicy`] into the engine as the per-tick activation gate (U-028 §4/§5).
///
/// `SimEngine::run` calls `active_agent_ids(tick)` each tick; this derives the round's
/// `simulated_hour` (`simulated_hour(tick)`, the 0-based round number — matching Python's
/// `get_active_agents_for_round(env, config, simulated_hour, round_num)` call with the main-loop
/// `round_num`) and returns the stochastically selected agent ids for that hour. The engine maps
/// those ids onto pool agents by `SocialProfile.user_id`.
impl crate::sim::ActivationPolicy for TimeActivationPolicy {
    fn active_agent_ids(&self, tick: u32) -> Vec<i64> {
        let hour = self.simulated_hour(tick);
        self.active_agents(hour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg(time_config: Value, agent_configs: Value) -> Value {
        json!({ "time_config": time_config, "agent_configs": agent_configs })
    }

    // ── select_multiplier — pure, deterministic (Python :490-495) ───────────────

    #[test]
    fn select_multiplier_peak_offpeak_neutral() {
        let peak = [9, 10, 11, 14, 15, 20, 21, 22];
        let off = [0, 1, 2, 3, 4, 5];
        assert_eq!(select_multiplier(9, &peak, &off, 1.5, 0.3), 1.5, "peak hour");
        assert_eq!(select_multiplier(22, &peak, &off, 1.5, 0.3), 1.5, "peak hour");
        assert_eq!(select_multiplier(2, &peak, &off, 1.5, 0.3), 0.3, "off-peak hour");
        assert_eq!(select_multiplier(13, &peak, &off, 1.5, 0.3), 1.0, "neutral hour");
        assert_eq!(select_multiplier(18, &peak, &off, 1.5, 0.3), 1.0, "neutral hour");
    }

    #[test]
    fn select_multiplier_peak_takes_precedence_over_offpeak() {
        // Python if/elif: an hour in BOTH lists resolves to peak (the `if` wins).
        let peak = [3];
        let off = [3];
        assert_eq!(select_multiplier(3, &peak, &off, 1.5, 0.3), 1.5);
    }

    // ── simulated_hour (Python :635-636) ────────────────────────────────────────

    #[test]
    fn simulated_hour_wraps_each_day() {
        // mpr = 30: round*30 minutes; //60 hours; %24 day-wrap.
        let p = TimeActivationPolicy::from_config(
            &cfg(json!({ "minutes_per_round": 30 }), json!([])),
            Some(1),
        );
        assert_eq!(p.simulated_hour(0), 0); // 0 min → hour 0
        assert_eq!(p.simulated_hour(2), 1); // 60 min → hour 1
        assert_eq!(p.simulated_hour(48), 0); // 1440 min → hour 24 → 0 (next day)
        assert_eq!(p.simulated_hour(50), 1); // 1500 min → hour 25 → 1
    }

    // ── active_agents — gating (deterministic regardless of seed) ────────────────

    #[test]
    fn active_agents_excludes_agents_outside_active_hours() {
        // Agent only active at hour 10; querying hour 5 must NEVER include it (any seed).
        let agents = json!([{ "agent_id": 7, "active_hours": [10], "activity_level": 1.0 }]);
        for seed in 0..20u64 {
            let p = TimeActivationPolicy::from_config(
                &cfg(
                    json!({ "agents_per_hour_min": 5, "agents_per_hour_max": 20 }),
                    agents.clone(),
                ),
                Some(seed),
            );
            assert!(p.active_agents(5).is_empty(), "agent gated out at hour 5 (seed {seed})");
        }
    }

    #[test]
    fn active_agents_activity_level_zero_never_selected() {
        // activity_level 0.0 → rng.gen::<f64>() < 0.0 is ALWAYS false → never a candidate.
        let agents = json!([{ "agent_id": 1, "active_hours": [10], "activity_level": 0.0 }]);
        for seed in 0..20u64 {
            let p = TimeActivationPolicy::from_config(&cfg(json!({}), agents.clone()), Some(seed));
            assert!(p.active_agents(10).is_empty(), "activity 0.0 never acts (seed {seed})");
        }
    }

    #[test]
    fn active_agents_activity_level_one_all_eligible_in_hour() {
        // activity 1.0 → gen() < 1.0 always true → every in-hour agent is a candidate. With a
        // large target_count (high agents_per_hour), all candidates are selected.
        let agents = json!([
            { "agent_id": 1, "active_hours": [10], "activity_level": 1.0 },
            { "agent_id": 2, "active_hours": [10], "activity_level": 1.0 },
            { "agent_id": 3, "active_hours": [10], "activity_level": 1.0 },
        ]);
        let p = TimeActivationPolicy::from_config(
            &cfg(json!({ "agents_per_hour_min": 100, "agents_per_hour_max": 100 }), agents),
            Some(42),
        );
        let mut got = p.active_agents(10);
        got.sort_unstable();
        assert_eq!(got, vec![1, 2, 3], "all eligible agents selected when target_count >> count");
    }

    #[test]
    fn active_agents_respects_target_count_cap_and_no_duplicates() {
        // 10 in-hour agents (activity 1.0) but a small target_count → selected.len() == cap,
        // unique, all from the candidate set.
        let agents: Vec<Value> = (0..10)
            .map(|i| json!({ "agent_id": i, "active_hours": [10], "activity_level": 1.0 }))
            .collect();
        // agents_per_hour 3..3, multiplier 1.0 (hour 10 is neither peak nor off-peak by default
        // script lists? 10 IS in default peak [9,10,11,...] → mult 1.5 → target=int(3*1.5)=4).
        // Use explicit empty peak/off lists to force multiplier 1.0 → target = int(3.0) = 3.
        let p = TimeActivationPolicy::from_config(
            &cfg(
                json!({
                    "agents_per_hour_min": 3, "agents_per_hour_max": 3,
                    "peak_hours": [], "off_peak_hours": []
                }),
                json!(agents),
            ),
            Some(7),
        );
        let got = p.active_agents(10);
        assert_eq!(got.len(), 3, "target_count cap = int(uniform(3,3)*1.0) = 3");
        let mut uniq = got.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), got.len(), "no duplicate agent ids (sample without replacement)");
        assert!(got.iter().all(|id| (0..10).contains(id)), "all selected from candidate set");
    }

    #[test]
    fn active_agents_reproducible_under_fixed_seed() {
        let agents: Vec<Value> = (0..8)
            .map(|i| json!({ "agent_id": i, "active_hours": [12], "activity_level": 0.5 }))
            .collect();
        let mk = || {
            TimeActivationPolicy::from_config(
                &cfg(
                    json!({ "agents_per_hour_min": 4, "agents_per_hour_max": 12,
                            "peak_hours": [], "off_peak_hours": [] }),
                    json!(agents),
                ),
                Some(99),
            )
        };
        assert_eq!(mk().active_agents(12), mk().active_agents(12), "same seed → same selection");
    }

    #[test]
    fn active_agents_empty_when_no_configs() {
        let p = TimeActivationPolicy::from_config(&cfg(json!({}), json!([])), Some(1));
        assert!(p.active_agents(10).is_empty(), "no agent_configs → no active agents");
    }
}
