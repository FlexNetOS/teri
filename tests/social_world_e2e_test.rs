//! Workstream C end-to-end: run the native `SimEngine` with the social-world substrate installed
//! and a deterministic mock LLM, then assert the materialized `{platform}_simulation.db` is
//! consumable by the EXACT reader queries (`SELECT * FROM post ORDER BY created_at DESC` /
//! `SELECT * FROM comment WHERE post_id = ?`) AND that round-2 engagement (LIKE/COMMENT against a
//! round-1 post id surfaced via the feed) is reflected — proving feed-back → apply → flush.
//!
//! `#[cfg(feature = "sqlite")]`: the DB is only written under the `sqlite` feature.

#![cfg(feature = "sqlite")]

use async_trait::async_trait;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use teri::agent::{Agent, AgentPool, Persona, Platform, SocialProfile};
use teri::graph::KnowledgeGraph;
use teri::llm::LlmClient;
use teri::sim::social_world::SocialWorldSet;
use teri::sim::{SimConfig, SimEngine};

/// Deterministic LLM: a social agent CREATE_POSTs when its prompt has no feed (round 1), then
/// LIKE_POSTs + CREATE_COMMENTs the first feed post id it is shown (round 2+). This exercises the
/// full feed-back loop: round-1 posts must appear in round-2's prompt as `post-<id>` for the agent
/// to target them.
struct FeedAwareMockLlm {
    completes: AtomicUsize,
}

impl FeedAwareMockLlm {
    fn new() -> Self {
        Self { completes: AtomicUsize::new(0) }
    }

    /// Extract the first `post-<id>` token from a feed prompt, if present.
    fn first_feed_post_id(prompt: &str) -> Option<String> {
        let idx = prompt.find("Recent posts in your feed")?;
        let after = &prompt[idx..];
        let token_idx = after.find("post-")?;
        let rest = &after[token_idx + "post-".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() { None } else { Some(digits) }
    }
}

#[async_trait]
impl LlmClient for FeedAwareMockLlm {
    async fn complete(&self, prompt: &str) -> teri::error::Result<String> {
        let n = self.completes.fetch_add(1, Ordering::SeqCst);
        match Self::first_feed_post_id(prompt) {
            // Round 2+: react to a real post id from the feed. Alternate like/comment per call so
            // both a like AND a comment land on the round-1 post.
            Some(id) if n.is_multiple_of(2) => Ok(format!("LIKE_POST(target_id=post-{id})")),
            Some(id) => Ok(format!("CREATE_COMMENT(post_id=post-{id}, content=great post)")),
            // Round 1 (empty feed): start a conversation.
            None => Ok("CREATE_POST(content=hello from the swarm)".to_string()),
        }
    }

    async fn complete_json<T: serde::de::DeserializeOwned>(
        &self,
        prompt: &str,
    ) -> teri::error::Result<T> {
        let response = self.complete(prompt).await?;
        serde_json::from_str(&response)
            .map_err(|e| teri::error::TeriError::Llm(format!("JSON parsing error: {e}")))
    }

    async fn stream(
        &self,
        _prompt: &str,
    ) -> teri::error::Result<Pin<Box<dyn futures::Stream<Item = teri::error::Result<String>> + Send>>>
    {
        Err(teri::error::TeriError::Llm("not used".to_string()))
    }

    async fn chat(
        &self,
        _messages: &[teri::llm::ChatMessage],
        _opts: &teri::llm::ChatOptions,
    ) -> teri::error::Result<String> {
        Err(teri::error::TeriError::Llm("not used".to_string()))
    }

    async fn chat_json<T: serde::de::DeserializeOwned>(
        &self,
        _messages: &[teri::llm::ChatMessage],
        _opts: &teri::llm::ChatOptions,
    ) -> teri::error::Result<T> {
        Err(teri::error::TeriError::Llm("not used".to_string()))
    }
}

fn social_agent(user_id: u64, name: &str) -> Agent {
    let social = SocialProfile {
        user_id,
        user_name: format!("u{user_id}"),
        bio: String::new(),
        persona: String::new(),
        platform: Platform::Reddit,
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
        source_entity_uuid: None,
        source_entity_type: None,
        created_at: String::new(),
    };
    Agent::new(Persona {
        name: name.to_string(),
        background: "bg".to_string(),
        traits: vec![],
        role: "agent".to_string(),
        social: Some(social),
    })
}

#[tokio::test]
async fn social_world_run_materializes_posts_and_round2_engagement() {
    let tmp = std::env::temp_dir().join(format!("teri_e2e_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();

    // One reddit social agent; 3 ticks so round 1 posts, rounds 2-3 react.
    let mut pool = AgentPool::new();
    pool.add_agent(social_agent(7, "Ada"));

    let mut engine = SimEngine::new(SimConfig::new(3, 1));
    engine.with_social(SocialWorldSet::new([Platform::Reddit], &tmp).unwrap());

    let graph = KnowledgeGraph::new();
    let llm = FeedAwareMockLlm::new();
    let result = engine.run_with_boost(&mut pool, &graph, &llm, None).await.unwrap();
    assert_eq!(result.history.len(), 3);

    // The reader's EXACT query must see a real post.
    let db = tmp.join("reddit_simulation.db");
    assert!(db.exists(), "the social DB must be materialized");
    let conn = rusqlite::Connection::open(&db).unwrap();

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM post", [], |r| r.get(0)).unwrap();
    assert!(total >= 1, "at least the round-1 post must be materialized (got {total})");

    // The round-1 post (post_id 1) must have accumulated a like from a later round — feed-back +
    // apply + flush end-to-end.
    let likes: i64 = conn
        .query_row("SELECT num_likes FROM post WHERE post_id = 1", [], |r| r.get(0))
        .unwrap();
    assert!(
        likes >= 1,
        "round-1 post must be liked in a later round (feed-back proof), got {likes}"
    );

    // A comment must have landed on the round-1 post, readable by the comments reader query.
    let comment_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM comment WHERE post_id = 1", [], |r| r.get(0))
        .unwrap();
    assert!(comment_count >= 1, "round-1 post must have a comment (got {comment_count})");

    // Posts are ordered DESC by created_at by the reader query.
    let mut stmt = conn
        .prepare("SELECT post_id FROM post ORDER BY created_at DESC LIMIT ? OFFSET ?")
        .unwrap();
    let ids: Vec<i64> = stmt
        .query_map(rusqlite::params![50_i64, 0_i64], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert!(!ids.is_empty());
    drop(stmt);

    drop(conn);
    let _ = std::fs::remove_dir_all(&tmp);
}
