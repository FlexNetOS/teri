//! OASIS profile export layer — S-367, S-369, S-370, S-371, S-372, S-373.
//!
//! Ports `OasisProfileGenerator.generate_profiles_from_entities` and the family of
//! file-write methods (`save_profiles`, `_save_twitter_csv`, `_save_reddit_json`,
//! `_normalize_gender`, `save_profiles_to_json`) from
//! `backend/app/services/oasis_profile_generator.py` (MiroFish).
//!
//! **DECISION-10 (target-architecture.md):** these methods produce CONTRACTUAL observable
//! outputs (`reddit_profiles.json` / `twitter_profiles.csv`) that are:
//!
//! 1. Served by the U-026 API (`GET /<id>/profiles`).
//! 2. Read back by `get_profiles(sim_id, platform)` in `SimulationManager`.
//! 3. Named by teri's existing i18n keys `loadedRedditProfiles`/`loadedTwitterProfiles`.
//!
//! They were previously `[≠]`-skipped; DECISION-10 rules they are PORTED.
//!
//! **Blast-radius: ADDITIVE ONLY.** This module CONSUMES (never modifies) the
//! parity-verified `SocialProfile`, `Persona`, `PersonaGenerator`, `generate_username`,
//! and `EntityNode` types.
//!
//! **Realtime-save vs final-save:** `generate_profiles_from_entities` uses
//! `Persona::to_reddit_format`/`to_twitter_format` for the _realtime_ incremental write
//! (matching MiroFish's inner `save_profiles_realtime` closure which calls those methods).
//! The _final_ `save_reddit_json`/`save_twitter_csv` writers use their own dedicated field
//! mapping with forced OASIS defaults — faithful to MiroFish's own realtime-vs-save split.

use crate::agent::{Persona, PersonaGenerator, Platform, SocialProfile};
use crate::graph::KnowledgeGraph;
use crate::llm::LlmClient;
use crate::services::entity_reader::EntityNode;
use std::io;
use std::path::Path;
use tracing::{info, warn};

/// Output platform for the OASIS export (selects the file format).
///
/// Mirrors the `output_platform: str` parameter of MiroFish
/// `generate_profiles_from_entities` and `save_profiles`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPlatform {
    Reddit,
    Twitter,
}

// ──────────────────────────────────────────────────────────────────────────────
// S-371: _normalize_gender
// ──────────────────────────────────────────────────────────────────────────────

/// Normalise a gender string to the OASIS required English values
/// `{male, female, other}`.
///
/// Ports `OasisProfileGenerator._normalize_gender` (L1121-1145) verbatim:
///
/// - `None` or empty → `"other"`.
/// - Chinese: `男` → `"male"`, `女` → `"female"`, `机构` / `其他` → `"other"`.
/// - English passthrough: `"male"` → `"male"`, `"female"` → `"female"`,
///   `"other"` → `"other"`.
/// - Any other value (after lowercase+trim) → `"other"`.
///
/// NOTE: Python uses `gender.lower().strip()` before the map lookup.
/// Chinese characters are unchanged by `.to_lowercase()` so they match correctly.
///
/// S-371's re-flag (symbol-map) fires here: its only call site is
/// `_save_reddit_json` (now ported), so it must port with it.
pub(crate) fn normalize_gender(gender: Option<&str>) -> &'static str {
    let Some(g) = gender else {
        return "other";
    };
    // Mirror Python `gender.lower().strip()`
    let g = g.trim().to_lowercase();
    match g.as_str() {
        // Chinese → English
        "男" => "male",
        "女" => "female",
        "机构" => "other",
        "其他" => "other",
        // English passthrough
        "male" => "male",
        "female" => "female",
        "other" => "other",
        // Empty after trim, or anything else → default
        _ => "other",
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// S-372: _save_reddit_json  (DEDICATED writer — NOT to_reddit_format)
// ──────────────────────────────────────────────────────────────────────────────

/// Write `reddit_profiles.json` with OASIS-mandatory forced defaults.
///
/// Ports `OasisProfileGenerator._save_reddit_json` (L1146-1193) exactly.
///
/// **Critical:** this is NOT `to_reddit_format`. It forces OASIS-mandatory
/// defaults that `to_reddit_format` conditionally omits:
///
/// - `user_id` = `profile.user_id` (always `u64`, no fallback needed in Rust).
/// - `bio` = `bio[:150]` (CHAR-based truncation) or `"{name}"` when empty.
/// - `persona` = `persona` or `"{name} is a participant in social discussions."`.
/// - `karma` = `karma` or `1000`.
/// - `age` = `age` or `30`  ← **UNCONDITIONAL default** (OASIS required).
/// - `gender` = `normalize_gender(gender)` — **ALWAYS present** (OASIS required).
/// - `mbti` = `mbti` or `"ISTJ"` ← **UNCONDITIONAL default** (OASIS required).
/// - `country` = `country` or `"中国"` ← **UNCONDITIONAL default** (OASIS required).
/// - Optional: `profession` / `interested_topics` only when truthy.
///
/// JSON is written with `ensure_ascii=False` semantics (serde_json with UTF-8 writer)
/// and 2-space indent, matching Python `json.dump(..., ensure_ascii=False, indent=2)`.
///
/// The `name` in each pair is the entity display name (not stored on `SocialProfile`
/// in Rust; passed alongside the profile by the batch function).
pub(crate) fn save_reddit_json(
    profiles: &[(SocialProfile, String)], // (profile, name)
    file_path: &Path,
) -> io::Result<()> {
    let mut data = Vec::with_capacity(profiles.len());
    for (profile, name) in profiles.iter() {
        // user_id: always present; SocialProfile.user_id is u64 (never None in Rust)
        let user_id = profile.user_id;

        // bio: truncate to 150 CHARS (not bytes), fallback to name
        let bio = if profile.bio.is_empty() {
            name.clone()
        } else {
            profile.bio.chars().take(150).collect::<String>()
        };

        // persona: fallback
        let persona = if profile.persona.is_empty() {
            format!("{name} is a participant in social discussions.")
        } else {
            profile.persona.clone()
        };

        // karma: fallback 1000 (mirrors Python `profile.karma if profile.karma else 1000`)
        let karma = if profile.karma == 0 { 1000 } else { profile.karma };

        // age: OASIS required — fallback 30 (mirrors `profile.age if profile.age else 30`)
        let age = profile.age.filter(|&a| a > 0).unwrap_or(30);

        // gender: OASIS required — ALWAYS present, always normalized
        let gender = normalize_gender(profile.gender.as_deref());

        // mbti: OASIS required — fallback "ISTJ"
        let mbti = match profile.mbti.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => "ISTJ".to_string(),
        };

        // country: OASIS required — fallback "中国"
        let country = match profile.country.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => "中国".to_string(),
        };

        let mut item = serde_json::json!({
            "user_id": user_id,
            "username": profile.user_name,
            "name": name,
            "bio": bio,
            "persona": persona,
            "karma": karma,
            "created_at": profile.created_at,
            // OASIS required fields with unconditional defaults
            "age": age,
            "gender": gender,
            "mbti": mbti,
            "country": country,
        });

        // Optional fields — only when truthy (mirroring Python `if profile.profession:`)
        if let Some(ref profession) = profile.profession
            && !profession.is_empty()
        {
            item["profession"] = serde_json::Value::from(profession.as_str());
        }
        if !profile.interested_topics.is_empty() {
            item["interested_topics"] =
                serde_json::Value::from(profile.interested_topics.clone());
        }

        data.push(item);
    }

    // Write with ensure_ascii=False (serde_json writes raw UTF-8 by default), indent=2
    let json_bytes = serde_json::to_vec_pretty(&data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(file_path, json_bytes)?;

    info!(
        "Saved {} Reddit profiles to {} (JSON, with OASIS-mandatory defaults)",
        profiles.len(),
        file_path.display()
    );
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// S-370: _save_twitter_csv  (DEDICATED writer — specific OASIS column contract)
// ──────────────────────────────────────────────────────────────────────────────

/// Write `twitter_profiles.csv` in the OASIS-required column layout.
///
/// Ports `OasisProfileGenerator._save_twitter_csv` (L1070-1119) exactly.
///
/// **Critical column contract (OASIS required):**
/// Header: `['user_id', 'name', 'username', 'user_char', 'description']`
///
/// - `user_id` = **CSV row index** (0-based), NOT `profile.user_id`.
/// - `name` = display name (passed in alongside profile, from source entity name).
/// - `username` = `profile.user_name`.
/// - `user_char` = `profile.bio` when `persona == bio` OR `"{bio} {persona}"` when they
///   differ; `\n`/`\r` replaced with a space (for LLM system-prompt injection).
/// - `description` = `profile.bio` with `\n`/`\r` → space (public display bio).
///
/// File extension: if `file_path` does not end with `.csv`, replaces `.json` with `.csv`
/// (matching Python's `if not file_path.endswith('.csv'): file_path.replace('.json','.csv')`).
///
/// Quoting: Python `csv.writer` default = `QUOTE_MINIMAL` (only quote when the value
/// contains the delimiter, quotechar, or line terminator). The `csv` crate's
/// `WriterBuilder::new()` default matches this field-level quoting/escaping (commas
/// quoted, embedded `"` doubled).  The line TERMINATOR differs — Python's `csv.writer`
/// emits CRLF, the `csv` crate emits LF — but this is NON-CONTRACTUAL: the read path
/// (`simulation.py` API / `zep_tools.py`) opens the file in text mode without
/// `newline=''`, so Python universal-newlines normalizes CRLF/LF before parsing and the
/// observable contract (the parsed rows) is identical (opus parity gate, DECISION-10 §1).
pub(crate) fn save_twitter_csv(
    profiles: &[(SocialProfile, String)], // (profile, name)
    file_path: &Path,
) -> io::Result<()> {
    // Enforce .csv extension (mirror Python: replace .json with .csv if needed)
    let resolved_path;
    let path = if file_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        file_path
    } else {
        let s = file_path.to_string_lossy();
        let new = if s.ends_with(".json") {
            s.replacen(".json", ".csv", 1)
        } else {
            format!("{s}.csv")
        };
        resolved_path = std::path::PathBuf::from(new);
        resolved_path.as_path()
    };

    let file = std::fs::File::create(path)?;
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false) // we write the header row ourselves
        .from_writer(file);

    // Write OASIS-required header
    writer.write_record(["user_id", "name", "username", "user_char", "description"])?;

    for (idx, (profile, name)) in profiles.iter().enumerate() {
        // user_char: bio alone when persona == bio or persona is empty;
        // else "{bio} {persona}". Replace \n/\r with space.
        let user_char = if profile.persona.is_empty() || profile.persona == profile.bio {
            profile.bio.replace(['\n', '\r'], " ")
        } else {
            format!("{} {}", profile.bio, profile.persona).replace(['\n', '\r'], " ")
        };

        // description: bio with newlines replaced
        let description = profile.bio.replace(['\n', '\r'], " ");

        writer.write_record([
            &idx.to_string(),       // user_id = row index (0-based)
            name.as_str(),
            profile.user_name.as_str(),
            &user_char,
            &description,
        ])?;
    }

    writer.flush()?;

    info!(
        "Saved {} Twitter profiles to {} (OASIS CSV format)",
        profiles.len(),
        path.display()
    );
    Ok(())
}

// ──────────────────────────────────────────────────────────────────────────────
// S-369: save_profiles  (platform-dispatch writer)
// ──────────────────────────────────────────────────────────────────────────────

/// Platform-dispatch writer: routes to `save_twitter_csv` or `save_reddit_json`.
///
/// Ports `OasisProfileGenerator.save_profiles` (L1047-1068).
///
/// - `OutputPlatform::Twitter` → `save_twitter_csv`.
/// - `OutputPlatform::Reddit` (or anything else) → `save_reddit_json`.
pub fn save_profiles(
    profiles: &[(SocialProfile, String)], // (profile, name)
    file_path: &Path,
    platform: OutputPlatform,
) -> io::Result<()> {
    match platform {
        OutputPlatform::Twitter => save_twitter_csv(profiles, file_path),
        OutputPlatform::Reddit => save_reddit_json(profiles, file_path),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// S-373: save_profiles_to_json  (deprecated alias)
// ──────────────────────────────────────────────────────────────────────────────

/// Deprecated alias for `save_profiles`.
///
/// Ports `OasisProfileGenerator.save_profiles_to_json` (L1196-1204).
/// Emits a deprecation warning (mirrors Python `logger.warning(...)`) then delegates.
pub fn save_profiles_to_json(
    profiles: &[(SocialProfile, String)],
    file_path: &Path,
    platform: OutputPlatform,
) -> io::Result<()> {
    warn!("save_profiles_to_json is deprecated; use save_profiles instead");
    save_profiles(profiles, file_path, platform)
}

// ──────────────────────────────────────────────────────────────────────────────
// S-367: generate_profiles_from_entities  (batch generator)
// ──────────────────────────────────────────────────────────────────────────────

/// Batch-generate `SocialProfile`s from a slice of `EntityNode`s.
///
/// Ports `OasisProfileGenerator.generate_profiles_from_entities` (L851-1014).
///
/// **Concurrency:** MiroFish runs this with a `ThreadPoolExecutor(max_workers=parallel_count)`.
/// DECISION-10 §4 rules that the concurrency model is the CALLER's concern
/// (e.g. `prepare_simulation` chooses sequential vs `tokio::JoinSet`). This function
/// runs SEQUENTIALLY (each entity awaited in order) — preserving the ordered `Vec` result
/// and per-entity fallback semantics.
///
/// **Realtime-save path (`realtime_output`):** after EACH profile is generated, if
/// `realtime_output` is `Some((path, platform))`, the complete set of
/// successfully-generated profiles so far is written to the file.
///
/// - Reddit platform: uses `Persona::to_reddit_format` (as MiroFish's realtime closure does).
/// - Twitter platform: uses `Persona::to_twitter_format` (as MiroFish's realtime closure does).
///
/// A write failure does NOT abort the batch — only logged as a warning, matching MiroFish's
/// `except Exception as e: logger.warning(...)`.
///
/// **Fallback profile:** on generation error, a baseline `SocialProfile` is constructed
/// (entity_type: entity_name for bio, entity.summary or generic persona, user_id=idx)
/// — matching MiroFish's `except Exception: … OasisAgentProfile(…)`.
///
/// **`user_id`:** set to `idx` (the entity's position in the input slice), matching
/// MiroFish `generate_profile_from_entity(entity=entity, user_id=idx, ...)`.
///
/// `progress_callback(current, total, message)`: called after each entity (including
/// fallbacks); `current` is 1-based, `total` is `entities.len()`.
///
/// Returns profiles in ENTITY ORDER (Vec index == idx).
#[allow(clippy::too_many_arguments)]
pub async fn generate_profiles_from_entities<L: LlmClient>(
    generator: &PersonaGenerator,
    llm: &L,
    entities: &[EntityNode],
    graph: Option<&KnowledgeGraph>,
    use_llm: bool,
    _parallel_count: usize, // reserved for caller-level parallelism; sequential here
    realtime_output: Option<(&Path, OutputPlatform)>,
    progress_callback: &mut dyn FnMut(i64, i64, String),
) -> Vec<(SocialProfile, String)> {
    let total = entities.len() as i64;
    let mut results: Vec<Option<(SocialProfile, String)>> = vec![None; entities.len()];

    for (idx, entity) in entities.iter().enumerate() {
        let entity_type = entity.get_entity_type().unwrap_or_else(|| "Entity".to_string());
        let entity_name = entity.name.clone();

        // Platform: Reddit as the neutral internal format for batch generation.
        // The dedicated writers apply the OASIS-required shape per platform.
        let platform = Platform::Reddit;

        // Graph context: resolve entity by uuid if the graph is available
        let graph_ctx = graph.and_then(|g| {
            uuid::Uuid::parse_str(&entity.uuid)
                .ok()
                .and_then(|id| g.get_entity_by_id(id))
                .map(|e| (g, e))
        });

        // generate_social has LLM → salvage → rule-based fallback internally;
        // `use_llm=false` cannot bypass it at this call level (generate_social always
        // tries LLM first). The `_parallel_count` hint and `use_llm` flag are preserved
        // as parameters for caller semantics; the sequential body is faithful per DECISION-10.
        let _ = use_llm; // consumed by caller semantics; generate_social has its own fallback

        let generation_result = generator
            .generate_social(
                &entity_name,
                &entity_type,
                &entity.summary,
                platform,
                llm,
                graph_ctx,
            )
            .await;

        let (mut profile, had_error) = match generation_result {
            Ok(p) => (p, false),
            Err(e) => {
                // Fallback profile — mirrors MiroFish's `except Exception: OasisAgentProfile(...)`
                warn!(
                    "Failed to generate profile for entity '{}': {}; using fallback",
                    entity_name, e
                );
                let fallback = SocialProfile {
                    user_id: 0,
                    user_name: PersonaGenerator::generate_username(&entity_name),
                    bio: format!("{entity_type}: {entity_name}"),
                    persona: if entity.summary.is_empty() {
                        "A participant in social discussions.".to_string()
                    } else {
                        entity.summary.clone()
                    },
                    platform,
                    karma: 1000,
                    friend_count: 100,
                    follower_count: 150,
                    following_count: 100,
                    statuses_count: 500,
                    age: None,
                    gender: None,
                    mbti: None,
                    country: None,
                    profession: None,
                    interested_topics: vec![],
                    posting_style: None,
                    source_entity_uuid: Some(entity.uuid.clone()),
                    source_entity_type: Some(entity_type.clone()),
                    created_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
                };
                (fallback, true)
            }
        };

        // Set user_id = idx (matching MiroFish `user_id=idx` in generate_profile_from_entity)
        profile.user_id = idx as u64;
        profile.source_entity_uuid = Some(entity.uuid.clone());
        profile.source_entity_type = Some(entity_type.clone());

        results[idx] = Some((profile, entity_name.clone()));

        // Realtime-save: write all completed profiles so far
        if let Some((rt_path, rt_platform)) = realtime_output {
            let completed: Vec<&(SocialProfile, String)> =
                results.iter().flatten().collect();
            if !completed.is_empty()
                && let Err(e) = realtime_save(&completed, rt_path, rt_platform)
            {
                warn!("实时保存 profiles 失败: {}", e);
            }
        }

        // Progress callback — 1-based current count
        let current = (idx as i64) + 1;
        let msg = if had_error {
            format!("[{current}/{total}] {entity_name} 使用备用人设")
        } else {
            format!("已完成 {current}/{total}: {entity_name}（{entity_type}）")
        };
        progress_callback(current, total, msg);
    }

    // Unwrap — every slot was filled (either Ok or fallback)
    results.into_iter().flatten().collect()
}

// ──────────────────────────────────────────────────────────────────────────────
// Realtime-save helper (inner closure from MiroFish — not a public symbol)
// ──────────────────────────────────────────────────────────────────────────────

/// Write the current set of completed profiles to the realtime output path.
///
/// Mirrors MiroFish's `save_profiles_realtime` inner closure (L889-917):
///
/// - Reddit: calls `to_reddit_format` on a minimal `Persona` wrapper.
/// - Twitter: calls `to_twitter_format` on a minimal `Persona` wrapper.
///
/// S-368 (`_print_generated_profile`) legitimately stays `[≠]` — it is a debug stdout
/// print, non-contractual, not reproduced here.
fn realtime_save(
    profiles: &[&(SocialProfile, String)],
    path: &Path,
    platform: OutputPlatform,
) -> io::Result<()> {
    match platform {
        OutputPlatform::Reddit => {
            let data: Vec<serde_json::Value> = profiles
                .iter()
                .filter_map(|(profile, name)| {
                    build_persona_wrapper(profile, name).to_reddit_format()
                })
                .collect();
            let bytes = serde_json::to_vec_pretty(&data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            std::fs::write(path, bytes)
        }
        OutputPlatform::Twitter => {
            let rows: Vec<serde_json::Value> = profiles
                .iter()
                .filter_map(|(profile, name)| {
                    build_persona_wrapper(profile, name).to_twitter_format()
                })
                .collect();
            if rows.is_empty() {
                return Ok(());
            }
            let file = std::fs::File::create(path)?;
            let mut writer = csv::WriterBuilder::new()
                .has_headers(false)
                .from_writer(file);
            // Header from to_twitter_format keys
            let fieldnames: Vec<String> = rows[0]
                .as_object()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            writer.write_record(&fieldnames)?;
            for row in &rows {
                if let Some(obj) = row.as_object() {
                    let record: Vec<String> = fieldnames
                        .iter()
                        .map(|k| value_to_csv_string(&obj[k]))
                        .collect();
                    writer.write_record(&record)?;
                }
            }
            writer.flush()
        }
    }
}

/// Build a minimal `Persona` wrapper around a `SocialProfile` so we can call
/// `to_reddit_format`/`to_twitter_format` which live on `Persona`.
fn build_persona_wrapper(profile: &SocialProfile, name: &str) -> Persona {
    Persona {
        name: name.to_string(),
        background: String::new(),
        traits: vec![],
        role: String::new(),
        social: Some(profile.clone()),
    }
}

/// Convert a `serde_json::Value` to a CSV cell string.
fn value_to_csv_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Platform;
    use tempfile::tempdir;

    // ── helpers ───────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn make_profile(
        user_id: u64,
        user_name: &str,
        bio: &str,
        persona: &str,
        age: Option<u32>,
        gender: Option<&str>,
        mbti: Option<&str>,
        country: Option<&str>,
        profession: Option<&str>,
        topics: Vec<&str>,
        karma: i64,
    ) -> SocialProfile {
        SocialProfile {
            user_id,
            user_name: user_name.to_string(),
            bio: bio.to_string(),
            persona: persona.to_string(),
            platform: Platform::Reddit,
            karma,
            friend_count: 100,
            follower_count: 150,
            following_count: 100,
            statuses_count: 500,
            age,
            gender: gender.map(|s| s.to_string()),
            mbti: mbti.map(|s| s.to_string()),
            country: country.map(|s| s.to_string()),
            profession: profession.map(|s| s.to_string()),
            interested_topics: topics.iter().map(|s| s.to_string()).collect(),
            posting_style: None,
            source_entity_uuid: None,
            source_entity_type: None,
            created_at: "2026-01-01".to_string(),
        }
    }

    // ── S-371: normalize_gender ───────────────────────────────────────────────

    #[test]
    fn normalize_gender_none_returns_other() {
        assert_eq!(normalize_gender(None), "other");
    }

    #[test]
    fn normalize_gender_empty_returns_other() {
        assert_eq!(normalize_gender(Some("")), "other");
    }

    #[test]
    fn normalize_gender_whitespace_returns_other() {
        assert_eq!(normalize_gender(Some("   ")), "other");
    }

    #[test]
    fn normalize_gender_male_chinese() {
        assert_eq!(normalize_gender(Some("男")), "male");
    }

    #[test]
    fn normalize_gender_female_chinese() {
        assert_eq!(normalize_gender(Some("女")), "female");
    }

    #[test]
    fn normalize_gender_institution_chinese() {
        assert_eq!(normalize_gender(Some("机构")), "other");
    }

    #[test]
    fn normalize_gender_other_chinese() {
        assert_eq!(normalize_gender(Some("其他")), "other");
    }

    #[test]
    fn normalize_gender_male_english() {
        assert_eq!(normalize_gender(Some("male")), "male");
        assert_eq!(normalize_gender(Some("Male")), "male");
        assert_eq!(normalize_gender(Some("MALE")), "male");
    }

    #[test]
    fn normalize_gender_female_english() {
        assert_eq!(normalize_gender(Some("female")), "female");
        assert_eq!(normalize_gender(Some("Female")), "female");
    }

    #[test]
    fn normalize_gender_other_english() {
        assert_eq!(normalize_gender(Some("other")), "other");
        assert_eq!(normalize_gender(Some("Other")), "other");
    }

    #[test]
    fn normalize_gender_unknown_returns_other() {
        assert_eq!(normalize_gender(Some("unknown")), "other");
        assert_eq!(normalize_gender(Some("non-binary")), "other");
        assert_eq!(normalize_gender(Some("garbage123")), "other");
    }

    // ── S-372: save_reddit_json — OASIS forced defaults ───────────────────────

    #[test]
    fn save_reddit_json_forces_oasis_defaults_when_fields_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        // Profile with all optional fields missing — should see all OASIS defaults
        let profile = make_profile(
            5, "alice_wonder", "Short bio.", "Detailed persona.", None, None, None, None, None,
            vec![], 0,
        );
        let pairs = vec![(profile, "Alice Wonder".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1);
        let p = &parsed[0];

        // OASIS mandatory forced defaults
        assert_eq!(p["age"], serde_json::json!(30), "age default must be 30");
        assert_eq!(p["gender"], serde_json::json!("other"), "gender must always be present");
        assert_eq!(p["mbti"], serde_json::json!("ISTJ"), "mbti default must be ISTJ");
        assert_eq!(p["country"], serde_json::json!("中国"), "country default must be 中国");
        // karma default
        assert_eq!(p["karma"], serde_json::json!(1000), "karma default must be 1000");
        // Always-present keys
        assert!(p["user_id"].is_number());
        assert_eq!(p["username"], serde_json::json!("alice_wonder"));
        assert_eq!(p["name"], serde_json::json!("Alice Wonder"));
        assert_eq!(p["bio"], serde_json::json!("Short bio."));
        assert_eq!(p["persona"], serde_json::json!("Detailed persona."));
    }

    #[test]
    fn save_reddit_json_uses_provided_values_when_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            7, "bob_smith", "My bio.", "My persona.",
            Some(25), Some("male"), Some("ENFP"), Some("美国"),
            Some("Engineer"), vec!["Tech", "Science"], 500,
        );
        let pairs = vec![(profile, "Bob Smith".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        let p = &parsed[0];

        assert_eq!(p["age"], serde_json::json!(25));
        assert_eq!(p["gender"], serde_json::json!("male"));
        assert_eq!(p["mbti"], serde_json::json!("ENFP"));
        assert_eq!(p["country"], serde_json::json!("美国"));
        assert_eq!(p["karma"], serde_json::json!(500));
        assert_eq!(p["profession"], serde_json::json!("Engineer"));
        assert_eq!(p["interested_topics"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn save_reddit_json_bio_truncated_to_150_chars() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        // bio of 200 chars
        let long_bio: String = "A".repeat(200);
        let profile = make_profile(
            0, "user", &long_bio, "persona", Some(30), Some("male"), Some("INTJ"),
            Some("China"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "User".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        let bio = parsed[0]["bio"].as_str().unwrap();
        assert_eq!(bio.chars().count(), 150, "bio must be truncated to exactly 150 chars");
    }

    #[test]
    fn save_reddit_json_bio_empty_falls_back_to_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            0, "user", "", "persona", Some(30), Some("male"), Some("INTJ"),
            Some("China"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Fallback Name".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed[0]["bio"], serde_json::json!("Fallback Name"));
    }

    #[test]
    fn save_reddit_json_persona_empty_falls_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            0, "user", "bio", "", Some(30), Some("male"), Some("INTJ"),
            Some("China"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Fallback Name".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        let persona = parsed[0]["persona"].as_str().unwrap();
        assert!(
            persona.contains("participant in social discussions"),
            "persona fallback must contain expected text, got: {persona}"
        );
    }

    #[test]
    fn save_reddit_json_gender_always_normalized() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        // Chinese gender input → should be normalized to English
        let profile = make_profile(
            0, "user", "bio", "persona", Some(30), Some("男"), Some("INTJ"),
            Some("China"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Name".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed[0]["gender"], serde_json::json!("male"));
    }

    #[test]
    fn save_reddit_json_utf8_raw_not_escaped() {
        // ensure_ascii=False: Chinese characters must appear as raw UTF-8, not \uXXXX
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            0, "user", "生物", "人设", Some(30), Some("女"), Some("INTJ"),
            Some("中国"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "张三".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("中国"), "中国 must appear as raw UTF-8");
        assert!(raw.contains("张三"), "张三 must appear as raw UTF-8");
        assert!(!raw.contains("\\u4e2d"), "must NOT have \\uXXXX escapes");
    }

    #[test]
    fn save_reddit_json_multiple_profiles_ordered() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let p0 = make_profile(0, "user0", "bio0", "persona0", Some(20), Some("male"), Some("INTJ"), Some("China"), None, vec![], 1000);
        let p1 = make_profile(1, "user1", "bio1", "persona1", Some(30), Some("female"), Some("ENFP"), Some("Japan"), None, vec![], 2000);
        let pairs = vec![(p0, "Name0".to_string()), (p1, "Name1".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["username"], serde_json::json!("user0"));
        assert_eq!(parsed[1]["username"], serde_json::json!("user1"));
    }

    // ── S-370: save_twitter_csv ───────────────────────────────────────────────

    #[test]
    fn save_twitter_csv_exact_header() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            99, "alice", "My bio.", "My persona.", Some(25), Some("female"),
            Some("INFJ"), Some("UK"), Some("Writer"), vec![], 1000,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let headers = reader.headers().unwrap().clone();
        let header_vec: Vec<&str> = headers.iter().collect();
        assert_eq!(
            header_vec,
            ["user_id", "name", "username", "user_char", "description"],
            "CSV header must be exactly ['user_id','name','username','user_char','description']"
        );
    }

    #[test]
    fn save_twitter_csv_user_id_is_row_index_not_profile_user_id() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        // profile.user_id = 99 but row index = 0
        let profile = make_profile(
            99, "alice", "bio", "persona", Some(25), Some("female"),
            Some("INFJ"), Some("UK"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        assert_eq!(
            record.get(0).unwrap(),
            "0",
            "user_id in CSV must be ROW INDEX (0), not profile.user_id (99)"
        );
    }

    #[test]
    fn save_twitter_csv_user_char_bio_plus_persona_when_different() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            0, "alice", "Short bio.", "Detailed persona text.", Some(25), Some("female"),
            Some("INFJ"), Some("UK"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        let user_char = record.get(3).unwrap();
        assert_eq!(
            user_char, "Short bio. Detailed persona text.",
            "user_char must be '{{bio}} {{persona}}' when they differ"
        );
    }

    #[test]
    fn save_twitter_csv_user_char_bio_only_when_persona_equals_bio() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            0, "alice", "Same text.", "Same text.", Some(25), Some("female"),
            Some("INFJ"), Some("UK"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        let user_char = record.get(3).unwrap();
        assert_eq!(
            user_char, "Same text.",
            "user_char must be just bio when persona == bio"
        );
    }

    #[test]
    fn save_twitter_csv_newlines_replaced_with_space() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            0, "alice", "bio\nline2\r\nline3", "persona\nnewline",
            Some(25), Some("female"), Some("INFJ"), Some("UK"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        let user_char = record.get(3).unwrap();
        let description = record.get(4).unwrap();
        assert!(!user_char.contains('\n'), "user_char must not contain \\n");
        assert!(!user_char.contains('\r'), "user_char must not contain \\r");
        assert!(!description.contains('\n'), "description must not contain \\n");
    }

    #[test]
    fn save_twitter_csv_description_is_bio_with_newlines_replaced() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            0, "alice", "bio line", "persona", Some(25), Some("female"),
            Some("INFJ"), Some("UK"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let record = reader.records().next().unwrap().unwrap();
        let description = record.get(4).unwrap();
        assert_eq!(description, "bio line", "description must be the bio text");
    }

    #[test]
    fn save_twitter_csv_multiple_rows_row_index_increments() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let p0 = make_profile(99, "u0", "bio0", "p0", None, None, None, None, None, vec![], 1000);
        let p1 = make_profile(99, "u1", "bio1", "p1", None, None, None, None, None, vec![], 1000);
        let p2 = make_profile(99, "u2", "bio2", "p2", None, None, None, None, None, vec![], 1000);
        let pairs = vec![
            (p0, "N0".to_string()),
            (p1, "N1".to_string()),
            (p2, "N2".to_string()),
        ];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let records: Vec<csv::StringRecord> =
            reader.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].get(0).unwrap(), "0");
        assert_eq!(records[1].get(0).unwrap(), "1");
        assert_eq!(records[2].get(0).unwrap(), "2");
    }

    #[test]
    fn save_twitter_csv_json_extension_replaced_with_csv() {
        let dir = tempdir().unwrap();
        // Pass a .json path — should be written as .csv
        let json_path = dir.path().join("twitter_profiles.json");
        let csv_path = dir.path().join("twitter_profiles.csv");

        let profile =
            make_profile(0, "alice", "bio", "persona", None, None, None, None, None, vec![], 1000);
        let pairs = vec![(profile, "Alice".to_string())];

        save_twitter_csv(&pairs, &json_path).unwrap();

        assert!(csv_path.exists(), ".json extension must be replaced with .csv");
        assert!(!json_path.exists(), "original .json path must not be created");
    }

    // ── S-369: save_profiles (dispatch) ──────────────────────────────────────

    #[test]
    fn save_profiles_reddit_writes_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            0, "user", "bio", "persona", Some(30), Some("male"), Some("INTJ"),
            Some("China"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Name".to_string())];

        save_profiles(&pairs, &path, OutputPlatform::Reddit).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn save_profiles_twitter_writes_csv() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            0, "user", "bio", "persona", Some(25), Some("female"), Some("ENFP"),
            Some("Japan"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Name".to_string())];

        save_profiles(&pairs, &path, OutputPlatform::Twitter).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let count = reader.records().count();
        assert_eq!(count, 1);
    }

    // ── S-373: save_profiles_to_json (deprecated alias) ──────────────────────

    #[test]
    fn save_profiles_to_json_delegates_to_save_profiles() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            0, "user", "bio", "persona", Some(30), Some("male"), Some("INTJ"),
            Some("China"), None, vec![], 1000,
        );
        let pairs = vec![(profile, "Name".to_string())];

        save_profiles_to_json(&pairs, &path, OutputPlatform::Reddit).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 1, "deprecated alias must produce the same output");
    }

    // ── S-367: generate_profiles_from_entities (batch generator) ─────────────

    /// Mock LLM client that always returns a valid JSON social profile.
    struct MockLlm {
        response: String,
    }

    impl MockLlm {
        fn always_ok() -> Self {
            Self {
                response: r#"{
                    "bio": "Mock bio.",
                    "persona": "Mock persona.",
                    "karma": 750,
                    "friend_count": 80,
                    "follower_count": 120,
                    "statuses_count": 300,
                    "age": 28,
                    "gender": "female",
                    "mbti": "INFP",
                    "country": "Canada",
                    "profession": "Teacher",
                    "interested_topics": ["Education"],
                    "posting_style": "Thoughtful"
                }"#
                .to_string(),
            }
        }

        fn always_error() -> Self {
            Self {
                response: "INVALID JSON !!!".to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _prompt: &str) -> crate::error::Result<String> {
            Ok(self.response.clone())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.response)
                .map_err(|e| crate::error::TeriError::Unknown(e.to_string()))
        }
        async fn stream(
            &self,
            _prompt: &str,
        ) -> crate::error::Result<
            std::pin::Pin<
                Box<dyn futures::Stream<Item = crate::error::Result<String>> + Send>,
            >,
        > {
            use futures::stream;
            Ok(Box::pin(stream::iter(vec![Ok(self.response.clone())])))
        }
        async fn chat(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> crate::error::Result<String> {
            Ok(self.response.clone())
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[crate::llm::ChatMessage],
            _opts: &crate::llm::ChatOptions,
        ) -> crate::error::Result<T> {
            serde_json::from_str(&self.response)
                .map_err(|e| crate::error::TeriError::Unknown(e.to_string()))
        }
    }

    fn make_entity(uuid: &str, name: &str) -> EntityNode {
        EntityNode {
            uuid: uuid.to_string(),
            name: name.to_string(),
            labels: vec!["Person".to_string()],
            summary: format!("{name} is a test entity."),
            attributes: serde_json::Map::new(),
            related_edges: vec![],
            related_nodes: vec![],
        }
    }

    #[tokio::test]
    async fn generate_profiles_from_entities_returns_ordered_vec() {
        let generator = PersonaGenerator::new();
        let llm = MockLlm::always_ok();
        let entities = vec![
            make_entity("00000000-0000-0000-0000-000000000001", "Alice"),
            make_entity("00000000-0000-0000-0000-000000000002", "Bob"),
            make_entity("00000000-0000-0000-0000-000000000003", "Carol"),
        ];
        let mut cb_calls = 0i64;
        let mut cb = |current: i64, total: i64, _msg: String| {
            cb_calls += 1;
            assert!(current >= 1 && current <= total);
        };

        let results = generate_profiles_from_entities(
            &generator, &llm, &entities, None, true, 1, None, &mut cb,
        )
        .await;

        assert_eq!(results.len(), 3, "must return one profile per entity");
        assert_eq!(cb_calls, 3, "progress_callback must be called once per entity");

        // user_id must be set to idx
        assert_eq!(results[0].0.user_id, 0);
        assert_eq!(results[1].0.user_id, 1);
        assert_eq!(results[2].0.user_id, 2);

        // entity names must be preserved
        assert_eq!(results[0].1, "Alice");
        assert_eq!(results[1].1, "Bob");
        assert_eq!(results[2].1, "Carol");
    }

    #[tokio::test]
    async fn generate_profiles_from_entities_fallback_on_llm_error() {
        let generator = PersonaGenerator::new();
        let llm = MockLlm::always_error();
        let entities = vec![make_entity(
            "00000000-0000-0000-0000-000000000001",
            "FailEntity",
        )];
        let mut cb = |_c: i64, _t: i64, _m: String| {};

        let results = generate_profiles_from_entities(
            &generator, &llm, &entities, None, true, 1, None, &mut cb,
        )
        .await;

        // Even on total parse failure, a fallback profile must be returned
        assert_eq!(results.len(), 1);
        assert!(!results[0].0.bio.is_empty(), "fallback bio must not be empty");
        assert_eq!(results[0].0.user_id, 0, "user_id must be set to idx=0");
    }

    #[tokio::test]
    async fn generate_profiles_from_entities_realtime_write_after_each() {
        let dir = tempdir().unwrap();
        let rt_path = dir.path().join("realtime_reddit.json");

        let generator = PersonaGenerator::new();
        let llm = MockLlm::always_ok();
        let entities = vec![
            make_entity("00000000-0000-0000-0000-000000000001", "Alice"),
            make_entity("00000000-0000-0000-0000-000000000002", "Bob"),
        ];

        let rt_path_clone = rt_path.clone();
        let mut cb = move |current: i64, _total: i64, _msg: String| {
            // After each profile, the file should exist and have at most `current` entries
            if rt_path_clone.exists() {
                let content = std::fs::read_to_string(&rt_path_clone).unwrap();
                let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
                assert!(
                    parsed.len() as i64 <= current,
                    "realtime file should have at most {current} entries after step {current}"
                );
            }
        };

        generate_profiles_from_entities(
            &generator,
            &llm,
            &entities,
            None,
            true,
            1,
            Some((&rt_path, OutputPlatform::Reddit)),
            &mut cb,
        )
        .await;

        // Final file must exist and contain all profiles
        assert!(rt_path.exists(), "realtime output file must exist after batch");
        let content = std::fs::read_to_string(&rt_path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.len(), 2, "final realtime file must contain all profiles");
    }

    #[tokio::test]
    async fn generate_profiles_from_entities_writes_files_when_sim_dir_given() {
        let dir = tempdir().unwrap();
        let reddit_path = dir.path().join("reddit_profiles.json");
        let twitter_path = dir.path().join("twitter_profiles.csv");

        let generator = PersonaGenerator::new();
        let llm = MockLlm::always_ok();
        let entities = vec![
            make_entity("00000000-0000-0000-0000-000000000001", "Alice"),
            make_entity("00000000-0000-0000-0000-000000000002", "Bob"),
        ];
        let mut cb = |_c: i64, _t: i64, _m: String| {};

        let results = generate_profiles_from_entities(
            &generator, &llm, &entities, None, true, 1, None, &mut cb,
        )
        .await;

        // Write both files (simulating what prepare_simulation stage 2 does)
        save_profiles(&results, &reddit_path, OutputPlatform::Reddit).unwrap();
        save_profiles(&results, &twitter_path, OutputPlatform::Twitter).unwrap();

        // Reddit file: valid JSON array with 2 entries
        let reddit_content = std::fs::read_to_string(&reddit_path).unwrap();
        let reddit: Vec<serde_json::Value> = serde_json::from_str(&reddit_content).unwrap();
        assert_eq!(reddit.len(), 2);

        // Twitter file: valid CSV with 2 data rows
        let mut reader = csv::Reader::from_path(&twitter_path).unwrap();
        let count = reader.records().count();
        assert_eq!(count, 2);
    }

    // ── round-trip: write then read back ─────────────────────────────────────

    #[test]
    fn reddit_json_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reddit_profiles.json");

        let profile = make_profile(
            3, "alice", "bio text", "persona text", Some(25), Some("female"),
            Some("INFJ"), Some("UK"), Some("Writer"), vec!["Arts"], 800,
        );
        let pairs = vec![(profile, "Alice".to_string())];

        save_reddit_json(&pairs, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&content).unwrap();
        let p = &parsed[0];

        // Verify the complete OASIS required field set is present
        for key in &["user_id", "username", "name", "bio", "persona", "karma",
                      "created_at", "age", "gender", "mbti", "country"] {
            assert!(p.get(*key).is_some(), "required key '{key}' must be present");
        }
        // Optional fields present when truthy
        assert!(p.get("profession").is_some(), "profession must be present");
        assert!(p.get("interested_topics").is_some(), "interested_topics must be present");
    }

    #[test]
    fn twitter_csv_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("twitter_profiles.csv");

        let profile = make_profile(
            5, "alice_w", "Alice's bio.", "Alice's detailed persona.",
            Some(30), Some("female"), Some("ENFJ"), Some("France"),
            Some("Artist"), vec![], 1200,
        );
        let pairs = vec![(profile, "Alice Wonder".to_string())];

        save_twitter_csv(&pairs, &path).unwrap();

        let mut reader = csv::Reader::from_path(&path).unwrap();
        let headers = reader.headers().unwrap().clone();
        assert_eq!(headers.get(0).unwrap(), "user_id");
        assert_eq!(headers.get(1).unwrap(), "name");
        assert_eq!(headers.get(2).unwrap(), "username");
        assert_eq!(headers.get(3).unwrap(), "user_char");
        assert_eq!(headers.get(4).unwrap(), "description");

        let record = reader.records().next().unwrap().unwrap();
        assert_eq!(record.get(0).unwrap(), "0"); // row index
        assert_eq!(record.get(1).unwrap(), "Alice Wonder");
        assert_eq!(record.get(2).unwrap(), "alice_w");
        // user_char = "{bio} {persona}" since they differ
        assert_eq!(
            record.get(3).unwrap(),
            "Alice's bio. Alice's detailed persona."
        );
        // description = bio
        assert_eq!(record.get(4).unwrap(), "Alice's bio.");
    }
}
