//! In-memory social-world substrate (Workstream C — OASIS post-graph + SQLite materialization +
//! feed-back).
//!
//! The native `SimEngine` runs social agents that can emit the full `SocialAction` taxonomy
//! (`crate::sim::SocialAction`), but before this module there was no social *world* for those
//! actions to mutate: `LIKE_POST`/`CREATE_COMMENT`/`REPOST`/`FOLLOW` had nothing real to target,
//! and the `{platform}_simulation.db` the `/posts` + `/comments` readers consume
//! (`src/api/simulation.rs`) was never written.
//!
//! This module adds three opt-in, no-downgrade pieces:
//! 1. [`SocialWorld`] — a pure in-memory post/comment/like/follow graph with monotonic id minting
//!    and a fail-closed [`SocialWorld::apply`] that mutates per the 12-variant `SocialAction`
//!    taxonomy. An unknown/unparseable target id resolves to [`ApplyOutcome::NoOp`] — never a
//!    panic, never an invented post.
//! 2. [`FeedSnapshot`] — a read-only, recency-ranked slice of the world fed back into the NEXT
//!    round's agent prompts so agents target REAL post ids.
//! 3. [`SocialDbWriter`] (behind the `sqlite` feature) — materializes the world to
//!    `{sim_dir}/{platform}_simulation.db` with `post`/`comment` tables that are a SUPERSET of the
//!    reader fixtures, so the existing `SELECT * FROM post ORDER BY created_at DESC` /
//!    `SELECT * FROM comment WHERE post_id = ?` queries consume it unchanged.
//!
//! The in-memory core (`SocialWorld`, `apply`, the feed) compiles and runs WITHOUT the `sqlite`
//! feature; only [`SocialDbWriter`] and the per-round flush are `#[cfg(feature = "sqlite")]`.

use crate::agent::Platform;
use crate::sim::{SocialAction, TargetKind};
use std::collections::HashMap;

/// One materialized post in the social world. `id` is the OASIS-ish post id surfaced to the
/// reader (`post.post_id`) AND to the agent feed (so `LIKE_POST(target_id=...)` can target it).
#[derive(Debug, Clone)]
pub struct Post {
    pub id: i64,
    /// `SocialProfile.user_id` of the poster.
    pub author_user_id: i64,
    pub content: String,
    /// `python_isoformat_local()` — the same TEXT-ISO format `actions.jsonl` uses, so the DB's
    /// `ORDER BY created_at DESC` sorts lexicographically as intended.
    pub created_at: String,
    pub num_likes: i64,
    pub num_dislikes: i64,
    /// REPOST + QUOTE_POST increment this.
    pub num_shares: i64,
}

/// One materialized comment in the social world.
#[derive(Debug, Clone)]
pub struct Comment {
    pub id: i64,
    pub post_id: i64,
    pub author_user_id: i64,
    pub content: String,
    pub created_at: String,
    pub num_likes: i64,
    pub num_dislikes: i64,
}

/// A directed follow edge (`follower` user id -> `followee` identifier, kept as the raw string the
/// agent emitted because a followee may be a handle, not a numeric user id).
#[derive(Debug, Clone)]
pub struct FollowEdge {
    pub follower: i64,
    pub followee: String,
}

/// Outcome of [`SocialWorld::apply`]. Carries the minted id when the action created a post/comment
/// so the caller can correlate it with the `actions.jsonl` record if desired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    CreatedPost(i64),
    CreatedComment(i64),
    /// like/dislike/repost/follow updated counts/edges.
    Mutated,
    /// search/trend/donothing/mute, or an unresolved/unparseable target (fail-closed).
    NoOp,
}

/// In-memory social world for ONE platform. A parallel (twitter + reddit) run keeps two of these
/// (held in a [`SocialWorldSet`]) so a reddit LIKE never mutates a twitter post and the two DB
/// files stay disjoint — mirroring the existing per-platform `PlatformLoggerSet` split.
#[derive(Debug, Default)]
pub struct SocialWorld {
    pub platform: Option<Platform>,
    posts: Vec<Post>,
    comments: Vec<Comment>,
    follows: Vec<FollowEdge>,
    /// `post_id` -> index into `posts` (O(1) like/repost resolution).
    post_index: HashMap<i64, usize>,
    /// `comment_id` -> index into `comments`.
    comment_index: HashMap<i64, usize>,
    /// `user_id` -> display name. Populated as agents act (poster/commenter/actor names are known
    /// at the call site). Used to resolve `author_name` / `target_user_name` enrichment for the
    /// `actions.jsonl` records (`run_parallel_simulation.py:_get_post_info`/`_enrich_action_context`
    /// look the same names up out of the OASIS `user` table). Missing ⇒ empty string, exactly like
    /// MiroFish's `author_name = ''` fallback.
    users: HashMap<i64, String>,
    next_post_id: i64,
    next_comment_id: i64,
}

/// Parse an LLM-emitted target id into a post/comment id. The taxonomy carries these as `String`
/// (the model emits e.g. `post-12`, `comment-9`, or a bare `12`). Strips a leading `post-` /
/// `comment-` prefix then parses the remainder as an `i64`. Returns `None` on anything
/// unparseable — the fail-closed signal that an `apply` becomes a [`ApplyOutcome::NoOp`] (never a
/// panic, never an invented post).
pub(crate) fn parse_target_id(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("post-")
        .or_else(|| trimmed.strip_prefix("comment-"))
        .unwrap_or(trimmed);
    stripped.parse::<i64>().ok()
}

impl SocialWorld {
    /// A fresh, empty world for `platform`.
    pub fn new(platform: Platform) -> Self {
        Self {
            platform: Some(platform),
            next_post_id: 1,
            next_comment_id: 1,
            ..Default::default()
        }
    }

    /// Read-only view of the posts (newest insertion last). Primarily for tests + the feed.
    pub fn posts(&self) -> &[Post] {
        &self.posts
    }

    /// Read-only view of the comments.
    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    /// Read-only view of the follow edges.
    pub fn follows(&self) -> &[FollowEdge] {
        &self.follows
    }

    /// Record a `user_id` -> display-name mapping for enrichment lookups. Idempotent; a later call
    /// overwrites (names are stable in practice). No-op for an empty name so we never shadow a real
    /// name with `""`.
    pub fn register_user(&mut self, user_id: i64, name: &str) {
        if !name.is_empty() {
            self.users.insert(user_id, name.to_string());
        }
    }

    /// Resolve a `user_id` to its registered display name, or `None` if unknown.
    pub fn user_name(&self, user_id: i64) -> Option<&str> {
        self.users.get(&user_id).map(String::as_str)
    }

    /// Read-only post lookup by id (for enrichment).
    pub fn post_by_id(&self, id: i64) -> Option<&Post> {
        self.post_index.get(&id).and_then(|&idx| self.posts.get(idx))
    }

    /// Read-only comment lookup by id (for enrichment).
    pub fn comment_by_id(&self, id: i64) -> Option<&Comment> {
        self.comment_index.get(&id).and_then(|&idx| self.comments.get(idx))
    }

    /// Mint and insert a new post; returns its id. Used by both `CreatePost` and the round-0
    /// initial-post seed.
    pub fn create_post(&mut self, author_user_id: i64, content: &str, created_at: &str) -> i64 {
        let id = self.next_post_id;
        self.next_post_id += 1;
        let idx = self.posts.len();
        self.posts.push(Post {
            id,
            author_user_id,
            content: content.to_string(),
            created_at: created_at.to_string(),
            num_likes: 0,
            num_dislikes: 0,
            num_shares: 0,
        });
        self.post_index.insert(id, idx);
        id
    }

    /// Mint and insert a new comment under `post_id`; returns its id.
    fn create_comment(
        &mut self,
        post_id: i64,
        author_user_id: i64,
        content: &str,
        created_at: &str,
    ) -> i64 {
        let id = self.next_comment_id;
        self.next_comment_id += 1;
        let idx = self.comments.len();
        self.comments.push(Comment {
            id,
            post_id,
            author_user_id,
            content: content.to_string(),
            created_at: created_at.to_string(),
            num_likes: 0,
            num_dislikes: 0,
        });
        self.comment_index.insert(id, idx);
        id
    }

    /// Mutate the world for one committed social action by `author_user_id` at `created_at`.
    ///
    /// Fail-closed: any action whose target id does not resolve to an existing post/comment is a
    /// [`ApplyOutcome::NoOp`] — the world is never corrupted and an action never invents a post.
    pub fn apply(
        &mut self,
        author_user_id: i64,
        sa: &SocialAction,
        created_at: &str,
    ) -> ApplyOutcome {
        match sa {
            SocialAction::CreatePost { content } => {
                ApplyOutcome::CreatedPost(self.create_post(author_user_id, content, created_at))
            }
            SocialAction::Comment { post_id, content } => {
                match parse_target_id(post_id).and_then(|id| self.post_index.get(&id).map(|_| id)) {
                    Some(id) => ApplyOutcome::CreatedComment(self.create_comment(
                        id,
                        author_user_id,
                        content,
                        created_at,
                    )),
                    None => ApplyOutcome::NoOp,
                }
            }
            SocialAction::Like { target_kind, target_id } => self.adjust_count(
                target_kind,
                target_id,
                |p| p.num_likes += 1,
                |c| c.num_likes += 1,
            ),
            SocialAction::Dislike { target_kind, target_id } => self.adjust_count(
                target_kind,
                target_id,
                |p| p.num_dislikes += 1,
                |c| c.num_dislikes += 1,
            ),
            SocialAction::Repost { post_id } => match self.resolve_post_mut(post_id) {
                Some(post) => {
                    post.num_shares += 1;
                    ApplyOutcome::Mutated
                }
                None => ApplyOutcome::NoOp,
            },
            SocialAction::Quote { post_id, content } => {
                // QUOTE_POST = a reshare (num_shares++) PLUS a derived comment carrying the quote
                // text, so the quote content is materialized + readable in the `comment` table.
                match parse_target_id(post_id).and_then(|id| self.post_index.get(&id).map(|_| id)) {
                    Some(id) => {
                        if let Some(idx) = self.post_index.get(&id).copied() {
                            self.posts[idx].num_shares += 1;
                        }
                        ApplyOutcome::CreatedComment(self.create_comment(
                            id,
                            author_user_id,
                            content,
                            created_at,
                        ))
                    }
                    None => ApplyOutcome::NoOp,
                }
            }
            SocialAction::Follow { user_id } => {
                self.follows
                    .push(FollowEdge { follower: author_user_id, followee: user_id.clone() });
                ApplyOutcome::Mutated
            }
            // No DB-visible state: these never appear in `post`/`comment`.
            SocialAction::Mute { .. }
            | SocialAction::SearchPosts { .. }
            | SocialAction::SearchUser { .. }
            | SocialAction::Trend
            | SocialAction::DoNothing => ApplyOutcome::NoOp,
        }
    }

    /// Resolve a post id string to a mutable post reference, or `None` (fail-closed).
    fn resolve_post_mut(&mut self, raw: &str) -> Option<&mut Post> {
        let id = parse_target_id(raw)?;
        let idx = *self.post_index.get(&id)?;
        self.posts.get_mut(idx)
    }

    /// Apply a like/dislike adjustment to the post or comment named by `target_kind`/`target_id`.
    fn adjust_count(
        &mut self,
        kind: &TargetKind,
        raw: &str,
        on_post: impl FnOnce(&mut Post),
        on_comment: impl FnOnce(&mut Comment),
    ) -> ApplyOutcome {
        let id = match parse_target_id(raw) {
            Some(id) => id,
            None => return ApplyOutcome::NoOp,
        };
        match kind {
            TargetKind::Post => match self.post_index.get(&id).copied() {
                Some(idx) => {
                    on_post(&mut self.posts[idx]);
                    ApplyOutcome::Mutated
                }
                None => ApplyOutcome::NoOp,
            },
            TargetKind::Comment => match self.comment_index.get(&id).copied() {
                Some(idx) => {
                    on_comment(&mut self.comments[idx]);
                    ApplyOutcome::Mutated
                }
                None => ApplyOutcome::NoOp,
            },
        }
    }

    /// Top-`top_n` posts for the feed, recency-ranked per `params`. Snapshot once per tick, before
    /// the concurrent prepare phase, so it reflects state through the previous tick.
    pub fn feed_snapshot(&self, top_n: usize, params: &FeedRankParams) -> FeedSnapshot {
        if self.posts.is_empty() || top_n == 0 {
            return FeedSnapshot::default();
        }
        // recency_norm: rank by insertion order (post.id is monotonic ⇒ a proxy for recency),
        // normalized to [0,1] where the newest post is 1.0.
        let n = self.posts.len() as f64;
        let max_engagement = self
            .posts
            .iter()
            .map(|p| (p.num_likes + p.num_shares).max(0) as f64)
            .fold(0.0_f64, f64::max)
            .max(1.0);

        let mut ranked: Vec<(f64, &Post)> = self
            .posts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let recency_norm = if n > 1.0 { i as f64 / (n - 1.0) } else { 1.0 };
                let engagement_norm = (p.num_likes + p.num_shares).max(0) as f64 / max_engagement;
                let topic_affinity = params.topic_affinity(p.author_user_id);
                let score = params.recency_weight * recency_norm
                    + params.echo_chamber_strength * topic_affinity
                    + params.influence_weight * engagement_norm;
                (score, p)
            })
            .collect();

        // Highest score first; ties broken by newest-first (higher index) so the default
        // recency-only ranking yields strictly newest-first ordering.
        ranked.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.1.id.cmp(&a.1.id))
        });
        ranked.truncate(top_n);

        FeedSnapshot {
            posts: ranked
                .into_iter()
                .map(|(_, p)| FeedPost {
                    id: p.id,
                    author_user_id: p.author_user_id,
                    content: p.content.clone(),
                    num_likes: p.num_likes,
                    num_shares: p.num_shares,
                })
                .collect(),
        }
    }
}

/// A read-only, recency-ranked slice of the world for injection into the NEXT round's prompts.
#[derive(Debug, Clone, Default)]
pub struct FeedSnapshot {
    /// Already ranked + truncated to top-N (newest/highest-scoring first).
    pub posts: Vec<FeedPost>,
}

impl FeedSnapshot {
    pub fn is_empty(&self) -> bool {
        self.posts.is_empty()
    }
}

/// One post in a [`FeedSnapshot`], carrying the exact id the agent should target.
#[derive(Debug, Clone)]
pub struct FeedPost {
    pub id: i64,
    pub author_user_id: i64,
    pub content: String,
    pub num_likes: i64,
    pub num_shares: i64,
}

/// Feed ranking knobs (read from `simulation_config.json`; default recency-only so the MVP is
/// "recent-ordered top-N"). `echo_chamber_strength` / `influence_weight` are wired but optional.
#[derive(Debug, Clone)]
pub struct FeedRankParams {
    pub recency_weight: f64,
    pub echo_chamber_strength: f64,
    pub influence_weight: f64,
    pub top_n: usize,
    /// Viewer's interested-topic affinity set, keyed by author `user_id`. An author whose post
    /// shares ≥1 interested topic with the viewer scores a `1.0` echo-chamber affinity. Empty ⇒
    /// every affinity is `0.0` (recency/engagement only).
    affinity_authors: std::collections::HashSet<i64>,
}

impl Default for FeedRankParams {
    fn default() -> Self {
        // Recency-only: the MVP ordering the workstream allows.
        Self {
            recency_weight: 1.0,
            echo_chamber_strength: 0.0,
            influence_weight: 0.0,
            top_n: 10,
            affinity_authors: std::collections::HashSet::new(),
        }
    }
}

impl FeedRankParams {
    /// Read ranking knobs from the prepared `simulation_config.json` value. Missing keys fall back
    /// to the recency-only default. `affinity_authors` is supplied by the caller (it depends on the
    /// viewer agent, not the config), via [`FeedRankParams::with_affinity_authors`].
    pub fn from_config(config: &serde_json::Value) -> Self {
        let feed = config.get("feed_config");
        let get_f64 = |key: &str, default: f64| -> f64 {
            feed.and_then(|f| f.get(key))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(default)
        };
        let top_n = feed
            .and_then(|f| f.get("top_n"))
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(10);
        Self {
            recency_weight: get_f64("recency_weight", 1.0),
            echo_chamber_strength: get_f64("echo_chamber_strength", 0.0),
            influence_weight: get_f64("influence_weight", 0.0),
            top_n,
            affinity_authors: std::collections::HashSet::new(),
        }
    }

    /// Set the viewer's affinity author-id set (authors sharing ≥1 interested topic with the
    /// viewer). Used to compute the echo-chamber term per-viewer.
    pub fn with_affinity_authors(mut self, authors: std::collections::HashSet<i64>) -> Self {
        self.affinity_authors = authors;
        self
    }

    fn topic_affinity(&self, author_user_id: i64) -> f64 {
        if self.affinity_authors.contains(&author_user_id) { 1.0 } else { 0.0 }
    }
}

// ---------------------------------------------------------------------------
// Per-platform holder + SQLite materialization
// ---------------------------------------------------------------------------

/// A per-platform set of [`SocialWorld`]s (≤2: twitter and/or reddit), mirroring the
/// `PlatformLoggerSet` split. Owns the optional [`SocialDbWriter`]s so the whole social substrate
/// travels by value with the engine into the run body (no shared mutability).
pub struct SocialWorldSet {
    worlds: Vec<(Platform, SocialWorld)>,
    #[cfg(feature = "sqlite")]
    writers: Vec<(Platform, SocialDbWriter)>,
}

impl SocialWorldSet {
    /// Build a set with one [`SocialWorld`] per platform in `platforms`, materializing under
    /// `sim_dir`. When the `sqlite` feature is on, opens (and creates the tables for) a
    /// `{platform}_simulation.db` writer for each platform.
    pub fn new(
        platforms: impl IntoIterator<Item = Platform>,
        sim_dir: impl Into<std::path::PathBuf>,
    ) -> crate::error::Result<Self> {
        let sim_dir = sim_dir.into();
        let platforms: Vec<Platform> = platforms.into_iter().collect();
        let worlds = platforms.iter().map(|&p| (p, SocialWorld::new(p))).collect();
        #[cfg(feature = "sqlite")]
        let writers = {
            let mut ws = Vec::with_capacity(platforms.len());
            for &p in &platforms {
                ws.push((p, SocialDbWriter::open(p, &sim_dir)?));
            }
            ws
        };
        // `sim_dir` is consumed by the per-platform writers above (sqlite build); without the
        // feature it is unused. Discard it explicitly so neither build warns.
        let _ = &sim_dir;
        Ok(Self {
            worlds,
            #[cfg(feature = "sqlite")]
            writers,
        })
    }

    /// Mutable access to a platform's world (for `apply` / seed), or `None` if not installed.
    pub fn world_mut(&mut self, platform: Platform) -> Option<&mut SocialWorld> {
        self.worlds.iter_mut().find(|(p, _)| *p == platform).map(|(_, w)| w)
    }

    /// Immutable access to a platform's world (for snapshotting the feed).
    pub fn world(&self, platform: Platform) -> Option<&SocialWorld> {
        self.worlds.iter().find(|(p, _)| *p == platform).map(|(_, w)| w)
    }

    /// The platforms present in this set.
    pub fn platforms(&self) -> impl Iterator<Item = Platform> + '_ {
        self.worlds.iter().map(|(p, _)| *p)
    }

    /// Flush every platform's world to its DB at the end of a round. No-op without `sqlite`.
    pub fn flush_round(&mut self) -> crate::error::Result<()> {
        #[cfg(feature = "sqlite")]
        {
            for (platform, writer) in &mut self.writers {
                if let Some((_, world)) = self.worlds.iter().find(|(p, _)| p == platform) {
                    writer.flush_round(world)?;
                }
            }
        }
        Ok(())
    }

    /// Final flush at sim end (then the connections drop). No-op without `sqlite`.
    pub fn flush_final(&mut self) -> crate::error::Result<()> {
        self.flush_round()
    }
}

/// Map a `rusqlite` error into a `TeriError::Sim`.
#[cfg(feature = "sqlite")]
fn db_err(context: &str, e: rusqlite::Error) -> crate::error::TeriError {
    crate::error::TeriError::Sim(format!("social DB {context} failed: {e}"))
}

/// SQLite writer for one platform's `{sim_dir}/{platform}_simulation.db`. Holds an owned
/// connection; flushes the full (small) in-memory world per round via `INSERT OR REPLACE`, which is
/// idempotent because the ids are stable + monotonic.
#[cfg(feature = "sqlite")]
pub struct SocialDbWriter {
    conn: rusqlite::Connection,
}

#[cfg(feature = "sqlite")]
impl SocialDbWriter {
    /// `post` / `comment` DDL — a SUPERSET of the reader fixtures
    /// (`post(post_id, content, created_at)`, `comment(comment_id, post_id, content, created_at)`),
    /// so `SELECT * FROM post ORDER BY created_at DESC` and `SELECT * FROM comment WHERE post_id = ?`
    /// consume it unchanged while the extra OASIS columns ride along harmlessly.
    const DDL: &'static str = "\
        CREATE TABLE IF NOT EXISTS post (\
            post_id      INTEGER PRIMARY KEY,\
            user_id      INTEGER,\
            content      TEXT,\
            created_at   TEXT,\
            num_likes    INTEGER DEFAULT 0,\
            num_dislikes INTEGER DEFAULT 0,\
            num_shares   INTEGER DEFAULT 0\
        );\
        CREATE TABLE IF NOT EXISTS comment (\
            comment_id   INTEGER PRIMARY KEY,\
            post_id      INTEGER,\
            user_id      INTEGER,\
            content      TEXT,\
            created_at   TEXT,\
            num_likes    INTEGER DEFAULT 0,\
            num_dislikes INTEGER DEFAULT 0\
        );";

    /// Open (creating the file + tables) the `{platform}_simulation.db` writer under `sim_dir`.
    pub fn open(platform: Platform, sim_dir: &std::path::Path) -> crate::error::Result<Self> {
        let file = format!("{}_simulation.db", platform_db_name(platform));
        let conn = rusqlite::Connection::open(sim_dir.join(file)).map_err(|e| db_err("open", e))?;
        conn.execute_batch(Self::DDL).map_err(|e| db_err("create tables", e))?;
        Ok(Self { conn })
    }

    /// Flush the full in-memory world in ONE transaction (idempotent `INSERT OR REPLACE`).
    pub fn flush_round(&mut self, world: &SocialWorld) -> crate::error::Result<()> {
        let tx = self.conn.transaction().map_err(|e| db_err("begin", e))?;
        {
            let mut post_stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO post \
                     (post_id, user_id, content, created_at, num_likes, num_dislikes, num_shares) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| db_err("prepare post", e))?;
            for p in world.posts() {
                post_stmt
                    .execute(rusqlite::params![
                        p.id,
                        p.author_user_id,
                        p.content,
                        p.created_at,
                        p.num_likes,
                        p.num_dislikes,
                        p.num_shares,
                    ])
                    .map_err(|e| db_err("insert post", e))?;
            }
            let mut comment_stmt = tx
                .prepare_cached(
                    "INSERT OR REPLACE INTO comment \
                     (comment_id, post_id, user_id, content, created_at, num_likes, num_dislikes) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| db_err("prepare comment", e))?;
            for c in world.comments() {
                comment_stmt
                    .execute(rusqlite::params![
                        c.id,
                        c.post_id,
                        c.author_user_id,
                        c.content,
                        c.created_at,
                        c.num_likes,
                        c.num_dislikes,
                    ])
                    .map_err(|e| db_err("insert comment", e))?;
            }
        }
        tx.commit().map_err(|e| db_err("commit", e))?;
        Ok(())
    }
}

/// The filename stem for a platform's DB (`twitter` / `reddit`), matching the reader's
/// `{platform}_simulation.db` lookup and the `clear_simulation_files` cleanup list.
#[cfg(feature = "sqlite")]
fn platform_db_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Twitter => "twitter",
        Platform::Reddit => "reddit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "2025-12-01T10:00:00";

    fn world() -> SocialWorld {
        SocialWorld::new(Platform::Reddit)
    }

    #[test]
    fn create_post_mints_monotonic_ids() {
        let mut w = world();
        let a = w.apply(7, &SocialAction::CreatePost { content: "first".into() }, TS);
        let b = w.apply(7, &SocialAction::CreatePost { content: "second".into() }, TS);
        assert_eq!(a, ApplyOutcome::CreatedPost(1));
        assert_eq!(b, ApplyOutcome::CreatedPost(2));
        assert_eq!(w.posts().len(), 2);
        assert_eq!(w.posts()[0].content, "first");
    }

    #[test]
    fn comment_on_known_post_creates_comment() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        let out = w.apply(
            8,
            &SocialAction::Comment { post_id: "post-1".into(), content: "nice".into() },
            TS,
        );
        assert_eq!(out, ApplyOutcome::CreatedComment(1));
        assert_eq!(w.comments().len(), 1);
        assert_eq!(w.comments()[0].post_id, 1);
        assert_eq!(w.comments()[0].content, "nice");
    }

    #[test]
    fn comment_on_unknown_post_is_noop_fail_closed() {
        let mut w = world();
        // No posts exist yet — a hallucinated id must not panic or invent a post.
        let out = w.apply(
            8,
            &SocialAction::Comment { post_id: "post-999".into(), content: "x".into() },
            TS,
        );
        assert_eq!(out, ApplyOutcome::NoOp);
        assert!(w.comments().is_empty());
        assert!(w.posts().is_empty());
    }

    #[test]
    fn comment_on_unparseable_id_is_noop() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        let out = w.apply(
            8,
            &SocialAction::Comment { post_id: "not-a-number".into(), content: "x".into() },
            TS,
        );
        assert_eq!(out, ApplyOutcome::NoOp);
        assert!(w.comments().is_empty());
    }

    #[test]
    fn like_post_increments_likes_bare_id_and_prefixed() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        let out = w.apply(
            8,
            &SocialAction::Like { target_kind: TargetKind::Post, target_id: "post-1".into() },
            TS,
        );
        assert_eq!(out, ApplyOutcome::Mutated);
        // Bare id (no prefix) must also resolve.
        w.apply(
            9,
            &SocialAction::Like { target_kind: TargetKind::Post, target_id: "1".into() },
            TS,
        );
        assert_eq!(w.posts()[0].num_likes, 2);
    }

    #[test]
    fn like_unknown_post_is_noop() {
        let mut w = world();
        let out = w.apply(
            8,
            &SocialAction::Like { target_kind: TargetKind::Post, target_id: "5".into() },
            TS,
        );
        assert_eq!(out, ApplyOutcome::NoOp);
    }

    #[test]
    fn like_comment_increments_comment_likes() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        w.apply(8, &SocialAction::Comment { post_id: "1".into(), content: "c".into() }, TS);
        let out = w.apply(
            9,
            &SocialAction::Like { target_kind: TargetKind::Comment, target_id: "1".into() },
            TS,
        );
        assert_eq!(out, ApplyOutcome::Mutated);
        assert_eq!(w.comments()[0].num_likes, 1);
    }

    #[test]
    fn dislike_post_increments_dislikes() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        w.apply(
            8,
            &SocialAction::Dislike { target_kind: TargetKind::Post, target_id: "1".into() },
            TS,
        );
        assert_eq!(w.posts()[0].num_dislikes, 1);
    }

    #[test]
    fn repost_increments_shares() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        let out = w.apply(8, &SocialAction::Repost { post_id: "1".into() }, TS);
        assert_eq!(out, ApplyOutcome::Mutated);
        assert_eq!(w.posts()[0].num_shares, 1);
    }

    #[test]
    fn repost_unknown_post_is_noop() {
        let mut w = world();
        let out = w.apply(8, &SocialAction::Repost { post_id: "1".into() }, TS);
        assert_eq!(out, ApplyOutcome::NoOp);
    }

    #[test]
    fn quote_increments_shares_and_mints_comment() {
        let mut w = world();
        w.apply(7, &SocialAction::CreatePost { content: "hi".into() }, TS);
        let out =
            w.apply(8, &SocialAction::Quote { post_id: "1".into(), content: "agree".into() }, TS);
        assert_eq!(out, ApplyOutcome::CreatedComment(1));
        assert_eq!(w.posts()[0].num_shares, 1);
        assert_eq!(w.comments()[0].content, "agree");
        assert_eq!(w.comments()[0].post_id, 1);
    }

    #[test]
    fn quote_unknown_post_is_noop() {
        let mut w = world();
        let out = w.apply(8, &SocialAction::Quote { post_id: "9".into(), content: "x".into() }, TS);
        assert_eq!(out, ApplyOutcome::NoOp);
        assert!(w.comments().is_empty());
    }

    #[test]
    fn follow_adds_edge() {
        let mut w = world();
        let out = w.apply(7, &SocialAction::Follow { user_id: "alice".into() }, TS);
        assert_eq!(out, ApplyOutcome::Mutated);
        assert_eq!(w.follows().len(), 1);
        assert_eq!(w.follows()[0].follower, 7);
        assert_eq!(w.follows()[0].followee, "alice");
    }

    #[test]
    fn non_materializing_actions_are_noop() {
        let mut w = world();
        for sa in [
            SocialAction::Mute { user_id: "x".into() },
            SocialAction::SearchPosts { query: "q".into() },
            SocialAction::SearchUser { query: "q".into() },
            SocialAction::Trend,
            SocialAction::DoNothing,
        ] {
            assert_eq!(w.apply(7, &sa, TS), ApplyOutcome::NoOp);
        }
        assert!(w.posts().is_empty());
        assert!(w.comments().is_empty());
        assert!(w.follows().is_empty());
    }

    #[test]
    fn feed_snapshot_recency_orders_newest_first() {
        let mut w = world();
        w.create_post(1, "oldest", TS);
        w.create_post(2, "middle", TS);
        w.create_post(3, "newest", TS);
        let feed = w.feed_snapshot(10, &FeedRankParams::default());
        assert_eq!(feed.posts.len(), 3);
        assert_eq!(feed.posts[0].content, "newest");
        assert_eq!(feed.posts[2].content, "oldest");
    }

    #[test]
    fn feed_snapshot_truncates_to_top_n() {
        let mut w = world();
        for i in 0..5 {
            w.create_post(1, &format!("p{i}"), TS);
        }
        let feed = w.feed_snapshot(2, &FeedRankParams::default());
        assert_eq!(feed.posts.len(), 2);
        // Newest first.
        assert_eq!(feed.posts[0].content, "p4");
        assert_eq!(feed.posts[1].content, "p3");
    }

    #[test]
    fn feed_snapshot_empty_world_is_empty() {
        let w = world();
        assert!(w.feed_snapshot(10, &FeedRankParams::default()).is_empty());
    }

    #[test]
    fn feed_snapshot_echo_chamber_boosts_affinity_author() {
        let mut w = world();
        // Oldest post by an affinity author; newest by a non-affinity author.
        w.create_post(42, "from-affinity-author", TS);
        w.create_post(99, "from-stranger", TS);
        let mut authors = std::collections::HashSet::new();
        authors.insert(42_i64);
        let params = FeedRankParams {
            recency_weight: 0.1,
            echo_chamber_strength: 1.0,
            influence_weight: 0.0,
            top_n: 10,
            ..FeedRankParams::default()
        }
        .with_affinity_authors(authors);
        let feed = w.feed_snapshot(10, &params);
        // Echo chamber dominates recency ⇒ affinity author's (older) post ranks first.
        assert_eq!(feed.posts[0].content, "from-affinity-author");
    }

    #[test]
    fn feed_rank_params_from_config_defaults_to_recency() {
        let cfg = serde_json::json!({});
        let p = FeedRankParams::from_config(&cfg);
        assert_eq!(p.recency_weight, 1.0);
        assert_eq!(p.echo_chamber_strength, 0.0);
        assert_eq!(p.top_n, 10);
    }

    #[test]
    fn feed_rank_params_from_config_reads_knobs() {
        let cfg = serde_json::json!({
            "feed_config": { "recency_weight": 0.5, "echo_chamber_strength": 0.3, "top_n": 4 }
        });
        let p = FeedRankParams::from_config(&cfg);
        assert_eq!(p.recency_weight, 0.5);
        assert_eq!(p.echo_chamber_strength, 0.3);
        assert_eq!(p.top_n, 4);
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_writer_round_trip_matches_reader_queries() {
        let tmp = std::env::temp_dir().join(format!("teri_sw_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();

        let mut set = SocialWorldSet::new([Platform::Reddit], &tmp).unwrap();
        {
            let w = set.world_mut(Platform::Reddit).unwrap();
            let p1 = w.create_post(7, "hello", "2025-12-01T10:00:00");
            w.create_post(7, "world", "2025-12-01T10:01:00");
            w.apply(
                8,
                &SocialAction::Like { target_kind: TargetKind::Post, target_id: p1.to_string() },
                "2025-12-01T10:02:00",
            );
            w.apply(
                8,
                &SocialAction::Comment { post_id: p1.to_string(), content: "a comment".into() },
                "2025-12-01T10:03:00",
            );
        }
        set.flush_round().unwrap();

        // Open the produced DB with the EXACT reader queries.
        let conn = rusqlite::Connection::open(tmp.join("reddit_simulation.db")).unwrap();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM post", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 2);

        // Scope each prepared statement so its borrow of `conn` releases before `drop(conn)`.
        {
            let mut stmt = conn
                .prepare("SELECT * FROM post ORDER BY created_at DESC LIMIT ? OFFSET ?")
                .unwrap();
            let cols: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
            // Reader fixtures expect at least these columns.
            for required in ["post_id", "content", "created_at"] {
                assert!(cols.iter().any(|c| c == required), "missing column {required}");
            }
            let rows: Vec<(i64, String)> = stmt
                .query_map(rusqlite::params![50_i64, 0_i64], |r| {
                    Ok((r.get::<_, i64>("post_id")?, r.get::<_, String>("content")?))
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            // ORDER BY created_at DESC ⇒ "world" (10:01) before "hello" (10:00).
            assert_eq!(rows[0].1, "world");
            assert_eq!(rows[1].1, "hello");
        }

        // The like landed on post 1.
        let likes: i64 = conn
            .query_row("SELECT num_likes FROM post WHERE post_id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(likes, 1);

        // The comment is consumable by the comments reader query.
        {
            let mut cstmt = conn
                .prepare(
                    "SELECT * FROM comment WHERE post_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?",
                )
                .unwrap();
            let ccols: Vec<String> = cstmt.column_names().iter().map(|s| s.to_string()).collect();
            for required in ["comment_id", "post_id", "content", "created_at"] {
                assert!(ccols.iter().any(|c| c == required), "missing comment column {required}");
            }
            let comment_count: i64 = cstmt
                .query_map(rusqlite::params![1_i64, 50_i64, 0_i64], |_| Ok(()))
                .unwrap()
                .count() as i64;
            assert_eq!(comment_count, 1);
        }

        // Re-flush is idempotent (INSERT OR REPLACE keyed on stable ids).
        set.flush_round().unwrap();
        let total2: i64 = conn.query_row("SELECT COUNT(*) FROM post", [], |r| r.get(0)).unwrap();
        assert_eq!(total2, 2);

        drop(conn);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
