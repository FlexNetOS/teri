//! User prompt-template store API (`/api/prompt-templates`).
//!
//! Lets the Home upload screen **save** the current simulation prompt + its seed documents as a
//! named, reusable template under a server-side folder, and **reload** a past one — so the prompt
//! and its seeds don't have to be retyped / re-uploaded every run. The Home dropdown is backed by
//! these routes.
//!
//! This is deliberately distinct from the read-only `/api/templates` *viewer* (`templates.rs`),
//! which exposes the engine's COMPILED LLM prompts. Here a "template" is USER content: a saved
//! simulation prompt + seed files.
//!
//! Storage layout — rooted at `{config.upload_folder}/templates/prompts/` (default
//! `./uploads/templates/prompts/`), one folder per template:
//! ```text
//! <slug>/
//!   meta.json          { "name", "prompt", "created_at", "seeds": ["a.pdf", ...] }
//!   seeds/<filename>   raw bytes (allowed extensions only: pdf/md/txt/markdown/json)
//! ```
//! The root is created lazily on first save; a fresh install simply lists `[]`.
//!
//! Routes (nested under `/api/prompt-templates` in `server.rs`):
//! - `GET    /`                      — list saved templates (newest first), `seeds` = filenames only
//! - `POST   /`                      — multipart `{name, prompt, files[]}` → save (overwrites same name)
//! - `GET    /:id`                   — one template's meta (name, prompt, seed filenames)
//! - `GET    /:id/seeds/:filename`   — raw bytes of one seed (so the UI can reconstruct a `File`)
//! - `DELETE /:id`                   — remove a template

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, Path as AxumPath, State},
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::get,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiState};

/// Persisted descriptor of a saved template (the `meta.json` shape + the derived `id`).
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct PromptTemplate {
    /// URL-safe slug derived from the name; the on-disk folder name and the `:id` path param.
    /// Skipped on disk (it IS the folder name) — re-derived on read.
    #[serde(default)]
    pub id: String,
    /// Human-entered display name.
    pub name: String,
    /// The saved simulation prompt text.
    pub prompt: String,
    /// RFC3339 timestamp of when the template was saved.
    pub created_at: String,
    /// Seed document filenames stored under `<slug>/seeds/`.
    #[serde(default)]
    pub seeds: Vec<String>,
}

/// Root folder for the prompt-template store: `{upload_folder}/templates/prompts`.
fn templates_root(config: &crate::Config) -> PathBuf {
    Path::new(&config.upload_folder).join("templates").join("prompts")
}

/// Derive a URL-safe, filesystem-safe slug from a display name.
///
/// Lowercases, maps any run of non-alphanumeric chars to a single `-`, trims leading/trailing
/// `-`, and caps length. Empty / all-symbol names fall back to `untitled` so a save never lands
/// on an empty folder name. This is also what makes a re-save of the same name idempotent
/// (same slug → same folder, overwritten).
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut prev_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let slug = out.trim_matches('-').to_string();
    let slug: String = slug.chars().take(80).collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() { "untitled".to_string() } else { slug }
}

/// Return the basename of `name` iff it is a safe single path component (no separators, no `..`).
/// Guards `:id` and `:filename` path params against traversal.
fn safe_component(name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return None;
    }
    // Defensive: take only the basename component.
    let base = Path::new(name).file_name()?.to_str()?.to_string();
    if base.is_empty() { None } else { Some(base) }
}

/// Allowed seed extensions — mirrors the engine's canonical set via `seed::is_allowed_ext`
/// (pdf/md/txt/markdown/json), so a template can only hold seeds the pipeline can actually ingest.
fn allowed_seed(filename: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(crate::seed::is_allowed_ext)
        .unwrap_or(false)
}

/// Read one template folder into a `PromptTemplate`, or `None` if it isn't a valid template.
fn read_template_dir(dir: &Path) -> Option<PromptTemplate> {
    let id = dir.file_name()?.to_str()?.to_string();
    let meta_raw = std::fs::read_to_string(dir.join("meta.json")).ok()?;
    let mut tpl: PromptTemplate = serde_json::from_str(&meta_raw).ok()?;
    tpl.id = id;
    Some(tpl)
}

/// List all saved templates, newest first (by `created_at`, falling back to name).
/// A missing/unreadable root yields an empty list (fresh install is not an error).
fn list_templates(root: &Path) -> Vec<PromptTemplate> {
    let mut out: Vec<PromptTemplate> = match std::fs::read_dir(root) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| read_template_dir(&e.path()))
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Load one template by id, or `None` if absent / id is unsafe.
fn load_template(root: &Path, id: &str) -> Option<PromptTemplate> {
    let id = safe_component(id)?;
    read_template_dir(&root.join(id))
}

/// Load one seed file's raw bytes, or `None` if absent / id or filename is unsafe.
fn load_seed(root: &Path, id: &str, filename: &str) -> Option<Vec<u8>> {
    let id = safe_component(id)?;
    let filename = safe_component(filename)?;
    std::fs::read(root.join(id).join("seeds").join(filename)).ok()
}

/// Save (create or overwrite) a template. Pure core of `POST /` so tests can drive it without
/// constructing a multipart body. Returns the persisted descriptor.
///
/// - `name` and `prompt` must be non-empty (the dropdown always supplies both).
/// - Only allowed-extension, safely-named seed files are written; others are skipped.
/// - An existing template with the same slug is fully replaced (idempotent re-save).
fn save_template(
    root: &Path,
    name: &str,
    prompt: &str,
    files: &[(String, Vec<u8>)],
    created_at: String,
) -> Result<PromptTemplate, ApiError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, "template name is required"));
    }
    if prompt.trim().is_empty() {
        return Err(ApiError::client(StatusCode::BAD_REQUEST, "template prompt is required"));
    }

    let id = slugify(name);
    let dir = root.join(&id);
    // Clean replace: drop any prior contents so removed seeds don't linger.
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(ApiError::server)?;
    }
    let seeds_dir = dir.join("seeds");
    std::fs::create_dir_all(&seeds_dir).map_err(ApiError::server)?;

    let mut seed_names: Vec<String> = Vec::new();
    for (filename, bytes) in files {
        let Some(base) = safe_component(filename) else { continue };
        if !allowed_seed(&base) {
            continue;
        }
        std::fs::write(seeds_dir.join(&base), bytes).map_err(ApiError::server)?;
        seed_names.push(base);
    }

    let tpl = PromptTemplate {
        id: id.clone(),
        name: name.to_string(),
        prompt: prompt.to_string(),
        created_at,
        seeds: seed_names,
    };
    let meta = serde_json::to_string_pretty(&tpl).map_err(ApiError::server)?;
    std::fs::write(dir.join("meta.json"), meta).map_err(ApiError::server)?;
    Ok(tpl)
}

/// Delete a template folder. Returns `true` if something was removed.
fn delete_template(root: &Path, id: &str) -> bool {
    let Some(id) = safe_component(id) else { return false };
    let dir = root.join(id);
    if dir.is_dir() { std::fs::remove_dir_all(&dir).is_ok() } else { false }
}

// ───────────────────────────── axum handlers (thin) ─────────────────────────────

/// `GET /api/prompt-templates` — list saved templates (newest first).
async fn list_route(State(state): State<Arc<ApiState>>) -> Json<Value> {
    let templates = list_templates(&templates_root(&state.config));
    Json(serde_json::to_value(&templates).expect("PromptTemplate is always serializable"))
}

/// `GET /api/prompt-templates/:id` — one template's metadata.
async fn get_route(
    State(state): State<Arc<ApiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    match load_template(&templates_root(&state.config), &id) {
        Some(tpl) => Ok(Json(serde_json::to_value(&tpl).expect("serializable"))),
        None => Err(ApiError::client(StatusCode::NOT_FOUND, "template not found")),
    }
}

/// `GET /api/prompt-templates/:id/seeds/:filename` — raw bytes of one seed document.
async fn seed_route(
    State(state): State<Arc<ApiState>>,
    AxumPath((id, filename)): AxumPath<(String, String)>,
) -> Response {
    match load_seed(&templates_root(&state.config), &id, &filename) {
        Some(bytes) => {
            let disposition = format!("attachment; filename=\"{}\"", filename.replace('"', ""));
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                bytes,
            )
                .into_response()
        }
        None => ApiError::client(StatusCode::NOT_FOUND, "seed not found").into_response(),
    }
}

/// `POST /api/prompt-templates` — save the current prompt + seeds as a named template.
///
/// Multipart fields: `name` (text), `prompt` (text), `files` (≥0 file fields).
async fn save_route(
    State(state): State<Arc<ApiState>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, ApiError> {
    let mut name = String::new();
    let mut prompt = String::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(field) = multipart.next_field().await.map_err(ApiError::server)? {
        match field.name().unwrap_or("").to_string().as_str() {
            "name" => name = field.text().await.map_err(ApiError::server)?,
            "prompt" => prompt = field.text().await.map_err(ApiError::server)?,
            "files" => {
                let filename = field.file_name().unwrap_or("").to_string();
                let bytes = field.bytes().await.map_err(ApiError::server)?;
                if !filename.is_empty() {
                    files.push((filename, bytes.to_vec()));
                }
            }
            _ => {
                let _ = field.bytes().await; // drain unknown field
            }
        }
    }

    let created_at = chrono::Utc::now().to_rfc3339();
    let tpl = save_template(&templates_root(&state.config), &name, &prompt, &files, created_at)?;
    Ok(Json(json!({ "success": true, "template": tpl })))
}

/// `DELETE /api/prompt-templates/:id` — remove a saved template.
async fn delete_route(
    State(state): State<Arc<ApiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    if delete_template(&templates_root(&state.config), &id) {
        Ok(Json(json!({ "success": true })))
    } else {
        Err(ApiError::client(StatusCode::NOT_FOUND, "template not found"))
    }
}

/// Build the `/prompt-templates` sub-router. Composes under the `/api` nest in `server.rs`.
/// The body limit (from `Config::max_content_length`) covers the multipart save.
pub fn prompt_templates_router(state: Arc<ApiState>) -> Router {
    let upload_limit = state.config.max_content_length as usize;
    Router::new()
        .route("/", get(list_route).post(save_route))
        .route("/:id", get(get_route).delete(delete_route))
        .route("/:id/seeds/:filename", get(seed_route))
        .layer(DefaultBodyLimit::max(upload_limit))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique temp root per test (no Date/rand in the engine; use pid + a caller-supplied tag).
    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("teri-prompt-tpl-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn slugify_handles_names_and_edges() {
        assert_eq!(slugify("Election Rumor"), "election-rumor");
        assert_eq!(slugify("  Product   Launch!! "), "product-launch");
        assert_eq!(slugify("中文 标题"), "untitled"); // all non-ascii-alnum → fallback
        assert_eq!(slugify("---"), "untitled");
        assert_eq!(slugify("a/b\\c"), "a-b-c");
    }

    #[test]
    fn safe_component_rejects_traversal() {
        assert!(safe_component("ok.pdf").is_some());
        assert!(safe_component("../etc/passwd").is_none());
        assert!(safe_component("a/b").is_none());
        assert!(safe_component("..").is_none());
        assert!(safe_component("").is_none());
    }

    #[test]
    fn allowed_seed_matches_engine_set() {
        assert!(allowed_seed("doc.pdf"));
        assert!(allowed_seed("notes.md"));
        assert!(allowed_seed("a.txt"));
        assert!(!allowed_seed("evil.exe"));
        assert!(!allowed_seed("noext"));
    }

    #[test]
    fn save_list_load_seed_delete_roundtrip() {
        let root = temp_root("roundtrip");
        let files = vec![
            ("brief.md".to_string(), b"# seed one".to_vec()),
            ("data.txt".to_string(), b"seed two".to_vec()),
            ("evil.exe".to_string(), b"nope".to_vec()), // skipped (disallowed ext)
        ];
        let saved = save_template(
            &root,
            "Election Rumor",
            "predict the spread",
            &files,
            "2026-01-01T00:00:00Z".into(),
        )
        .expect("save ok");
        assert_eq!(saved.id, "election-rumor");
        assert_eq!(saved.seeds.len(), 2, "evil.exe must be skipped");

        // list
        let all = list_templates(&root);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Election Rumor");
        assert_eq!(all[0].prompt, "predict the spread");

        // load meta
        let one = load_template(&root, "election-rumor").expect("found");
        assert_eq!(one.seeds.len(), 2);

        // load a seed's bytes
        let bytes = load_seed(&root, "election-rumor", "brief.md").expect("seed bytes");
        assert_eq!(bytes, b"# seed one");
        // traversal guarded
        assert!(load_seed(&root, "election-rumor", "../meta.json").is_none());

        // delete
        assert!(delete_template(&root, "election-rumor"));
        assert!(list_templates(&root).is_empty());
        assert!(!delete_template(&root, "election-rumor")); // already gone

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resave_same_name_overwrites_and_drops_old_seeds() {
        let root = temp_root("overwrite");
        save_template(
            &root,
            "Trial",
            "v1",
            &[("a.md".into(), b"a".to_vec())],
            "2026-01-01T00:00:00Z".into(),
        )
        .expect("save v1");
        // Re-save same name with a different seed set.
        let v2 = save_template(
            &root,
            "Trial",
            "v2",
            &[("b.txt".into(), b"b".to_vec())],
            "2026-01-02T00:00:00Z".into(),
        )
        .expect("save v2");
        assert_eq!(v2.prompt, "v2");
        assert_eq!(v2.seeds, vec!["b.txt".to_string()]);
        // Only one folder, and the old seed is gone.
        assert_eq!(list_templates(&root).len(), 1);
        assert!(load_seed(&root, "trial", "a.md").is_none());
        assert!(load_seed(&root, "trial", "b.txt").is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_requires_name_and_prompt() {
        let root = temp_root("validate");
        assert!(save_template(&root, "  ", "p", &[], "t".into()).is_err());
        assert!(save_template(&root, "n", "  ", &[], "t".into()).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn list_route_empty_on_fresh_install() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        // Point upload_folder at a guaranteed-empty temp dir.
        let mut config = crate::Config::build_test();
        config.upload_folder = temp_root("fresh").to_string_lossy().to_string();
        let app = crate::server::create_app(Arc::new(ApiState::new(config)));

        let resp = app
            .oneshot(Request::builder().uri("/api/prompt-templates").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.as_array().expect("array").len(), 0);
    }
}
