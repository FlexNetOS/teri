//! Ontology generation service.
//!
//! Port of `backend/app/services/ontology_generator.py` (MiroFish, 506 lines).
//!
//! # Symbol mapping (S-172..S-180)
//!
//! | Source symbol | Lines | Rust symbol |
//! |---|---|---|
//! | S-172 `ONTOLOGY_SYSTEM_PROMPT` | 30-173 | `ONTOLOGY_SYSTEM_PROMPT` const |
//! | S-173 `_to_pascal_case` | 16-26 | `to_pascal_case` fn |
//! | S-174 `OntologyGenerator` | 176 | `OntologyGenerator` struct |
//! | S-175 `__init__` | 182-183 | `OntologyGenerator::new` |
//! | S-176 `generate` | 185-226 | `OntologyGenerator::generate` |
//! | S-177 `MAX_TEXT_LENGTH_FOR_LLM` | 229 | `MAX_TEXT_LENGTH_FOR_LLM` const |
//! | S-178 `_build_user_message` | 231-275 | `build_user_message` fn |
//! | S-179 `_validate_and_process` | 277-398 | `validate_and_process` fn |
//! | S-180 `generate_python_code` | 400-505 | `[≠]` — see note below |
//!
//! # S-180 `generate_python_code` — `[≠]` intentional divergence
//!
//! **NOT PORTED.** Justification (meets the `[≠]` bar):
//!
//! 1. **Zero callers in MiroFish** — verified by exhaustive grep across the entire
//!    `backend/` tree: no file imports or calls `generate_python_code`. It is dead code
//!    in the source.
//!
//! 2. **Zep-Cloud-specific Python output** — the method emits Zep Cloud Python class
//!    strings (`from zep_cloud.external_clients.ontology import EntityModel, EntityText,
//!    EdgeModel`). Teri replaces Zep Cloud with native petgraph (ADR-0001, Decision-2);
//!    the ontology-registration behavior maps onto `EntityKind::Custom` and the future
//!    `set_ontology` (S-192) port, NOT onto Python class strings. The method is
//!    **genuinely inexpressible** in teri's substrate AND has zero callers, satisfying
//!    both `[≠]` conditions simultaneously.
//!
//! If a future unit requires the ontology-as-Python-code export, the correct destination
//! is `EntityKind` / petgraph registration in `src/graph/mod.rs`, not this module.

use crate::error::{Result, TeriError};
use crate::i18n::get_language_instruction;
use crate::llm::{ChatMessage, ChatOptions, LlmClient};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

// ============================================================================
// S-173 — _to_pascal_case
// ============================================================================

/// Convert any name format to PascalCase.
///
/// Port of MiroFish `ontology_generator.py:16-26` `_to_pascal_case`.
///
/// Two-stage algorithm (matches Python exactly):
/// 1. Split on any run of non-alphanumeric characters.
/// 2. For each part, insert `_` at camelCase boundaries
///    (`([a-z])([A-Z])` → `$1_$2`), then split on `_`.
/// 3. Capitalize first letter of each word (Python `.capitalize()` = first char
///    upper + REST LOWER — so `"ABC"` → `"Abc"`, `"abc"` → `"Abc"`).
/// 4. Filter empties, join.
/// 5. Empty result → `"Unknown"`.
///
/// # Examples
/// ```
/// assert_eq!(teri::services::ontology::to_pascal_case("works_for"), "WorksFor");
/// assert_eq!(teri::services::ontology::to_pascal_case("person"), "Person");
/// assert_eq!(teri::services::ontology::to_pascal_case("camelCase"), "CamelCase");
/// assert_eq!(teri::services::ontology::to_pascal_case("MEDIA_outlet"), "MediaOutlet");
/// assert_eq!(teri::services::ontology::to_pascal_case("___"), "Unknown");
/// assert_eq!(teri::services::ontology::to_pascal_case("ABC"), "Abc");
/// ```
pub fn to_pascal_case(name: &str) -> String {
    // Step 1: split on non-alphanumeric chars
    let non_alnum = Regex::new(r"[^a-zA-Z0-9]+").expect("static regex");
    let parts: Vec<&str> = non_alnum.split(name).collect();

    // Step 2: camelCase boundary split
    let camel_boundary = Regex::new(r"([a-z])([A-Z])").expect("static regex");

    let mut words: Vec<String> = Vec::new();
    for part in parts {
        // Insert _ at camelCase boundaries
        let expanded = camel_boundary.replace_all(part, "${1}_${2}").to_string();
        // Split on _ and collect non-empty segments
        for seg in expanded.split('_') {
            if !seg.is_empty() {
                words.push(seg.to_string());
            }
        }
    }

    // Step 3: capitalize each word (Python .capitalize() = first upper + REST lower)
    let result: String = words
        .iter()
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    // REST LOWER — matches Python .capitalize() exactly (e.g. "ABC" → "Abc")
                    let rest: String = chars.as_str().to_lowercase();
                    upper + &rest
                }
            }
        })
        .collect();

    if result.is_empty() { "Unknown".to_string() } else { result }
}

// ============================================================================
// S-172 — ONTOLOGY_SYSTEM_PROMPT
// ============================================================================

/// System prompt for ontology generation.
///
/// Port of MiroFish `ontology_generator.py:30-173` `ONTOLOGY_SYSTEM_PROMPT`.
/// Ported VERBATIM — the LLM behavior depends on the exact prompt text,
/// including all Chinese characters, the JSON format block, design guidelines,
/// and entity/relation reference lists.
pub const ONTOLOGY_SYSTEM_PROMPT: &str = r#"你是一个专业的知识图谱本体设计专家。你的任务是分析给定的文本内容和模拟需求，设计适合**社交媒体舆论模拟**的实体类型和关系类型。

**重要：你必须输出有效的JSON格式数据，不要输出任何其他内容。**

## 核心任务背景

我们正在构建一个**社交媒体舆论模拟系统**。在这个系统中：
- 每个实体都是一个可以在社交媒体上发声、互动、传播信息的"账号"或"主体"
- 实体之间会相互影响、转发、评论、回应
- 我们需要模拟舆论事件中各方的反应和信息传播路径

因此，**实体必须是现实中真实存在的、可以在社媒上发声和互动的主体**：

**可以是**：
- 具体的个人（公众人物、当事人、意见领袖、专家学者、普通人）
- 公司、企业（包括其官方账号）
- 组织机构（大学、协会、NGO、工会等）
- 政府部门、监管机构
- 媒体机构（报纸、电视台、自媒体、网站）
- 社交媒体平台本身
- 特定群体代表（如校友会、粉丝团、维权群体等）

**不可以是**：
- 抽象概念（如"舆论"、"情绪"、"趋势"）
- 主题/话题（如"学术诚信"、"教育改革"）
- 观点/态度（如"支持方"、"反对方"）

## 输出格式

请输出JSON格式，包含以下结构：

```json
{
    "entity_types": [
        {
            "name": "实体类型名称（英文，PascalCase）",
            "description": "简短描述（英文，不超过100字符）",
            "attributes": [
                {
                    "name": "属性名（英文，snake_case）",
                    "type": "text",
                    "description": "属性描述"
                }
            ],
            "examples": ["示例实体1", "示例实体2"]
        }
    ],
    "edge_types": [
        {
            "name": "关系类型名称（英文，UPPER_SNAKE_CASE）",
            "description": "简短描述（英文，不超过100字符）",
            "source_targets": [
                {"source": "源实体类型", "target": "目标实体类型"}
            ],
            "attributes": []
        }
    ],
    "analysis_summary": "对文本内容的简要分析说明"
}
```

## 设计指南（极其重要！）

### 1. 实体类型设计 - 必须严格遵守

**数量要求：必须正好10个实体类型**

**层次结构要求（必须同时包含具体类型和兜底类型）**：

你的10个实体类型必须包含以下层次：

A. **兜底类型（必须包含，放在列表最后2个）**：
   - `Person`: 任何自然人个体的兜底类型。当一个人不属于其他更具体的人物类型时，归入此类。
   - `Organization`: 任何组织机构的兜底类型。当一个组织不属于其他更具体的组织类型时，归入此类。

B. **具体类型（8个，根据文本内容设计）**：
   - 针对文本中出现的主要角色，设计更具体的类型
   - 例如：如果文本涉及学术事件，可以有 `Student`, `Professor`, `University`
   - 例如：如果文本涉及商业事件，可以有 `Company`, `CEO`, `Employee`

**为什么需要兜底类型**：
- 文本中会出现各种人物，如"中小学教师"、"路人甲"、"某位网友"
- 如果没有专门的类型匹配，他们应该被归入 `Person`
- 同理，小型组织、临时团体等应该归入 `Organization`

**具体类型的设计原则**：
- 从文本中识别出高频出现或关键的角色类型
- 每个具体类型应该有明确的边界，避免重叠
- description 必须清晰说明这个类型和兜底类型的区别

### 2. 关系类型设计

- 数量：6-10个
- 关系应该反映社媒互动中的真实联系
- 确保关系的 source_targets 涵盖你定义的实体类型

### 3. 属性设计

- 每个实体类型1-3个关键属性
- **注意**：属性名不能使用 `name`、`uuid`、`group_id`、`created_at`、`summary`（这些是系统保留字）
- 推荐使用：`full_name`, `title`, `role`, `position`, `location`, `description` 等

## 实体类型参考

**个人类（具体）**：
- Student: 学生
- Professor: 教授/学者
- Journalist: 记者
- Celebrity: 明星/网红
- Executive: 高管
- Official: 政府官员
- Lawyer: 律师
- Doctor: 医生

**个人类（兜底）**：
- Person: 任何自然人（不属于上述具体类型时使用）

**组织类（具体）**：
- University: 高校
- Company: 公司企业
- GovernmentAgency: 政府机构
- MediaOutlet: 媒体机构
- Hospital: 医院
- School: 中小学
- NGO: 非政府组织

**组织类（兜底）**：
- Organization: 任何组织机构（不属于上述具体类型时使用）

## 关系类型参考

- WORKS_FOR: 工作于
- STUDIES_AT: 就读于
- AFFILIATED_WITH: 隶属于
- REPRESENTS: 代表
- REGULATES: 监管
- REPORTS_ON: 报道
- COMMENTS_ON: 评论
- RESPONDS_TO: 回应
- SUPPORTS: 支持
- OPPOSES: 反对
- COLLABORATES_WITH: 合作
- COMPETES_WITH: 竞争
"#;

// ============================================================================
// S-177 — MAX_TEXT_LENGTH_FOR_LLM
// ============================================================================

/// Maximum text length (in Unicode scalar values / chars) passed to the LLM.
///
/// Port of MiroFish `ontology_generator.py:229` `MAX_TEXT_LENGTH_FOR_LLM = 50000`.
///
/// NOTE: Python `len()` on a str counts characters (Unicode scalar values), not
/// bytes. Truncation in `build_user_message` uses `.chars().count()` and
/// `.chars().take(MAX_TEXT_LENGTH_FOR_LLM)` to match this exactly, avoiding
/// byte-boundary slicing in multi-byte UTF-8 text (e.g. Chinese characters).
pub const MAX_TEXT_LENGTH_FOR_LLM: usize = 50000;

// ============================================================================
// S-174 / S-175 — OntologyGenerator + new()
// ============================================================================

/// Ontology generator service.
///
/// Analyzes document text and a simulation requirement, then generates entity
/// and relation type definitions suitable for social-media opinion simulation.
///
/// Port of MiroFish `OntologyGenerator` (`ontology_generator.py:176-398`).
///
/// # Type parameter
/// `L` is the LLM client implementation.  Use a concrete type at the call site
/// (e.g. `OntologyGenerator::new(OpenAiAdapter::new(&cfg))`) or a generic
/// bound `<L: LlmClient>`.  This mirrors the `<L: LlmClient>` idiom used by
/// `Agent`, `AgentPool`, and `PersonaGenerator` in `src/agent/mod.rs`.
pub struct OntologyGenerator<L: LlmClient> {
    /// The LLM client used for generation.
    client: L,
}

impl<L: LlmClient> OntologyGenerator<L> {
    /// Construct a new `OntologyGenerator` with the given LLM client.
    ///
    /// Port of `OntologyGenerator.__init__` (py:182-183).
    pub fn new(client: L) -> Self {
        Self { client }
    }

    // =========================================================================
    // S-176 — generate
    // =========================================================================

    /// Generate ontology definitions from document texts and a simulation requirement.
    ///
    /// Port of MiroFish `OntologyGenerator.generate` (py:185-226).
    ///
    /// 1. Builds the user message (S-178 `_build_user_message`).
    /// 2. Fetches the language instruction (`get_language_instruction()`).
    /// 3. Constructs the system prompt with the IMPORTANT English suffix.
    /// 4. Calls `chat_json` with `[system, user]` messages,
    ///    `temperature=0.3`, `max_tokens=4096`.
    /// 5. Validates and post-processes the result (S-179 `_validate_and_process`).
    pub async fn generate(
        &self,
        document_texts: &[String],
        simulation_requirement: &str,
        additional_context: Option<&str>,
    ) -> Result<Value> {
        // Step 1: build user message
        let user_message =
            build_user_message(document_texts, simulation_requirement, additional_context);

        // Step 2: language instruction
        let lang_instruction = get_language_instruction();

        // Step 3: system prompt with IMPORTANT English suffix (ported verbatim from py:210)
        let system_prompt = format!(
            "{}\n\n{}\nIMPORTANT: Entity type names MUST be in English PascalCase (e.g., 'PersonEntity', 'MediaOrganization'). Relationship type names MUST be in English UPPER_SNAKE_CASE (e.g., 'WORKS_FOR'). Attribute names MUST be in English snake_case. Only description fields and analysis_summary should use the specified language above.",
            ONTOLOGY_SYSTEM_PROMPT, lang_instruction
        );

        // Step 4: call chat_json with temperature=0.3, max_tokens=4096
        let result: Value = self
            .client
            .chat_json(
                &[ChatMessage::system(system_prompt), ChatMessage::user(user_message)],
                &ChatOptions { temperature: Some(0.3), max_tokens: Some(4096) },
            )
            .await
            .map_err(|e| TeriError::Llm(format!("OntologyGenerator LLM call failed: {e}")))?;

        // Step 5: validate and post-process
        Ok(validate_and_process(result))
    }
}

// ============================================================================
// S-178 — _build_user_message
// ============================================================================

/// Build the user message for the ontology-generation LLM call.
///
/// Port of MiroFish `OntologyGenerator._build_user_message` (py:231-275).
///
/// # Truncation
/// If the combined text exceeds [`MAX_TEXT_LENGTH_FOR_LLM`] **characters**
/// (not bytes), it is truncated to that limit and a Chinese notice is appended.
/// Python `len()` counts Unicode scalar values, so this implementation uses
/// `.chars().count()` / `.chars().take(N)` to match exactly.
pub fn build_user_message(
    document_texts: &[String],
    simulation_requirement: &str,
    additional_context: Option<&str>,
) -> String {
    // Join with the Python separator (py:240)
    let combined_raw: String = document_texts.join("\n\n---\n\n");

    // Count in chars (Unicode scalar values) — matches Python len() (py:241)
    let original_length = combined_raw.chars().count();

    let combined_text: String = if original_length > MAX_TEXT_LENGTH_FOR_LLM {
        // Truncate to MAX chars (not bytes) — avoids slicing mid-UTF-8 sequence (py:244-246)
        let truncated: String = combined_raw.chars().take(MAX_TEXT_LENGTH_FOR_LLM).collect();
        format!(
            "{}\n\n...(原文共{}字，已截取前{}字用于本体分析)...",
            truncated, original_length, MAX_TEXT_LENGTH_FOR_LLM
        )
    } else {
        combined_raw
    };

    // Build base message (py:248-255) — f-string port with exact format
    let mut message = format!(
        "## 模拟需求\n\n{}\n\n## 文档内容\n\n{}\n",
        simulation_requirement, combined_text
    );

    // Optional additional context (py:257-262)
    if let Some(ctx) = additional_context {
        message.push_str(&format!("\n## 额外说明\n\n{}\n", ctx));
    }

    // Rules footer (py:264-273) — verbatim Chinese
    message.push_str(
        r#"
请根据以上内容，设计适合社会舆论模拟的实体类型和关系类型。

**必须遵守的规则**：
1. 必须正好输出10个实体类型
2. 最后2个必须是兜底类型：Person（个人兜底）和 Organization（组织兜底）
3. 前8个是根据文本内容设计的具体类型
4. 所有实体类型必须是现实中可以发声的主体，不能是抽象概念
5. 属性名不能使用 name、uuid、group_id 等保留字，用 full_name、org_name 等替代
"#,
    );

    message
}

// ============================================================================
// S-179 — _validate_and_process
// ============================================================================

/// Validate and post-process the raw LLM-generated ontology JSON.
///
/// Port of MiroFish `OntologyGenerator._validate_and_process` (py:277-398).
/// Pure logic — no LLM calls; fully unit-testable.
///
/// # Branches (every branch preserved):
/// - Ensure `entity_types`/`edge_types`/`analysis_summary` keys exist.
/// - Entities: PascalCase names (recording a rename map); ensure `attributes`/`examples`;
///   truncate `description` to 97 chars + `"..."` if > 100 chars.
/// - Edges: UPPER_SNAKE_CASE names; remap `source_targets` source/target via rename map;
///   ensure `source_targets`/`attributes`; truncate description.
/// - Dedup entities by `name` (keep first; warn on dup).
/// - Fallback injection: if `Person` or `Organization` absent, inject them; if the
///   injection would exceed 10 entities, remove from the END of the list first.
/// - Final defensive cap: entity_types[:10], edge_types[:10].
///
/// # Description truncation
/// Uses `.chars().count()` (not `.len()`) to match Python's character-based `len()`.
/// Truncates to 97 chars + `"..."` when length > 100 chars.
pub fn validate_and_process(mut result: Value) -> Value {
    // --- Ensure required top-level keys (py:281-286) ---
    if result.get("entity_types").is_none() {
        result["entity_types"] = Value::Array(vec![]);
    }
    if result.get("edge_types").is_none() {
        result["edge_types"] = Value::Array(vec![]);
    }
    if result.get("analysis_summary").is_none() {
        result["analysis_summary"] = Value::String(String::new());
    }

    // --- Build entity_name_map and process entity types (py:290-305) ---
    let mut entity_name_map: HashMap<String, String> = HashMap::new();

    if let Some(entities) = result["entity_types"].as_array_mut() {
        for entity in entities.iter_mut() {
            // PascalCase name (py:294-298)
            if let Some(original_name) =
                entity.get("name").and_then(Value::as_str).map(str::to_string)
            {
                let pascal = to_pascal_case(&original_name);
                if pascal != original_name {
                    tracing::warn!(
                        "Entity type name '{}' auto-converted to '{}'",
                        original_name,
                        pascal
                    );
                }
                entity_name_map.insert(original_name, pascal.clone());
                entity["name"] = Value::String(pascal);
            }
            // Ensure attributes / examples (py:299-302)
            if entity.get("attributes").is_none() {
                entity["attributes"] = Value::Array(vec![]);
            }
            if entity.get("examples").is_none() {
                entity["examples"] = Value::Array(vec![]);
            }
            // Truncate description (py:303-305) — char-based
            truncate_description(entity);
        }
    }

    // --- Process edge types (py:308-326) ---
    if let Some(edges) = result["edge_types"].as_array_mut() {
        for edge in edges.iter_mut() {
            // UPPER_SNAKE_CASE name (py:311-314)
            if let Some(original_name) =
                edge.get("name").and_then(Value::as_str).map(str::to_string)
            {
                let upper = original_name.to_uppercase();
                if upper != original_name {
                    tracing::warn!(
                        "Edge type name '{}' auto-converted to '{}'",
                        original_name,
                        upper
                    );
                }
                edge["name"] = Value::String(upper);
            }
            // Remap source_targets via entity_name_map (py:315-320)
            // NOTE: ensure source_targets exists BEFORE iterating it (py order: iterate first, then ensure)
            if let Some(source_targets) =
                edge.get_mut("source_targets").and_then(Value::as_array_mut)
            {
                for st in source_targets.iter_mut() {
                    if let Some(src) = st.get("source").and_then(Value::as_str).map(str::to_string)
                        && let Some(mapped) = entity_name_map.get(&src)
                    {
                        st["source"] = Value::String(mapped.clone());
                    }
                    if let Some(tgt) = st.get("target").and_then(Value::as_str).map(str::to_string)
                        && let Some(mapped) = entity_name_map.get(&tgt)
                    {
                        st["target"] = Value::String(mapped.clone());
                    }
                }
            }
            // Ensure source_targets / attributes (py:321-324)
            if edge.get("source_targets").is_none() {
                edge["source_targets"] = Value::Array(vec![]);
            }
            if edge.get("attributes").is_none() {
                edge["attributes"] = Value::Array(vec![]);
            }
            // Truncate description (py:325-326) — char-based
            truncate_description(edge);
        }
    }

    // Limits (py:329-330)
    const MAX_ENTITY_TYPES: usize = 10;
    const MAX_EDGE_TYPES: usize = 10;

    // --- Dedup entities by name, keep first (py:333-342) ---
    let entities: Vec<Value> = result["entity_types"].as_array().cloned().unwrap_or_default();

    let mut seen_names: HashSet<String> = HashSet::new();
    let mut deduped: Vec<Value> = Vec::new();
    for entity in entities {
        let name = entity.get("name").and_then(Value::as_str).unwrap_or("").to_string();
        if name.is_empty() {
            // No name key — include as-is (Python doesn't filter nameless entities here)
            deduped.push(entity);
        } else if seen_names.contains(&name) {
            tracing::warn!("Duplicate entity type '{}' removed during validation", name);
        } else {
            seen_names.insert(name);
            deduped.push(entity);
        }
    }
    result["entity_types"] = Value::Array(deduped);

    // --- Fallback injection (py:344-389) ---
    // Exact fallback dicts — verbatim port from py:345-363
    let person_fallback = serde_json::json!({
        "name": "Person",
        "description": "Any individual person not fitting other specific person types.",
        "attributes": [
            {"name": "full_name", "type": "text", "description": "Full name of the person"},
            {"name": "role", "type": "text", "description": "Role or occupation"}
        ],
        "examples": ["ordinary citizen", "anonymous netizen"]
    });

    let organization_fallback = serde_json::json!({
        "name": "Organization",
        "description": "Any organization not fitting other specific organization types.",
        "attributes": [
            {"name": "org_name", "type": "text", "description": "Name of the organization"},
            {"name": "org_type", "type": "text", "description": "Type of organization"}
        ],
        "examples": ["small business", "community group"]
    });

    // Check which fallbacks are absent (py:365-368)
    let entity_names: HashSet<String> = result["entity_types"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|e| e.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();

    let has_person = entity_names.contains("Person");
    let has_organization = entity_names.contains("Organization");

    let mut fallbacks_to_add: Vec<Value> = Vec::new();
    if !has_person {
        fallbacks_to_add.push(person_fallback);
    }
    if !has_organization {
        fallbacks_to_add.push(organization_fallback);
    }

    if !fallbacks_to_add.is_empty() {
        let current_count = result["entity_types"].as_array().map(|a| a.len()).unwrap_or(0);
        let needed_slots = fallbacks_to_add.len();

        // Remove from the END if adding would exceed MAX (py:382-386)
        if current_count + needed_slots > MAX_ENTITY_TYPES {
            let to_remove = current_count + needed_slots - MAX_ENTITY_TYPES;
            let entities = result["entity_types"].as_array_mut().unwrap();
            // Remove to_remove items from the end (Python entities[:-to_remove])
            let new_len = entities.len().saturating_sub(to_remove);
            entities.truncate(new_len);
        }

        // Extend with fallbacks (py:389)
        let entities = result["entity_types"].as_array_mut().unwrap();
        entities.extend(fallbacks_to_add);
    }

    // --- Final defensive caps (py:392-396) ---
    if let Some(entities) = result["entity_types"].as_array_mut() {
        entities.truncate(MAX_ENTITY_TYPES);
    }
    if let Some(edges) = result["edge_types"].as_array_mut() {
        edges.truncate(MAX_EDGE_TYPES);
    }

    result
}

/// Truncate `description` in a JSON object to 97 chars + `"..."` when > 100 chars.
///
/// Uses `.chars().count()` to match Python's character-based `len()` (not byte length).
fn truncate_description(obj: &mut Value) {
    if let Some(desc) = obj.get("description").and_then(Value::as_str).map(str::to_string)
        && desc.chars().count() > 100
    {
        // Take first 97 chars (char-indexed) then append "..."
        let truncated: String = desc.chars().take(97).collect();
        obj["description"] = Value::String(format!("{truncated}..."));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // =========================================================================
    // S-173 — to_pascal_case
    // =========================================================================

    #[test]
    fn pascal_case_works_for() {
        assert_eq!(to_pascal_case("works_for"), "WorksFor");
    }

    #[test]
    fn pascal_case_person() {
        assert_eq!(to_pascal_case("person"), "Person");
    }

    #[test]
    fn pascal_case_camel_case() {
        // camelCase → insert _ at boundary → "camel_Case" → split → ["camel", "Case"]
        // Python .capitalize(): "camel" → "Camel", "Case" → "Case" (C upper + ase lower)
        // Result = "CamelCase"
        assert_eq!(to_pascal_case("camelCase"), "CamelCase");
    }

    #[test]
    fn pascal_case_media_outlet() {
        assert_eq!(to_pascal_case("MEDIA_outlet"), "MediaOutlet");
    }

    #[test]
    fn pascal_case_empty_from_underscores() {
        assert_eq!(to_pascal_case("___"), "Unknown");
    }

    #[test]
    fn pascal_case_all_caps() {
        // Python "ABC".capitalize() = "Abc"
        assert_eq!(to_pascal_case("ABC"), "Abc");
    }

    #[test]
    fn pascal_case_empty_string() {
        assert_eq!(to_pascal_case(""), "Unknown");
    }

    #[test]
    fn pascal_case_with_spaces() {
        assert_eq!(to_pascal_case("hello world"), "HelloWorld");
    }

    #[test]
    fn pascal_case_already_pascal() {
        // "Person" → split on non-alnum (none) → one part "Person"
        // camel boundary: no lowercase→uppercase transition inside "Person"? P-e boundary: e is lowercase, r is lowercase...
        // Actually: "Person" → camel_boundary replaces ([a-z])([A-Z]) — no such transition here
        // → split on _ → ["Person"] → capitalize → "Person"
        assert_eq!(to_pascal_case("Person"), "Person");
    }

    #[test]
    fn pascal_case_hyphenated() {
        assert_eq!(to_pascal_case("media-outlet"), "MediaOutlet");
    }

    // =========================================================================
    // S-178 — build_user_message
    // =========================================================================

    #[test]
    fn build_user_message_no_truncation() {
        let texts = vec!["Hello world".to_string()];
        let msg = build_user_message(&texts, "Simulate public opinion", None);
        assert!(msg.contains("## 模拟需求"));
        assert!(msg.contains("Simulate public opinion"));
        assert!(msg.contains("## 文档内容"));
        assert!(msg.contains("Hello world"));
        assert!(!msg.contains("## 额外说明"));
        assert!(msg.contains("必须遵守的规则"));
    }

    #[test]
    fn build_user_message_with_additional_context() {
        let texts = vec!["content".to_string()];
        let msg = build_user_message(&texts, "req", Some("extra context here"));
        assert!(msg.contains("## 额外说明"));
        assert!(msg.contains("extra context here"));
    }

    #[test]
    fn build_user_message_without_additional_context() {
        let texts = vec!["content".to_string()];
        let msg = build_user_message(&texts, "req", None);
        assert!(!msg.contains("## 额外说明"));
    }

    #[test]
    fn build_user_message_truncation() {
        // A string of 50001 'a' characters should be truncated to 50000 + notice
        let long_text: String = "a".repeat(MAX_TEXT_LENGTH_FOR_LLM + 1);
        let original_len = long_text.chars().count();
        let texts = vec![long_text];
        let msg = build_user_message(&texts, "req", None);
        // The truncation notice must include the original length and the max
        assert!(
            msg.contains(&format!("原文共{}字", original_len)),
            "should contain original char count"
        );
        assert!(
            msg.contains(&format!("前{}字", MAX_TEXT_LENGTH_FOR_LLM)),
            "should reference max chars"
        );
    }

    #[test]
    fn build_user_message_truncation_with_chinese() {
        // Chinese chars are 3 bytes each but 1 char — test char-based truncation
        // 50001 Chinese chars → should truncate at char 50000
        let long_text: String = "中".repeat(MAX_TEXT_LENGTH_FOR_LLM + 1);
        let original_len = long_text.chars().count();
        assert_eq!(original_len, MAX_TEXT_LENGTH_FOR_LLM + 1);
        let texts = vec![long_text];
        let msg = build_user_message(&texts, "req", None);
        assert!(msg.contains(&format!("原文共{}字", original_len)));
        assert!(msg.contains(&format!("前{}字", MAX_TEXT_LENGTH_FOR_LLM)));
    }

    #[test]
    fn build_user_message_exactly_at_limit_no_truncation() {
        // Exactly 50000 chars — no truncation
        let text: String = "x".repeat(MAX_TEXT_LENGTH_FOR_LLM);
        let texts = vec![text];
        let msg = build_user_message(&texts, "req", None);
        assert!(!msg.contains("原文共"), "should NOT contain truncation notice");
    }

    #[test]
    fn build_user_message_multiple_texts_joined() {
        let texts = vec!["first".to_string(), "second".to_string()];
        let msg = build_user_message(&texts, "req", None);
        assert!(msg.contains("first\n\n---\n\nsecond"));
    }

    // =========================================================================
    // S-179 — validate_and_process
    // =========================================================================

    #[test]
    fn validate_missing_keys_get_defaults() {
        // When entity_types/edge_types/analysis_summary are absent, they're initialized to
        // empty arrays/string — BUT the fallback injection then adds Person + Organization
        // (since an empty entity list has neither).  So entity_types ends up with 2 fallbacks,
        // edge_types stays empty, and analysis_summary is "".
        let result = validate_and_process(json!({}));
        let entities = result["entity_types"].as_array().unwrap();
        // Person and Organization fallbacks are injected
        let names: Vec<&str> = entities.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"Person"), "Person fallback injected");
        assert!(names.contains(&"Organization"), "Organization fallback injected");
        assert_eq!(result["edge_types"], json!([]));
        assert_eq!(result["analysis_summary"], json!(""));
    }

    #[test]
    fn validate_entity_name_pascal_cased() {
        let input = json!({
            "entity_types": [{"name": "media_outlet", "description": "A media entity"}],
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        assert_eq!(entities[0]["name"], "MediaOutlet");
    }

    #[test]
    fn validate_edge_name_upper_cased() {
        let input = json!({
            "entity_types": [],
            "edge_types": [{"name": "works_for", "source_targets": []}],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let edges = result["edge_types"].as_array().unwrap();
        assert_eq!(edges[0]["name"], "WORKS_FOR");
    }

    #[test]
    fn validate_source_target_remapped_via_entity_name_map() {
        let input = json!({
            "entity_types": [
                {"name": "media_outlet", "description": "media"},
                {"name": "government_agency", "description": "gov"}
            ],
            "edge_types": [
                {
                    "name": "REGULATES",
                    "source_targets": [{"source": "government_agency", "target": "media_outlet"}]
                }
            ],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let edges = result["edge_types"].as_array().unwrap();
        let st = &edges[0]["source_targets"].as_array().unwrap()[0];
        assert_eq!(st["source"], "GovernmentAgency");
        assert_eq!(st["target"], "MediaOutlet");
    }

    #[test]
    fn validate_description_truncated_at_100_chars() {
        // A 101-char ASCII description → truncated to 97 + "..."
        let long_desc: String = "a".repeat(101);
        let input = json!({
            "entity_types": [{"name": "Person", "description": long_desc}],
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        let desc = entities[0]["description"].as_str().unwrap();
        assert_eq!(desc.chars().count(), 100); // 97 + 3 ("...")
        assert!(desc.ends_with("..."));
    }

    #[test]
    fn validate_description_truncated_with_chinese_chars() {
        // 101 Chinese chars (each 3 bytes) — truncation must be char-based
        let long_desc: String = "中".repeat(101);
        let input = json!({
            "entity_types": [{"name": "Person", "description": long_desc}],
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        let desc = entities[0]["description"].as_str().unwrap();
        // 97 chars + "..." = 100 chars
        assert_eq!(desc.chars().count(), 100);
        assert!(desc.ends_with("..."));
    }

    #[test]
    fn validate_dedup_entities_keeps_first() {
        let input = json!({
            "entity_types": [
                {"name": "Person", "description": "first"},
                {"name": "Person", "description": "second dup"},
            ],
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        // Only one Person (dedup keeps first)
        let person_count = entities.iter().filter(|e| e["name"] == "Person").count();
        assert_eq!(person_count, 1);
        assert_eq!(entities[0]["description"], "first");
    }

    #[test]
    fn validate_fallback_injection_when_both_absent() {
        // Only 2 specific entities, neither Person nor Organization
        let input = json!({
            "entity_types": [
                {"name": "Student", "description": "A student"},
                {"name": "Professor", "description": "A professor"},
            ],
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        let names: Vec<&str> = entities.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"Person"), "Person fallback must be injected");
        assert!(names.contains(&"Organization"), "Organization fallback must be injected");
    }

    #[test]
    fn validate_fallback_removes_from_end_to_make_room() {
        // 10 entities, neither Person nor Organization → remove last 2, add fallbacks
        let entities: Vec<Value> = (0..10)
            .map(|i| json!({"name": format!("Entity{i}"), "description": "d"}))
            .collect();
        let input = json!({
            "entity_types": entities,
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let out = result["entity_types"].as_array().unwrap();
        // Should be exactly 10
        assert_eq!(out.len(), 10);
        let names: Vec<&str> = out.iter().filter_map(|e| e["name"].as_str()).collect();
        // Person and Organization should be at the end
        assert!(names.contains(&"Person"));
        assert!(names.contains(&"Organization"));
        // Entity8 and Entity9 should have been removed (last 2)
        assert!(!names.contains(&"Entity8"));
        assert!(!names.contains(&"Entity9"));
    }

    #[test]
    fn validate_caps_entities_at_10() {
        // 12 entities (including Person and Organization) → capped to 10
        let mut entities: Vec<Value> = (0..10)
            .map(|i| json!({"name": format!("Entity{i}"), "description": "d"}))
            .collect();
        entities.push(json!({"name": "Person", "description": "d"}));
        entities.push(json!({"name": "Organization", "description": "d"}));
        let input = json!({
            "entity_types": entities,
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let out = result["entity_types"].as_array().unwrap();
        assert!(out.len() <= 10);
    }

    #[test]
    fn validate_caps_edge_types_at_10() {
        let edges: Vec<Value> = (0..12)
            .map(|i| json!({"name": format!("EDGE_{i}"), "source_targets": []}))
            .collect();
        let input = json!({
            "entity_types": [],
            "edge_types": edges,
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let out = result["edge_types"].as_array().unwrap();
        assert!(out.len() <= 10);
    }

    #[test]
    fn validate_ensures_attributes_and_examples_defaults() {
        // Entity without attributes or examples gets empty arrays
        let input = json!({
            "entity_types": [{"name": "Person", "description": "d"}],
            "edge_types": [{"name": "WORKS_FOR"}],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        assert_eq!(entities[0]["attributes"], json!([]));
        assert_eq!(entities[0]["examples"], json!([]));
        let edges = result["edge_types"].as_array().unwrap();
        assert_eq!(edges[0]["source_targets"], json!([]));
        assert_eq!(edges[0]["attributes"], json!([]));
    }

    #[test]
    fn validate_person_already_present_not_duplicated() {
        let input = json!({
            "entity_types": [
                {"name": "Person", "description": "individual"},
                {"name": "Organization", "description": "org"},
            ],
            "edge_types": [],
            "analysis_summary": "summary"
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        let person_count = entities.iter().filter(|e| e["name"] == "Person").count();
        let org_count = entities.iter().filter(|e| e["name"] == "Organization").count();
        assert_eq!(person_count, 1, "Person must appear exactly once");
        assert_eq!(org_count, 1, "Organization must appear exactly once");
    }

    #[test]
    fn validate_description_exactly_100_not_truncated() {
        let desc: String = "x".repeat(100);
        let input = json!({
            "entity_types": [{"name": "Person", "description": desc}],
            "edge_types": [],
            "analysis_summary": ""
        });
        let result = validate_and_process(input);
        let entities = result["entity_types"].as_array().unwrap();
        let out_desc = entities[0]["description"].as_str().unwrap();
        assert_eq!(out_desc.chars().count(), 100);
        assert!(!out_desc.ends_with("..."));
    }
}
