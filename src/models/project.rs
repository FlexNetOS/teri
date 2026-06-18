//! Project persistence model.
//!
//! Port of `backend/app/models/project.py` (MiroFish, 306 lines).
//! Provides filesystem-backed project state: create, save, get, list, delete,
//! file upload, extracted-text storage.
//!
//! # Symbols ported: S-098..S-137
//!
//! ## Timestamp choice (S-129 / S-130 note — flagged for verifier)
//! Python uses `datetime.now().isoformat()` — local naive time, no timezone suffix,
//! microsecond fraction omitted when µs==0.  We use `chrono::Local::now().naive_local()`
//! to match `datetime.now()` semantics (local, naive).  The same µs-omission rule from
//! `task.rs::python_isoformat` is applied.  The tz offset is NOT appended because
//! `datetime.now()` (no `tz=` argument) returns a naive datetime (no +HH:MM suffix).
//! Divergence from task.rs (which uses Utc) is intentional: project.py != task.py here.
//! created_at / updated_at are used only as opaque sortable strings within teri, so
//! local-naive is a strict parity match.
//!
//! ## get_project missing-vs-corrupt faithfulness (S-131)
//! Missing project.json → `Ok(None)`.
//! File present but contains invalid JSON OR missing `project_id` key → `Err(...)`.
//! This matches the Python code exactly: `not os.path.exists(meta_path)` → return None;
//! `json.load(f)` raises `json.JSONDecodeError` → propagates (uncaught); `data['project_id']`
//! raises `KeyError` → propagates (uncaught).  The ledger summary "returns None on corrupt"
//! is incorrect; the actual Python source propagates errors on corrupt files.
//!
//! ## delete_project absent→false faithfulness (S-133)
//! Python: `if not os.path.exists(project_dir): return False`.  This is NOT a raised error.
//! Rust: returns `Ok(false)`.  No Err variant on absent directory.

use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{Result, TeriError};

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

/// Format a local naive datetime exactly like Python's `datetime.now().isoformat()`:
/// emit the microsecond fraction ONLY when it is non-zero (Python omits `.000000`
/// for whole-second times), with NO timezone suffix (naive datetime).
///
/// This is `pub(crate)` so that `sim::action_logger` can reuse it — both modules
/// call Python's `datetime.now().isoformat()` which has the same local-naive semantics.
pub(crate) fn python_isoformat_local() -> String {
    let now = Local::now().naive_local();
    let micros = now.and_utc().timestamp_subsec_micros();
    if micros == 0 {
        now.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        now.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
    }
}

// ---------------------------------------------------------------------------
// ProjectStatus  (S-098..S-103)
// ---------------------------------------------------------------------------

/// Project lifecycle state.
///
/// Serialises to the same lowercase string values as the Python `ProjectStatus(str, Enum)`:
/// `"created"`, `"ontology_generated"`, `"graph_building"`, `"graph_completed"`, `"failed"`.
///
/// `serde(rename_all = "snake_case")` produces the right wire values for all five variants
/// because snake_case on `OntologyGenerated` → `ontology_generated`, etc.  Single-word
/// variants (`Created`, `Failed`) also collapse correctly to `"created"` / `"failed"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    /// Just created; files uploaded.  Python value: `"created"`.
    Created,
    /// Ontology has been generated.  Python value: `"ontology_generated"`.
    OntologyGenerated,
    /// Graph is being built.  Python value: `"graph_building"`.
    GraphBuilding,
    /// Graph construction is complete.  Python value: `"graph_completed"`.
    GraphCompleted,
    /// Terminal failure.  Python value: `"failed"`.
    Failed,
}

impl ProjectStatus {
    /// Returns the exact string value the Python enum `.value` property returns.
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectStatus::Created => "created",
            ProjectStatus::OntologyGenerated => "ontology_generated",
            ProjectStatus::GraphBuilding => "graph_building",
            ProjectStatus::GraphCompleted => "graph_completed",
            ProjectStatus::Failed => "failed",
        }
    }
}

// ---------------------------------------------------------------------------
// ProjectFile  (keys from S-134 / Python lines 267-272)
// ---------------------------------------------------------------------------

/// One uploaded file record stored inside `Project.files`.
///
/// Python stores this as a plain `Dict[str, str]` (despite `size` being an int at runtime;
/// the type hint says `str` but `os.path.getsize` returns int — we use `i64` to match
/// the actual runtime value).  The four keys map 1-to-1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProjectFile {
    pub original_filename: String,
    pub saved_filename: String,
    pub path: String,
    pub size: i64,
}

// ---------------------------------------------------------------------------
// Project  (S-104..S-121)
// ---------------------------------------------------------------------------

/// Project data model.
///
/// Field-by-field mapping vs. Python dataclass (project.py lines 28-53):
///
/// | Python field             | Rust field               | Type                       |
/// |--------------------------|--------------------------|----------------------------|
/// | `project_id`             | `project_id`             | `String`                   |
/// | `name`                   | `name`                   | `String`                   |
/// | `status`                 | `status`                 | `ProjectStatus`            |
/// | `created_at`             | `created_at`             | `String`                   |
/// | `updated_at`             | `updated_at`             | `String`                   |
/// | `files`                  | `files`                  | `Vec<Value>`               |
/// | `total_text_length`      | `total_text_length`      | `i64`                      |
/// | `ontology`               | `ontology`               | `Option<Value>`            |
/// | `analysis_summary`       | `analysis_summary`       | `Option<String>`           |
/// | `graph_id`               | `graph_id`               | `Option<String>`           |
/// | `graph_build_task_id`    | `graph_build_task_id`    | `Option<String>`           |
/// | `simulation_requirement` | `simulation_requirement` | `Option<String>`           |
/// | `chunk_size`             | `chunk_size`             | `i64`                      |
/// | `chunk_overlap`          | `chunk_overlap`          | `i64`                      |
/// | `error`                  | `error`                  | `Option<String>`           |
///
/// ## `files` field type choice (FAIL #2 fix)
///
/// Python stores `files` as `List[Dict[str, str]]` and never validates individual dict shapes.
/// The Python `data.get('files', [])` call keeps any array verbatim — including the legacy
/// 3-key form `{"filename", "path", "size"}` (project.py line 36).  Using `Vec<ProjectFile>`
/// with `.ok().unwrap_or_default()` caused silent full-vector collapse on any non-matching entry.
/// `Vec<Value>` matches Python's untyped behaviour: any array of JSON objects is preserved
/// verbatim through from_dict → to_dict round-trips.  `save_file_to_project` still pushes a
/// 4-key object (its own output shape is unchanged) — it just does so as a `serde_json::Value`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub project_id: String,
    pub name: String,
    /// Serialised as its string value (e.g. `"created"`) — see `to_dict`.
    pub status: ProjectStatus,
    pub created_at: String,
    pub updated_at: String,
    /// Raw JSON objects; never parsed per-element — matches Python's untyped `List[Dict[str,str]]`.
    #[serde(default)]
    pub files: Vec<Value>,
    #[serde(default)]
    pub total_text_length: i64,
    #[serde(default)]
    pub ontology: Option<Value>,
    #[serde(default)]
    pub analysis_summary: Option<String>,
    #[serde(default)]
    pub graph_id: Option<String>,
    #[serde(default)]
    pub graph_build_task_id: Option<String>,
    #[serde(default)]
    pub simulation_requirement: Option<String>,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: i64,
    #[serde(default = "default_chunk_overlap")]
    pub chunk_overlap: i64,
    #[serde(default)]
    pub error: Option<String>,
}

fn default_chunk_size() -> i64 {
    500
}
fn default_chunk_overlap() -> i64 {
    50
}

impl Project {
    // -----------------------------------------------------------------------
    // S-120 to_dict
    // -----------------------------------------------------------------------

    /// Serialise to a JSON `Value` with all 14 keys, status as its string value,
    /// and `None` fields as JSON `null`.
    ///
    /// Wire-identical to Python's `to_dict()` + `json.dump(..., ensure_ascii=False, indent=2)`.
    /// Non-ASCII characters (e.g. Chinese) are NOT escaped — `serde_json::to_string_pretty`
    /// matches `ensure_ascii=False`.
    pub fn to_dict(&self) -> Value {
        serde_json::json!({
            "project_id": self.project_id,
            "name": self.name,
            "status": self.status.as_str(),
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "files": self.files,
            "total_text_length": self.total_text_length,
            "ontology": self.ontology,
            "analysis_summary": self.analysis_summary,
            "graph_id": self.graph_id,
            "graph_build_task_id": self.graph_build_task_id,
            "simulation_requirement": self.simulation_requirement,
            "chunk_size": self.chunk_size,
            "chunk_overlap": self.chunk_overlap,
            "error": self.error,
        })
    }

    // -----------------------------------------------------------------------
    // S-121 from_dict
    // -----------------------------------------------------------------------

    /// Deserialise from a JSON `Value` with Python's `.get(key, default)` tolerance:
    ///
    /// | Key                  | Missing behaviour          |
    /// |----------------------|---------------------------|
    /// | `project_id`         | **Error** (`data['project_id']` raises `KeyError` in Python) |
    /// | `name`               | `"Unnamed Project"`        |
    /// | `status`             | `"created"` → `Created`   |
    /// | `created_at`         | `""` (empty string)        |
    /// | `updated_at`         | `""` (empty string)        |
    /// | `files`              | `[]`                       |
    /// | `total_text_length`  | `0`                        |
    /// | `chunk_size`         | `500`                      |
    /// | `chunk_overlap`      | `50`                       |
    /// | optionals (ontology, analysis_summary, …) | `None` |
    ///
    /// An unknown / invalid `status` string causes a `TeriError::Config` (Python would
    /// raise `ValueError` from `ProjectStatus(status)`).
    pub fn from_dict(data: &Value) -> Result<Self> {
        let obj = data
            .as_object()
            .ok_or_else(|| TeriError::Config("project data is not a JSON object".to_string()))?;

        // project_id is REQUIRED — KeyError in Python if absent.
        let project_id = obj
            .get("project_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TeriError::Config("missing required field: project_id".to_string()))?
            .to_string();

        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unnamed Project")
            .to_string();

        let status_str = obj.get("status").and_then(|v| v.as_str()).unwrap_or("created");
        let status: ProjectStatus = serde_json::from_value(Value::String(status_str.to_string()))
            .map_err(|_| {
            TeriError::Config(format!("invalid project status: {status_str:?}"))
        })?;

        let created_at = obj.get("created_at").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let updated_at = obj.get("updated_at").and_then(|v| v.as_str()).unwrap_or("").to_string();

        // Python: data.get('files', []) — keep verbatim, never validate per-element shape.
        // Using strict per-element parse caused silent full-vector collapse on legacy 3-key dicts.
        let files: Vec<Value> =
            obj.get("files").and_then(|v| v.as_array().cloned()).unwrap_or_default();

        let total_text_length = obj.get("total_text_length").and_then(|v| v.as_i64()).unwrap_or(0);

        let ontology = obj
            .get("ontology")
            .and_then(|v| if v.is_null() { None } else { Some(v.clone()) });

        let analysis_summary =
            obj.get("analysis_summary").and_then(|v| v.as_str()).map(|s| s.to_string());

        let graph_id = obj.get("graph_id").and_then(|v| v.as_str()).map(|s| s.to_string());

        let graph_build_task_id =
            obj.get("graph_build_task_id").and_then(|v| v.as_str()).map(|s| s.to_string());

        let simulation_requirement = obj
            .get("simulation_requirement")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let chunk_size = obj.get("chunk_size").and_then(|v| v.as_i64()).unwrap_or(500);

        let chunk_overlap = obj.get("chunk_overlap").and_then(|v| v.as_i64()).unwrap_or(50);

        let error = obj.get("error").and_then(|v| v.as_str()).map(|s| s.to_string());

        Ok(Project {
            project_id,
            name,
            status,
            created_at,
            updated_at,
            files,
            total_text_length,
            ontology,
            analysis_summary,
            graph_id,
            graph_build_task_id,
            simulation_requirement,
            chunk_size,
            chunk_overlap,
            error,
        })
    }
}

// ---------------------------------------------------------------------------
// ProjectManager  (S-122..S-137)
// ---------------------------------------------------------------------------

/// Filesystem-backed project storage.
///
/// Stateless in Rust (class methods → free functions on the struct).  The base directory
/// (`projects_dir`) is derived from `Config.upload_folder` at construction time, matching
/// Python's `PROJECTS_DIR = os.path.join(Config.UPLOAD_FOLDER, 'projects')`.
///
/// For testability, `ProjectManager::new(base)` accepts any `PathBuf`; the production
/// path is `{config.upload_folder}/projects` — constructed by `ProjectManager::from_config`.
pub struct ProjectManager {
    /// S-123: PROJECTS_DIR = `{upload_folder}/projects`
    projects_dir: PathBuf,
}

impl ProjectManager {
    // -----------------------------------------------------------------------
    // Constructors
    // -----------------------------------------------------------------------

    /// Create a `ProjectManager` with an explicit projects directory.
    /// Used in tests to point at a temp dir.
    pub fn new(projects_dir: impl Into<PathBuf>) -> Self {
        ProjectManager { projects_dir: projects_dir.into() }
    }

    /// Create a `ProjectManager` from teri's `Config`.
    /// Faithfully implements `PROJECTS_DIR = os.path.join(Config.UPLOAD_FOLDER, 'projects')`.
    pub fn from_config(config: &crate::config::Config) -> Self {
        let projects_dir = Path::new(&config.upload_folder).join("projects");
        ProjectManager { projects_dir }
    }

    // -----------------------------------------------------------------------
    // S-124 _ensure_projects_dir
    // -----------------------------------------------------------------------

    /// Ensure the projects root directory exists (== `os.makedirs(exist_ok=True)`).
    fn ensure_projects_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.projects_dir)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // S-125..S-128 path helpers
    // -----------------------------------------------------------------------

    /// S-125: `{PROJECTS_DIR}/{project_id}`
    fn project_dir(&self, project_id: &str) -> PathBuf {
        self.projects_dir.join(project_id)
    }

    /// S-126: `{project_dir}/project.json`
    fn project_meta_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("project.json")
    }

    /// S-127: `{project_dir}/files`
    fn project_files_dir(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("files")
    }

    /// S-128: `{project_dir}/extracted_text.txt`
    fn project_text_path(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("extracted_text.txt")
    }

    // -----------------------------------------------------------------------
    // S-129 create_project
    // -----------------------------------------------------------------------

    /// Create a new project.
    ///
    /// * `project_id` = `"proj_" + uuid4().hex[:12]` (12 hex chars, no hyphens)
    /// * `now` = local naive ISO timestamp (see module-level timestamp note)
    /// * Creates `{projects_dir}/{project_id}/` and `…/files/`
    /// * Writes `project.json` via `save_project`
    pub fn create_project(&self, name: &str) -> Result<Project> {
        self.ensure_projects_dir()?;

        // "proj_" + uuid4().hex[:12] — 12 lowercase hex chars, no hyphens
        let hex = Uuid::new_v4().simple().to_string(); // 32 hex chars, no hyphens
        let project_id = format!("proj_{}", &hex[..12]);

        let now = python_isoformat_local();

        let mut project = Project {
            project_id: project_id.clone(),
            name: name.to_string(),
            status: ProjectStatus::Created,
            created_at: now.clone(),
            updated_at: now,
            files: Vec::new(),
            total_text_length: 0,
            ontology: None,
            analysis_summary: None,
            graph_id: None,
            graph_build_task_id: None,
            simulation_requirement: None,
            chunk_size: 500,
            chunk_overlap: 50,
            error: None,
        };

        // Create project dir + files subdir
        std::fs::create_dir_all(self.project_dir(&project_id))?;
        std::fs::create_dir_all(self.project_files_dir(&project_id))?;

        // Save metadata — mutates project.updated_at to the save-time timestamp (same object,
        // matching Python's `save_project(project)` which sets project.updated_at in-place).
        // FAIL #1 fix: was `save_project(&mut project.clone())` which left the returned object
        // with updated_at == created_at; now we pass &mut project so the returned object's
        // updated_at == the persisted value (save-time, >= created_at).
        self.save_project(&mut project)?;

        Ok(project)
    }

    // -----------------------------------------------------------------------
    // S-130 save_project
    // -----------------------------------------------------------------------

    /// Persist project metadata to `project.json`.
    ///
    /// MUTATES `project.updated_at` to the current local timestamp before writing —
    /// this matches Python line 170: `project.updated_at = datetime.now().isoformat()`.
    ///
    /// The file is written as pretty-printed JSON (`serde_json::to_string_pretty`), which
    /// does NOT escape non-ASCII characters, matching Python's `ensure_ascii=False, indent=2`.
    pub fn save_project(&self, project: &mut Project) -> Result<()> {
        project.updated_at = python_isoformat_local();
        let meta_path = self.project_meta_path(&project.project_id);
        let json = serde_json::to_string_pretty(&project.to_dict())?;
        std::fs::write(&meta_path, json.as_bytes())?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // S-131 get_project
    // -----------------------------------------------------------------------

    /// Load a project by ID.
    ///
    /// * `project.json` absent → `Ok(None)` (matches Python `if not os.path.exists: return None`)
    /// * `project.json` present but corrupt JSON → `Err(TeriError::Json(...))`
    ///   (matches Python's uncaught `json.JSONDecodeError`)
    /// * `project.json` present, valid JSON, but missing `project_id` key → `Err(TeriError::Config(...))`
    ///   (matches Python's uncaught `KeyError` from `data['project_id']`)
    pub fn get_project(&self, project_id: &str) -> Result<Option<Project>> {
        let meta_path = self.project_meta_path(project_id);

        if !meta_path.exists() {
            return Ok(None);
        }

        let raw = std::fs::read_to_string(&meta_path)?;
        let data: Value = serde_json::from_str(&raw)?; // corrupt JSON → Err
        let project = Project::from_dict(&data)?; // missing project_id → Err
        Ok(Some(project))
    }

    // -----------------------------------------------------------------------
    // S-132 list_projects
    // -----------------------------------------------------------------------

    /// List all projects, sorted by `created_at` descending, up to `limit`.
    ///
    /// Faithfulness notes:
    /// * Calls `ensure_projects_dir` first (Python does `cls._ensure_projects_dir()`).
    /// * Iterates directory entries; calls `get_project(entry_name)` on each.
    /// * Entries for which `get_project` returns `None` are silently skipped
    ///   (no `project.json` = not a project dir, or it's a non-dir entry).
    /// * A corrupt `project.json` propagates as `Err` — same as Python (no catch around
    ///   `get_project` in `list_projects`).
    /// * Sort is by `created_at` descending (reverse); then take `limit`.
    pub fn list_projects(&self, limit: usize) -> Result<Vec<Project>> {
        self.ensure_projects_dir()?;

        let mut projects: Vec<Project> = Vec::new();

        for entry in std::fs::read_dir(&self.projects_dir)? {
            let entry = entry?;
            let entry_name = entry.file_name();
            let name_str = entry_name.to_string_lossy();
            if let Some(project) = self.get_project(name_str.as_ref())? {
                projects.push(project);
            }
        }

        // Sort by created_at descending (reverse)
        projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(projects.into_iter().take(limit).collect())
    }

    // -----------------------------------------------------------------------
    // S-133 delete_project
    // -----------------------------------------------------------------------

    /// Delete a project and all its files.
    ///
    /// * Project dir absent → `Ok(false)` (matches Python `if not os.path.exists: return False`)
    /// * Project dir present → `remove_dir_all` + `Ok(true)`
    ///
    /// This is NOT an error on absent directory — the Python code returns `False`, not raises.
    pub fn delete_project(&self, project_id: &str) -> Result<bool> {
        let project_dir = self.project_dir(project_id);

        if !project_dir.exists() {
            return Ok(false);
        }

        std::fs::remove_dir_all(&project_dir)?;
        Ok(true)
    }

    // -----------------------------------------------------------------------
    // S-134 save_file_to_project
    // -----------------------------------------------------------------------

    /// Save uploaded file bytes to the project's `files/` subdirectory.
    ///
    /// * `file_bytes`: the raw content (maps to Python's `FileStorage.save(path)`)
    /// * `original_filename`: the user-supplied filename (used to extract the extension)
    ///
    /// Filename generation:
    /// * `ext` = lowercased extension including the leading `.` (like `os.path.splitext`)
    ///   e.g. `"Report.PDF"` → `".pdf"`.  If no extension, `ext = ""`.
    /// * `safe_filename` = `uuid4().hex[:8]` + ext  (8 hex chars)
    ///
    /// Returns a `ProjectFile` with the same 4 keys Python returns (lines 267-272):
    /// `original_filename`, `saved_filename`, `path` (absolute string), `size`.
    pub fn save_file_to_project(
        &self,
        project_id: &str,
        file_bytes: &[u8],
        original_filename: &str,
    ) -> Result<ProjectFile> {
        let files_dir = self.project_files_dir(project_id);
        std::fs::create_dir_all(&files_dir)?;

        // Extension: os.path.splitext(original_filename)[1].lower()
        let ext = Path::new(original_filename)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
            .unwrap_or_default();

        // uuid4().hex[:8] — 8 lowercase hex chars, no hyphens
        let hex = Uuid::new_v4().simple().to_string();
        let safe_filename = format!("{}{}", &hex[..8], ext);

        let file_path = files_dir.join(&safe_filename);
        std::fs::write(&file_path, file_bytes)?;

        let size = file_bytes.len() as i64;

        Ok(ProjectFile {
            original_filename: original_filename.to_string(),
            saved_filename: safe_filename,
            path: file_path.to_string_lossy().into_owned(),
            size,
        })
    }

    // -----------------------------------------------------------------------
    // S-135 save_extracted_text
    // -----------------------------------------------------------------------

    /// Write extracted text to `{project_dir}/extracted_text.txt` (UTF-8, overwrite).
    pub fn save_extracted_text(&self, project_id: &str, text: &str) -> Result<()> {
        let text_path = self.project_text_path(project_id);
        std::fs::write(&text_path, text.as_bytes())?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // S-136 get_extracted_text
    // -----------------------------------------------------------------------

    /// Read extracted text from `{project_dir}/extracted_text.txt`.
    ///
    /// * File absent → `Ok(None)` (matches Python `if not os.path.exists: return None`)
    /// * File present → `Ok(Some(content))`
    pub fn get_extracted_text(&self, project_id: &str) -> Result<Option<String>> {
        let text_path = self.project_text_path(project_id);

        if !text_path.exists() {
            return Ok(None);
        }

        let text = std::fs::read_to_string(&text_path)?;
        Ok(Some(text))
    }

    // -----------------------------------------------------------------------
    // S-137 get_project_files
    // -----------------------------------------------------------------------

    /// List all file paths in `{project_dir}/files/`.
    ///
    /// * `files/` dir absent → empty `Vec` (matches Python `if not os.path.exists: return []`)
    /// * Returns full path strings for entries that are FILES (not dirs), matching Python's
    ///   `os.path.isfile` filter.
    pub fn get_project_files(&self, project_id: &str) -> Result<Vec<String>> {
        let files_dir = self.project_files_dir(project_id);

        if !files_dir.exists() {
            return Ok(Vec::new());
        }

        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&files_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                paths.push(entry.path().to_string_lossy().into_owned());
            }
        }

        Ok(paths)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn temp_manager() -> (ProjectManager, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let pm = ProjectManager::new(dir.path().join("projects"));
        (pm, dir)
    }

    // -----------------------------------------------------------------------
    // S-098..S-103 ProjectStatus serde round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_project_status_serde_values() {
        // Each variant must serialise to its exact Python .value string
        let cases: &[(&str, ProjectStatus)] = &[
            ("created", ProjectStatus::Created),
            ("ontology_generated", ProjectStatus::OntologyGenerated),
            ("graph_building", ProjectStatus::GraphBuilding),
            ("graph_completed", ProjectStatus::GraphCompleted),
            ("failed", ProjectStatus::Failed),
        ];
        for (expected, variant) in cases {
            let serialised = serde_json::to_value(variant).unwrap();
            assert_eq!(
                serialised,
                serde_json::Value::String(expected.to_string()),
                "variant {:?} should serialise to {:?}",
                variant,
                expected
            );
            // Round-trip
            let back: ProjectStatus = serde_json::from_value(serialised).unwrap();
            assert_eq!(back, *variant);
        }
    }

    #[test]
    fn test_project_status_as_str() {
        assert_eq!(ProjectStatus::Created.as_str(), "created");
        assert_eq!(ProjectStatus::OntologyGenerated.as_str(), "ontology_generated");
        assert_eq!(ProjectStatus::GraphBuilding.as_str(), "graph_building");
        assert_eq!(ProjectStatus::GraphCompleted.as_str(), "graph_completed");
        assert_eq!(ProjectStatus::Failed.as_str(), "failed");
    }

    // -----------------------------------------------------------------------
    // S-129 create_project
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_project_dir_structure() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("Test Project").unwrap();

        // project_id format: "proj_" + 12 hex chars
        assert!(project.project_id.starts_with("proj_"));
        let suffix = &project.project_id["proj_".len()..];
        assert_eq!(suffix.len(), 12, "suffix must be 12 hex chars");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()), "suffix must be hex");

        // Dir structure: project dir + files/ + project.json exist
        let project_dir = pm.project_dir(&project.project_id);
        assert!(project_dir.exists(), "project dir must exist");
        assert!(pm.project_files_dir(&project.project_id).exists(), "files/ must exist");
        assert!(pm.project_meta_path(&project.project_id).exists(), "project.json must exist");

        // Status = Created
        assert_eq!(project.status, ProjectStatus::Created);

        // Default name when passed
        assert_eq!(project.name, "Test Project");
    }

    #[test]
    fn test_create_project_default_name() {
        let (pm, _dir) = temp_manager();
        // Python default: name="Unnamed Project"
        let project = pm.create_project("Unnamed Project").unwrap();
        assert_eq!(project.name, "Unnamed Project");
    }

    // -----------------------------------------------------------------------
    // S-130/S-131 save/get round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_get_roundtrip() {
        let (pm, _dir) = temp_manager();
        let mut project = pm.create_project("Round Trip").unwrap();

        // Modify and save
        project.status = ProjectStatus::OntologyGenerated;
        project.analysis_summary = Some("Summary text".to_string());
        project.chunk_size = 1000;
        pm.save_project(&mut project).unwrap();

        let loaded = pm.get_project(&project.project_id).unwrap().unwrap();
        assert_eq!(loaded.project_id, project.project_id);
        assert_eq!(loaded.name, "Round Trip");
        assert_eq!(loaded.status, ProjectStatus::OntologyGenerated);
        assert_eq!(loaded.analysis_summary, Some("Summary text".to_string()));
        assert_eq!(loaded.chunk_size, 1000);
    }

    // -----------------------------------------------------------------------
    // S-131 get_project missing → None
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_project_missing_returns_none() {
        let (pm, _dir) = temp_manager();
        let result = pm.get_project("proj_nonexistent").unwrap();
        assert!(result.is_none(), "missing project must return None");
    }

    // -----------------------------------------------------------------------
    // S-131 get_project corrupt JSON → Err
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_project_corrupt_json_returns_err() {
        let (pm, _dir) = temp_manager();
        // Create the dir structure manually to plant a corrupt file
        let project_id = "proj_corrupt123456";
        let project_dir = pm.project_dir(project_id);
        std::fs::create_dir_all(&project_dir).unwrap();
        let meta_path = pm.project_meta_path(project_id);
        std::fs::write(&meta_path, b"NOT VALID JSON {{{").unwrap();

        let result = pm.get_project(project_id);
        assert!(result.is_err(), "corrupt JSON must return Err, not None");
    }

    // -----------------------------------------------------------------------
    // S-121 from_dict defaults
    // -----------------------------------------------------------------------

    #[test]
    fn test_from_dict_with_missing_optional_keys_applies_defaults() {
        // Minimal dict: only project_id required; all else should use Python defaults
        let data = serde_json::json!({
            "project_id": "proj_abc123def456"
        });
        let project = Project::from_dict(&data).unwrap();
        assert_eq!(project.name, "Unnamed Project");
        assert_eq!(project.status, ProjectStatus::Created);
        assert_eq!(project.created_at, "");
        assert_eq!(project.updated_at, "");
        assert!(project.files.is_empty());
        assert_eq!(project.total_text_length, 0);
        assert_eq!(project.chunk_size, 500);
        assert_eq!(project.chunk_overlap, 50);
        assert!(project.ontology.is_none());
        assert!(project.analysis_summary.is_none());
        assert!(project.graph_id.is_none());
        assert!(project.graph_build_task_id.is_none());
        assert!(project.simulation_requirement.is_none());
        assert!(project.error.is_none());
    }

    #[test]
    fn test_from_dict_missing_project_id_returns_err() {
        let data = serde_json::json!({
            "name": "No ID Project"
        });
        let result = Project::from_dict(&data);
        assert!(result.is_err(), "missing project_id must return Err");
    }

    // -----------------------------------------------------------------------
    // S-120 to_dict JSON shape
    // -----------------------------------------------------------------------

    #[test]
    fn test_to_dict_has_all_14_keys() {
        let project = Project {
            project_id: "proj_abc123def456".to_string(),
            name: "中文项目".to_string(),
            status: ProjectStatus::GraphCompleted,
            created_at: "2024-01-01T12:00:00".to_string(),
            updated_at: "2024-01-01T13:00:00".to_string(),
            files: Vec::new(),
            total_text_length: 42,
            ontology: None,
            analysis_summary: None,
            graph_id: Some("g123".to_string()),
            graph_build_task_id: None,
            simulation_requirement: None,
            chunk_size: 500,
            chunk_overlap: 50,
            error: None,
        };

        let dict = project.to_dict();
        let obj = dict.as_object().unwrap();

        // All 14 keys present
        let expected_keys: HashSet<&str> = [
            "project_id",
            "name",
            "status",
            "created_at",
            "updated_at",
            "files",
            "total_text_length",
            "ontology",
            "analysis_summary",
            "graph_id",
            "graph_build_task_id",
            "simulation_requirement",
            "chunk_size",
            "chunk_overlap",
            "error",
        ]
        .iter()
        .copied()
        .collect();
        let actual_keys: HashSet<&str> = obj.keys().map(|s| s.as_str()).collect();
        assert_eq!(actual_keys, expected_keys);

        // status as its string value
        assert_eq!(obj["status"], serde_json::Value::String("graph_completed".to_string()));

        // None fields → JSON null
        assert!(obj["ontology"].is_null());
        assert!(obj["analysis_summary"].is_null());
        assert!(obj["graph_build_task_id"].is_null());
        assert!(obj["simulation_requirement"].is_null());
        assert!(obj["error"].is_null());
    }

    #[test]
    fn test_to_dict_non_ascii_not_escaped() {
        // Non-ASCII (Chinese) must NOT be \uXXXX-escaped in the pretty-printed file
        // serde_json::to_string_pretty matches ensure_ascii=False
        let project = Project {
            project_id: "proj_abc123def456".to_string(),
            name: "中文项目名称".to_string(),
            status: ProjectStatus::Created,
            created_at: "".to_string(),
            updated_at: "".to_string(),
            files: Vec::new(),
            total_text_length: 0,
            ontology: None,
            analysis_summary: None,
            graph_id: None,
            graph_build_task_id: None,
            simulation_requirement: None,
            chunk_size: 500,
            chunk_overlap: 50,
            error: None,
        };

        let json = serde_json::to_string_pretty(&project.to_dict()).unwrap();
        // Raw Chinese chars must appear verbatim, not as \u escape sequences
        assert!(json.contains("中文项目名称"), "Non-ASCII must not be escaped in JSON output");
        assert!(!json.contains("\\u4e2d"), "\\u escapes must NOT appear for non-ASCII");
    }

    // -----------------------------------------------------------------------
    // S-132 list_projects sorting + limit
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_projects_sorts_by_created_at_desc() {
        let (pm, _dir) = temp_manager();

        let mut p1 = pm.create_project("Alpha").unwrap();
        let mut p2 = pm.create_project("Beta").unwrap();
        let mut p3 = pm.create_project("Gamma").unwrap();

        // Overwrite created_at to deterministic values (avoids same-second ties)
        p1.created_at = "2024-01-01T10:00:00".to_string();
        p2.created_at = "2024-01-03T10:00:00".to_string();
        p3.created_at = "2024-01-02T10:00:00".to_string();
        pm.save_project(&mut p1).unwrap();
        pm.save_project(&mut p2).unwrap();
        pm.save_project(&mut p3).unwrap();

        let projects = pm.list_projects(50).unwrap();
        assert_eq!(projects.len(), 3);
        // Sorted descending: p2 (Jan 3), p3 (Jan 2), p1 (Jan 1)
        assert_eq!(projects[0].name, "Beta");
        assert_eq!(projects[1].name, "Gamma");
        assert_eq!(projects[2].name, "Alpha");
    }

    #[test]
    fn test_list_projects_respects_limit() {
        let (pm, _dir) = temp_manager();
        pm.create_project("A").unwrap();
        pm.create_project("B").unwrap();
        pm.create_project("C").unwrap();

        let projects = pm.list_projects(2).unwrap();
        assert_eq!(projects.len(), 2);
    }

    // -----------------------------------------------------------------------
    // S-133 delete_project
    // -----------------------------------------------------------------------

    #[test]
    fn test_delete_project_present_returns_true() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("To Delete").unwrap();
        let result = pm.delete_project(&project.project_id).unwrap();
        assert!(result, "delete of existing project must return true");
        assert!(!pm.project_dir(&project.project_id).exists(), "project dir must be gone");
    }

    #[test]
    fn test_delete_project_absent_returns_false() {
        let (pm, _dir) = temp_manager();
        let result = pm.delete_project("proj_doesnotexist").unwrap();
        assert!(!result, "delete of absent project must return false (not Err)");
    }

    // -----------------------------------------------------------------------
    // S-134 save_file_to_project
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_file_to_project() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("File Test").unwrap();

        let bytes = b"hello world content";
        let file_info = pm.save_file_to_project(&project.project_id, bytes, "Report.PDF").unwrap();

        // saved_filename = {8 hex chars}.pdf
        assert_eq!(file_info.saved_filename.len(), "abcdef12.pdf".len()); // 8+1+3=12? actually 8+4=12
        // Extension lowercased
        assert!(file_info.saved_filename.ends_with(".pdf"), "extension must be lowercased .pdf");
        // 8 hex chars before extension
        let stem = file_info.saved_filename.trim_end_matches(".pdf");
        assert_eq!(stem.len(), 8);
        assert!(stem.chars().all(|c| c.is_ascii_hexdigit()));

        // original_filename preserved
        assert_eq!(file_info.original_filename, "Report.PDF");

        // size matches
        assert_eq!(file_info.size, bytes.len() as i64);

        // path points to existing file
        assert!(std::path::Path::new(&file_info.path).exists());

        // file contents correct
        let on_disk = std::fs::read(&file_info.path).unwrap();
        assert_eq!(on_disk, bytes);
    }

    #[test]
    fn test_save_file_no_extension() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("No Ext").unwrap();
        let file_info = pm.save_file_to_project(&project.project_id, b"data", "README").unwrap();
        // No extension: saved_filename = 8 hex chars (no dot)
        assert_eq!(file_info.saved_filename.len(), 8);
        assert!(file_info.saved_filename.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_get_project_files_lists_files() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("Files List").unwrap();

        pm.save_file_to_project(&project.project_id, b"aaa", "a.txt").unwrap();
        pm.save_file_to_project(&project.project_id, b"bbb", "b.pdf").unwrap();

        let files = pm.get_project_files(&project.project_id).unwrap();
        assert_eq!(files.len(), 2, "should list 2 files");
        // Each path should exist and be a file
        for path in &files {
            assert!(std::path::Path::new(path).is_file());
        }
    }

    #[test]
    fn test_get_project_files_missing_dir_returns_empty() {
        let (pm, _dir) = temp_manager();
        let files = pm.get_project_files("proj_nodirexists").unwrap();
        assert!(files.is_empty());
    }

    // -----------------------------------------------------------------------
    // S-135/S-136 save/get_extracted_text
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_get_extracted_text_roundtrip() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("Text Test").unwrap();

        let text = "提取的文本内容\nSecond line";
        pm.save_extracted_text(&project.project_id, text).unwrap();

        let loaded = pm.get_extracted_text(&project.project_id).unwrap();
        assert_eq!(loaded, Some(text.to_string()));
    }

    #[test]
    fn test_get_extracted_text_missing_returns_none() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("No Text").unwrap();
        // Don't write any text; just check
        let result = pm.get_extracted_text(&project.project_id).unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Regression: FAIL #1 — create_project returns stale updated_at (clone bug)
    // -----------------------------------------------------------------------

    /// create_project must return an object whose updated_at:
    ///   (a) equals the value persisted to project.json on disk (not a pre-save snapshot), AND
    ///   (b) is >= created_at (save_project stamps updated_at AFTER created_at is set).
    ///
    /// The bug: `self.save_project(&mut project.clone())` mutated a temporary; the returned
    /// `project` still had `updated_at == created_at` while the file had the later save-time.
    #[test]
    fn test_create_project_updated_at_matches_persisted_and_gte_created_at() {
        let (pm, _dir) = temp_manager();
        let project = pm.create_project("Regression FAIL1").unwrap();

        // (a) updated_at in returned object must equal the value written to project.json
        let on_disk = pm.get_project(&project.project_id).unwrap().unwrap();
        assert_eq!(
            project.updated_at, on_disk.updated_at,
            "returned updated_at must equal persisted updated_at (not a pre-save snapshot)"
        );

        // (b) updated_at >= created_at (save_project stamps it strictly after or at the same
        // instant, never before)
        assert!(
            project.updated_at >= project.created_at,
            "updated_at ({:?}) must be >= created_at ({:?})",
            project.updated_at,
            project.created_at
        );
    }

    // -----------------------------------------------------------------------
    // Regression: FAIL #2 — from_dict silently drops legacy 3-key files entries
    // -----------------------------------------------------------------------

    /// from_dict with a `files` array containing a 3-key legacy entry
    /// `{"filename":"x","path":"y","size":"3"}` must:
    ///   (a) NOT collapse to [] (len must be 1, not 0), AND
    ///   (b) round-trip the value verbatim through to_dict → from_dict.
    ///
    /// The bug: strict per-element `serde_json::from_value::<Vec<ProjectFile>>(...).ok()` caused
    /// the entire vector to collapse to [] when ANY entry didn't match the 4-key ProjectFile shape.
    #[test]
    fn test_from_dict_legacy_3key_files_entry_preserved_verbatim() {
        let legacy_entry = serde_json::json!({
            "filename": "old_report.pdf",
            "path": "/uploads/old_report.pdf",
            "size": "3072"
        });

        let data = serde_json::json!({
            "project_id": "proj_legacytest123",
            "files": [legacy_entry.clone()]
        });

        let project = Project::from_dict(&data).unwrap();

        // (a) must NOT be dropped to [] — len must be 1
        assert_eq!(
            project.files.len(),
            1,
            "legacy 3-key files entry must NOT be silently dropped (got len={})",
            project.files.len()
        );

        // (b) value preserved verbatim
        assert_eq!(
            project.files[0], legacy_entry,
            "legacy files entry must be preserved verbatim through from_dict"
        );

        // (c) survives a to_dict → from_dict round-trip
        let dict = project.to_dict();
        let project2 = Project::from_dict(&dict).unwrap();
        assert_eq!(project2.files.len(), 1, "round-trip must preserve files len");
        assert_eq!(
            project2.files[0], legacy_entry,
            "round-trip must preserve files entry verbatim"
        );
    }
}
