//! Locale/i18n subsystem — port of `backend/app/utils/locale.py` (MiroFish, 70 lines).
//!
//! # Symbol mapping (S-036..S-042)
//!
//! | Source symbol          | Rust mapping                                    |
//! |------------------------|-------------------------------------------------|
//! | S-036 `_thread_local`  | `tokio::task_local! { static LOCALE: String; }`|
//! | S-037 `_locales_dir`   | `include_str!` paths — no runtime dir           |
//! | S-038 `_translations`  | `OnceLock<HashMap<String, Value>>` (en + zh)    |
//! | S-039 `set_locale`     | `LOCALE.scope(value, future).await` — see below |
//! | S-040 `get_locale`     | Two branches: request-ctx (PENDING) + task-local|
//! | S-041 `t` / `t_args`  | key traversal + zh fallback + key passthrough   |
//! | S-042 `get_language_instruction` | languages.json lookup + zh fallback  |
//!
//! # S-039 `set_locale` — task-local design note
//!
//! Python's `_thread_local.locale = locale` is a synchronous thread-local write.  In an async
//! Tokio runtime, task-local storage is the faithful equivalent — `thread_local!` does **not**
//! survive `.await` hops across worker threads.  Task-locals in Tokio are set via
//! `LOCALE.scope(value, future).await`, which pins the value for the entire lifetime of the
//! enclosed future.
//!
//! The **caller pattern** in MiroFish is:
//! ```python
//! current_locale = get_locale()          # capture from request context
//! # ...
//! set_locale(current_locale)             # propagate into background thread
//! ```
//! The Rust equivalent is:
//! ```rust,ignore
//! let current = i18n::get_locale();
//! LOCALE.scope(current, async move {
//!     // all .await calls here see the captured locale
//! }).await;
//! ```
//! There is no separate `set_locale(s)` function because task-locals cannot be mutated in place
//! after creation — the entire future must be wrapped.  `with_locale` provides a named wrapper
//! for ergonomics at call sites.
//!
//! # S-040 `get_locale` — request-context branch (LANDED in `accept_language_middleware`)
//!
//! Python's `get_locale` has **two branches**:
//! ```python
//! if has_request_context():
//!     raw = request.headers.get('Accept-Language', 'zh')
//!     return raw if raw in _translations else 'zh'
//! return getattr(_thread_local, 'locale', 'zh')
//! ```
//! Branch 1 (Flask request context → `Accept-Language` header) is ported in teri's axum HTTP
//! layer as `accept_language_middleware` (`src/server.rs`), applied to all routes:
//! - Reads the `Accept-Language` header from the axum `HeaderMap`.
//! - Validates via `is_supported_locale(raw)` → `raw` else `"zh"` (matching `raw in _translations`).
//! - Wraps the handler in `with_locale(validated_locale, next.run(request)).await`, so inner
//!   calls to `get_locale()` see the request locale automatically.
//!
//! Branch 2 (task-local fallback, default `"zh"`) is implemented here in `get_locale`.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;

// ---------------------------------------------------------------------------
// S-036 — task-local locale storage
// ---------------------------------------------------------------------------

tokio::task_local! {
    /// Task-local locale string.  Set via [`with_locale`].  Defaults to `"en"` when unset
    /// (teri ships English-first; Chinese is served when a request asks for it via
    /// `Accept-Language` or a `with_locale("zh", …)` scope).
    pub static LOCALE: String;
}

// ---------------------------------------------------------------------------
// S-037 / S-038 — embedded translation assets, parsed once
// ---------------------------------------------------------------------------

/// Raw embedded JSON bytes for each translation file.
const ZH_JSON: &str = include_str!("locales/zh.json");
const EN_JSON: &str = include_str!("locales/en.json");
/// Raw embedded JSON bytes for the language registry (7 entries).
const LANGUAGES_JSON: &str = include_str!("locales/languages.json");

/// Parsed translation map: exactly `{"zh": <Object>, "en": <Object>}`.
/// Mirrors Python's `_translations` dict which iterates `locales/` and skips `languages.json`,
/// finding only `zh.json` and `en.json` — so exactly two keys.
fn translations() -> &'static HashMap<String, Value> {
    static TRANSLATIONS: OnceLock<HashMap<String, Value>> = OnceLock::new();
    TRANSLATIONS.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert(
            "zh".to_string(),
            serde_json::from_str(ZH_JSON).expect("zh.json embedded parse error"),
        );
        map.insert(
            "en".to_string(),
            serde_json::from_str(EN_JSON).expect("en.json embedded parse error"),
        );
        map
    })
}

/// Returns `true` if `locale` is a key in the embedded translations map.
///
/// Use this to validate an incoming locale string against the actual embedded set
/// rather than a hardcoded list.  When a new locale file is added under
/// `i18n/locales/` and wired into [`translations`], this predicate automatically
/// covers it — no second site to update.
///
/// Mirrors the membership test in `locale.py:31`:
/// `return raw if raw in _translations else 'zh'`
pub fn is_supported_locale(locale: &str) -> bool {
    translations().contains_key(locale)
}

/// Parsed language registry: 7 entries (zh/en/es/fr/pt/ru/de).
/// Mirrors Python's `_languages` dict loaded from `languages.json`.
fn languages() -> &'static Value {
    static LANGUAGES: OnceLock<Value> = OnceLock::new();
    LANGUAGES.get_or_init(|| {
        serde_json::from_str(LANGUAGES_JSON).expect("languages.json embedded parse error")
    })
}

// ---------------------------------------------------------------------------
// S-039 — set_locale / with_locale
// ---------------------------------------------------------------------------

/// Run `future` with the task-local locale set to `locale`.
///
/// This is the idiomatic Rust/Tokio equivalent of Python's
/// `_thread_local.locale = locale`.  Because task-locals cannot be mutated in
/// place, the value must be established by wrapping the future:
///
/// ```rust,ignore
/// // capture locale from request context, then propagate into a spawned task
/// let locale = i18n::get_locale();
/// tokio::spawn(i18n::with_locale(locale, async move {
///     // all i18n::t() calls here see the propagated locale
///     do_work().await;
/// }));
/// ```
pub async fn with_locale<F, T>(locale: String, future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    LOCALE.scope(locale, future).await
}

// ---------------------------------------------------------------------------
// S-040 — get_locale
// ---------------------------------------------------------------------------

/// Returns the active locale string.
///
/// **Branch 1 — request-context (LANDED in `accept_language_middleware`):**
/// teri's axum HTTP layer ports Python's `has_request_context()` branch as
/// `accept_language_middleware` (`src/server.rs`), applied to all routes. It reads the
/// `Accept-Language` header, validates via `is_supported_locale` (→ `raw` else `"zh"`), and
/// calls `with_locale(validated_locale, handler).await` so inner calls to `get_locale()`
/// already see the request locale — no explicit request-context check is needed here.
///
/// **Branch 2 — task-local fallback (fully implemented):**
/// Reads the `LOCALE` task-local; defaults to `"en"` when unset. teri ships English-first
/// (the owner-facing default), so any code path that runs outside an HTTP request scope
/// (background sim/report tasks, the CLI pipeline) renders English unless a locale was
/// explicitly propagated. Chinese remains fully supported via the request-context branch
/// (`Accept-Language: zh`) and the frontend language switcher.
///
/// Note: the task-local branch returns the stored value **as-is**, without validating against
/// `translations()`.  Only the request-context branch validates.
pub fn get_locale() -> String {
    LOCALE.try_with(|l| l.clone()).unwrap_or_else(|_| "en".to_string())
}

// ---------------------------------------------------------------------------
// S-041 — t (zero-arg) and t_args (with interpolation)
// ---------------------------------------------------------------------------

/// Translate `key` using the current locale, with no parameter substitution.
///
/// Equivalent to `t(key)` in Python (the `**kwargs`-free call form).
/// See [`t_args`] for the substitution form.
pub fn t(key: &str) -> String {
    t_args(key, &[])
}

/// Translate `key` using the current locale, substituting `{name}` placeholders with values.
///
/// # Python mapping
/// Python's `t(key, **kwargs)` accepts arbitrary keyword arguments and replaces `{k}` with
/// `str(v)` for each `(k, v)` pair.  Rust does not have `**kwargs`, so callers pass an
/// ordered slice of `(&str, impl Display)` pairs:
/// ```rust,ignore
/// i18n::t_args("api.buildFailed", &[("error", &err_msg)])
/// // equivalent to Python: t('api.buildFailed', error=err_msg)
/// ```
///
/// # Algorithm (every branch preserved from locale.py:35-63)
/// 1. locale = `get_locale()`
/// 2. `messages` = `translations[locale]` or `translations["zh"]` or `{}` (empty object)
/// 3. Traverse `key.split('.')` segments through the nested dict.  If any segment is missing
///    or the intermediate value is not an object, `value = None`.
/// 4. If `value is None` → **retry entire traversal against zh** (second pass).
/// 5. If still `None` → **return the key itself** (string passthrough).
/// 6. Replace `{name}` with the string representation of each kwarg value.
pub fn t_args(key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let locale = get_locale();
    let map = translations();

    // Step 2: get the root messages object for the current locale, fallback to zh, then empty.
    let messages = map
        .get(&locale)
        .or_else(|| map.get("zh"))
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));

    // Steps 3–5: traverse + zh-fallback + key passthrough.
    let value = traverse(&messages, key).or_else(|| {
        // Step 4: retry against zh
        map.get("zh").and_then(|zh| traverse(zh, key))
    });

    let mut result = match value {
        Some(s) => s,
        None => return key.to_string(), // Step 5: key passthrough
    };

    // Step 6: interpolate `{name}` placeholders.
    for (k, v) in args {
        let placeholder = format!("{{{k}}}");
        result = result.replace(&placeholder, &v.to_string());
    }

    result
}

/// Traverse a nested `serde_json::Value` by dot-separated key segments.
///
/// Returns `Some(String)` if the final value is a JSON string, `None` otherwise
/// (missing key, non-string leaf, or non-object intermediate — matching Python's
/// `value = None` branch on `not isinstance(value, dict)` mid-traverse).
fn traverse(root: &Value, key: &str) -> Option<String> {
    let mut current = root;
    for part in key.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None, // non-dict intermediate — Python sets value = None and breaks
        }
    }
    // Only accept string leaves (JSON translation values are always strings).
    match current {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// S-042 — get_language_instruction
// ---------------------------------------------------------------------------

/// Returns the LLM language instruction for the current locale.
///
/// Looks up `languages.json[locale].llmInstruction`, falling back to
/// `languages["en"].llmInstruction`, then hard-defaulting to `"Please respond in English."`.
///
/// Note: `languages.json` has 7 entries (zh/en/es/fr/pt/ru/de) — all 7 are embedded.
/// The lookup uses whatever locale `get_locale()` returns (including the 5 non-translation
/// locales: es/fr/pt/ru/de) because `languages.json` covers all 7.
pub fn get_language_instruction() -> String {
    let locale = get_locale();
    let langs = languages();

    langs
        .get(&locale)
        .and_then(|lc| lc.get("llmInstruction"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            langs
                .get("en")
                .and_then(|lc| lc.get("llmInstruction"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "Please respond in English.".to_string())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: run a future with a given locale set.
    async fn run_with<F, T>(locale: &str, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        with_locale(locale.to_string(), f).await
    }

    // -----------------------------------------------------------------------
    // S-040 get_locale
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_locale_defaults_to_en_when_unset() {
        // Outside any with_locale scope, LOCALE task-local is unset → "en" (English-first).
        let locale = get_locale();
        assert_eq!(locale, "en");
    }

    #[tokio::test]
    async fn get_locale_returns_set_value_inside_scope() {
        let locale = run_with("en", async { get_locale() }).await;
        assert_eq!(locale, "en");
    }

    #[tokio::test]
    async fn get_locale_unknown_locale_returns_stored_value_as_is() {
        // Task-local branch does NOT validate against translations() — Python's thread_local
        // branch also returns the stored value as-is (only the request-context branch validates).
        let locale = run_with("jp", async { get_locale() }).await;
        assert_eq!(locale, "jp");
    }

    #[tokio::test]
    async fn get_locale_reverts_after_scope_ends() {
        run_with("zh", async {}).await;
        // After the scope, task-local is unset again → English-first default.
        let locale = get_locale();
        assert_eq!(locale, "en");
    }

    // -----------------------------------------------------------------------
    // S-041 t — exact key hit (zh default)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn t_exact_key_hit_zh_default() {
        // Under an explicit zh scope → progress.taskComplete = "任务完成".
        let msg = run_with("zh", async { t("progress.taskComplete") }).await;
        assert_eq!(msg, "任务完成");
    }

    #[tokio::test]
    async fn t_exact_key_hit_zh_task_failed() {
        let msg = run_with("zh", async { t("progress.taskFailed") }).await;
        assert_eq!(msg, "任务失败");
    }

    // -----------------------------------------------------------------------
    // S-041 t — nested key traversal (deep api.* key)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn t_nested_key_traversal_api() {
        // api.projectNotFound exists in zh.json (tested under an explicit zh scope).
        let msg = run_with("zh", async { t("api.projectNotFound") }).await;
        assert_eq!(msg, "项目不存在: {id}");
    }

    #[tokio::test]
    async fn t_nested_key_traversal_en() {
        let msg = run_with("en", async { t("api.projectNotFound") }).await;
        assert_eq!(msg, "Project not found: {id}");
    }

    // -----------------------------------------------------------------------
    // S-041 t — missing key → returns key itself
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn t_missing_key_returns_key_string() {
        let key = "nonexistent.deeply.nested.key";
        let msg = t(key);
        assert_eq!(msg, key);
    }

    #[tokio::test]
    async fn t_missing_top_level_returns_key() {
        let key = "no_such_section.foo";
        assert_eq!(t(key), key);
    }

    // -----------------------------------------------------------------------
    // S-041 t — zh fallback when current locale lacks the key
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn t_falls_back_to_zh_when_locale_lacks_key() {
        // If we add a hypothetical key only in zh.json — for testing we use a known-zh-only
        // pattern: "zh" has keys, "en" also has them; but if we set an unknown locale (e.g. "jp")
        // the first pass finds nothing (jp not in translations), so it falls back to zh.
        let msg = run_with("jp", async { t("progress.taskComplete") }).await;
        // locale "jp" not in translations → messages = zh (Step 2: zh fallback in get)
        // or if "jp" not found, try zh path in Step 4.
        // Either way the zh string must emerge.
        assert_eq!(msg, "任务完成");
    }

    #[tokio::test]
    async fn t_zh_fallback_second_pass_when_en_missing_key() {
        // We cannot easily insert a synthetic zh-only key, but we can verify the second-pass
        // logic by testing a key that exists in zh but not en — the JSON files do have some
        // zh-only content (Chinese-only strings).  For robustness, test the established
        // behaviour: setting locale=en for a key present in en returns en value; setting
        // locale=unknown falls through to zh.
        //
        // Structural second-pass test: fake an unknown locale, check zh result surfaces.
        let result = run_with("xx_unknown", async { t("common.confirm") }).await;
        // "xx_unknown" not in translations → messages = zh → "确认"
        assert_eq!(result, "确认");
    }

    // -----------------------------------------------------------------------
    // S-041 t_args — placeholder interpolation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn t_args_interpolation_single_placeholder() {
        // api.buildFailed = "构建失败: {error}" in zh.json (tested under an explicit zh scope).
        let msg =
            run_with("zh", async { t_args("api.buildFailed", &[("error", &"timeout")]) }).await;
        assert_eq!(msg, "构建失败: timeout");
    }

    #[tokio::test]
    async fn t_args_interpolation_multi_placeholder() {
        // progress.sendingBatch = "发送第 {current}/{total} 批数据 ({chunks} 块)..." (zh scope).
        let msg = run_with("zh", async {
            t_args(
                "progress.sendingBatch",
                &[("current", &1i32), ("total", &5i32), ("chunks", &10i32)],
            )
        })
        .await;
        assert_eq!(msg, "发送第 1/5 批数据 (10 块)...");
    }

    #[tokio::test]
    async fn t_args_no_args_same_as_t() {
        let a = t("progress.taskComplete");
        let b = t_args("progress.taskComplete", &[]);
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn t_args_en_locale_interpolation() {
        // en.json progress.buildFailed = "Build failed: {error}"
        let msg =
            run_with("en", async { t_args("api.buildFailed", &[("error", &"network error")]) })
                .await;
        assert_eq!(msg, "Build failed: network error");
    }

    // -----------------------------------------------------------------------
    // S-042 get_language_instruction
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn get_language_instruction_default_en() {
        // LOCALE unset → en → "Please respond in English." (English-first default).
        let instr = get_language_instruction();
        assert_eq!(instr, "Please respond in English.");
    }

    #[tokio::test]
    async fn get_language_instruction_explicit_zh() {
        // Chinese instruction still served under an explicit zh scope.
        let instr = run_with("zh", async { get_language_instruction() }).await;
        assert_eq!(instr, "请使用中文回答。");
    }

    #[tokio::test]
    async fn get_language_instruction_en() {
        let instr = run_with("en", async { get_language_instruction() }).await;
        assert_eq!(instr, "Please respond in English.");
    }

    #[tokio::test]
    async fn get_language_instruction_es() {
        let instr = run_with("es", async { get_language_instruction() }).await;
        assert_eq!(instr, "Por favor, responde en español.");
    }

    #[tokio::test]
    async fn get_language_instruction_fr() {
        let instr = run_with("fr", async { get_language_instruction() }).await;
        assert_eq!(instr, "Veuillez répondre en français.");
    }

    #[tokio::test]
    async fn get_language_instruction_unknown_locale_falls_back_to_en() {
        // An unknown locale (not in languages.json) falls back to the English-first default.
        let instr = run_with("xx", async { get_language_instruction() }).await;
        assert_eq!(instr, "Please respond in English.");
    }

    // -----------------------------------------------------------------------
    // Embedded asset integrity
    // -----------------------------------------------------------------------

    #[test]
    fn translations_map_has_exactly_two_keys() {
        // S-038: only en and zh — no fabricated locales.
        let map = translations();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("zh"));
        assert!(map.contains_key("en"));
    }

    #[test]
    fn languages_map_has_seven_entries() {
        // S-042: all 7 locales from languages.json are present.
        let langs = languages();
        let obj = langs.as_object().expect("languages must be an object");
        assert_eq!(obj.len(), 7);
        for code in &["zh", "en", "es", "fr", "pt", "ru", "de"] {
            assert!(obj.contains_key(*code), "missing language: {code}");
        }
    }

    // -----------------------------------------------------------------------
    // with_locale nesting
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn with_locale_is_nestable() {
        let outer = run_with("en", async {
            // Outer scope is "en"
            let outer_val = get_locale();
            let inner_val = run_with("zh", async { get_locale() }).await;
            (outer_val, inner_val)
        })
        .await;
        // Outer restores to "en" after inner scope ends.
        assert_eq!(outer.0, "en");
        assert_eq!(outer.1, "zh");
    }
}
