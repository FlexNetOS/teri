//! Agent activity DTO and episode-text formatter.
//!
//! Port of `backend/app/services/zep_graph_memory_updater.py` lines 24-199 (MiroFish).
//! Covers: `AgentActivity` dataclass and its `to_episode_text` / `_describe_*` methods ONLY.
//!
//! The `ZepGraphMemoryUpdater` class (L202+) and `ZepGraphMemoryManager` are NOT ported
//! here; they are later sub-cycles (b) and (c) respectively.
//!
//! # Symbol mapping (S-493..S-514)
//!
//! | Source symbol               | Lines   | Rust symbol                                        |
//! |-----------------------------|---------|---------------------------------------------------|
//! | S-493 `AgentActivity`       | 24-33   | `AgentActivity` struct                             |
//! | S-494 `platform`            | 27      | `AgentActivity::platform`                          |
//! | S-495 `agent_id`            | 28      | `AgentActivity::agent_id`                          |
//! | S-496 `agent_name`          | 29      | `AgentActivity::agent_name`                        |
//! | S-497 `action_type`         | 30      | `AgentActivity::action_type`                       |
//! | S-498 `action_args`         | 31      | `AgentActivity::action_args`                       |
//! | S-499 `round_num`           | 32      | `AgentActivity::round_num`                         |
//! | S-500 `timestamp`           | 33      | `AgentActivity::timestamp`                         |
//! | S-501 `to_episode_text`     | 35-62   | `AgentActivity::to_episode_text`                   |
//! | S-502 `_describe_create_post`   | 64-68  | `AgentActivity::describe_create_post`          |
//! | S-503 `_describe_like_post`     | 70-81  | `AgentActivity::describe_like_post`            |
//! | S-504 `_describe_dislike_post`  | 83-94  | `AgentActivity::describe_dislike_post`         |
//! | S-505 `_describe_repost`        | 96-107 | `AgentActivity::describe_repost`               |
//! | S-506 `_describe_quote_post`    | 109-127| `AgentActivity::describe_quote_post`           |
//! | S-507 `_describe_follow`        | 129-135| `AgentActivity::describe_follow`               |
//! | S-508 `_describe_create_comment`| 137-151| `AgentActivity::describe_create_comment`       |
//! | S-509 `_describe_like_comment`  | 153-164| `AgentActivity::describe_like_comment`         |
//! | S-510 `_describe_dislike_comment`| 166-177| `AgentActivity::describe_dislike_comment`     |
//! | S-511 `_describe_search`        | 179-182| `AgentActivity::describe_search`               |
//! | S-512 `_describe_search_user`   | 184-187| `AgentActivity::describe_search_user`          |
//! | S-513 `_describe_mute`          | 189-195| `AgentActivity::describe_mute`                 |
//! | S-514 `_describe_generic`       | 197-199| `AgentActivity::describe_generic`              |
//!
//! # Design notes
//!
//! `action_type` is a plain dispatch string (e.g. "CREATE_POST"), NOT teri's `SocialAction`
//! enum — this is the serialized/loggable form that MiroFish reads from `actions.jsonl`.
//! Coupling it to the live action enum would be a narrowing downgrade.
//!
//! `action_args` is a free-form JSON object (`serde_json::Map<String, Value>`), matching
//! Python's `Dict[str, Any]`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A loggable agent activity record, faithful to MiroFish's `AgentActivity` dataclass.
///
/// Read from `actions.jsonl` and converted to natural-language episode text for Zep
/// graph memory updates.  `action_type` is the raw dispatch string (e.g. "CREATE_POST");
/// `action_args` is the free-form argument dict exactly as serialised in the log file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivity {
    /// Platform identifier: "twitter" or "reddit".
    pub platform: String,
    /// Integer agent ID.
    pub agent_id: i64,
    /// Human-readable agent name (used as the episode prefix).
    pub agent_name: String,
    /// Raw action type string: "CREATE_POST", "LIKE_POST", etc.
    pub action_type: String,
    /// Free-form argument dictionary; keys and value types vary per action_type.
    pub action_args: Map<String, Value>,
    /// Simulation round number.
    pub round_num: i64,
    /// ISO-8601 timestamp string.
    pub timestamp: String,
}

impl AgentActivity {
    /// Retrieve a string argument from `action_args`, defaulting to `""` when the
    /// key is absent or the value is not a JSON string.  Mirrors Python's
    /// `self.action_args.get("key", "")`.
    fn arg<'a>(&'a self, key: &str) -> &'a str {
        self.action_args.get(key).and_then(|v| v.as_str()).unwrap_or("")
    }

    /// Convert this activity to a natural-language episode string suitable for
    /// sending to Zep graph memory.
    ///
    /// Format: `"{agent_name}: {description}"` — NO simulation prefix (source L61-62).
    ///
    /// Port of `to_episode_text` (L35-62).
    pub fn to_episode_text(&self) -> String {
        let description = match self.action_type.as_str() {
            "CREATE_POST" => self.describe_create_post(),
            "LIKE_POST" => self.describe_like_post(),
            "DISLIKE_POST" => self.describe_dislike_post(),
            "REPOST" => self.describe_repost(),
            "QUOTE_POST" => self.describe_quote_post(),
            "FOLLOW" => self.describe_follow(),
            "CREATE_COMMENT" => self.describe_create_comment(),
            "LIKE_COMMENT" => self.describe_like_comment(),
            "DISLIKE_COMMENT" => self.describe_dislike_comment(),
            "SEARCH_POSTS" => self.describe_search(),
            "SEARCH_USER" => self.describe_search_user(),
            "MUTE" => self.describe_mute(),
            _ => self.describe_generic(),
        };
        format!("{}: {}", self.agent_name, description)
    }

    /// Port of `_describe_create_post` (L64-68).
    fn describe_create_post(&self) -> String {
        let content = self.arg("content");
        if !content.is_empty() {
            format!("发布了一条帖子：「{content}」")
        } else {
            "发布了一条帖子".to_string()
        }
    }

    /// Port of `_describe_like_post` (L70-81).
    /// 4-way ladder: content+author / content / author / neither.
    fn describe_like_post(&self) -> String {
        let post_content = self.arg("post_content");
        let post_author = self.arg("post_author_name");

        if !post_content.is_empty() && !post_author.is_empty() {
            format!("点赞了{post_author}的帖子：「{post_content}」")
        } else if !post_content.is_empty() {
            format!("点赞了一条帖子：「{post_content}」")
        } else if !post_author.is_empty() {
            format!("点赞了{post_author}的一条帖子")
        } else {
            "点赞了一条帖子".to_string()
        }
    }

    /// Port of `_describe_dislike_post` (L83-94).
    /// 4-way ladder: content+author / content / author / neither.
    fn describe_dislike_post(&self) -> String {
        let post_content = self.arg("post_content");
        let post_author = self.arg("post_author_name");

        if !post_content.is_empty() && !post_author.is_empty() {
            format!("踩了{post_author}的帖子：「{post_content}」")
        } else if !post_content.is_empty() {
            format!("踩了一条帖子：「{post_content}」")
        } else if !post_author.is_empty() {
            format!("踩了{post_author}的一条帖子")
        } else {
            "踩了一条帖子".to_string()
        }
    }

    /// Port of `_describe_repost` (L96-107).
    /// 4-way ladder on original_content / original_author_name.
    fn describe_repost(&self) -> String {
        let original_content = self.arg("original_content");
        let original_author = self.arg("original_author_name");

        if !original_content.is_empty() && !original_author.is_empty() {
            format!("转发了{original_author}的帖子：「{original_content}」")
        } else if !original_content.is_empty() {
            format!("转发了一条帖子：「{original_content}」")
        } else if !original_author.is_empty() {
            format!("转发了{original_author}的一条帖子")
        } else {
            "转发了一条帖子".to_string()
        }
    }

    /// Port of `_describe_quote_post` (L109-127).
    ///
    /// Base 4-way ladder on original_content / original_author_name, then if
    /// `quote_content` (= `action_args["quote_content"] or action_args["content"]`)
    /// is non-empty, append `，并评论道：「{quote_content}」`.
    ///
    /// The Python `or`-fallback `self.action_args.get("quote_content", "") or
    /// self.action_args.get("content", "")` means: use quote_content if it is a
    /// non-empty string, otherwise fall back to content.
    fn describe_quote_post(&self) -> String {
        let original_content = self.arg("original_content");
        let original_author = self.arg("original_author_name");
        // Faithful `or`-fallback: first non-empty string wins.
        let quote_content_raw = self.arg("quote_content");
        let quote_content =
            if !quote_content_raw.is_empty() { quote_content_raw } else { self.arg("content") };

        let mut base = if !original_content.is_empty() && !original_author.is_empty() {
            format!("引用了{original_author}的帖子「{original_content}」")
        } else if !original_content.is_empty() {
            format!("引用了一条帖子「{original_content}」")
        } else if !original_author.is_empty() {
            format!("引用了{original_author}的一条帖子")
        } else {
            "引用了一条帖子".to_string()
        };

        if !quote_content.is_empty() {
            base.push_str(&format!("，并评论道：「{quote_content}」"));
        }
        base
    }

    /// Port of `_describe_follow` (L129-135).
    fn describe_follow(&self) -> String {
        let target_user_name = self.arg("target_user_name");
        if !target_user_name.is_empty() {
            format!("关注了用户「{target_user_name}」")
        } else {
            "关注了一个用户".to_string()
        }
    }

    /// Port of `_describe_create_comment` (L137-151).
    ///
    /// Outer branch on `content`; if content is non-empty, inner 4-way on
    /// post_content+post_author_name; else fall back to "发表了评论".
    fn describe_create_comment(&self) -> String {
        let content = self.arg("content");
        let post_content = self.arg("post_content");
        let post_author = self.arg("post_author_name");

        if !content.is_empty() {
            if !post_content.is_empty() && !post_author.is_empty() {
                format!("在{post_author}的帖子「{post_content}」下评论道：「{content}」")
            } else if !post_content.is_empty() {
                format!("在帖子「{post_content}」下评论道：「{content}」")
            } else if !post_author.is_empty() {
                format!("在{post_author}的帖子下评论道：「{content}」")
            } else {
                format!("评论道：「{content}」")
            }
        } else {
            "发表了评论".to_string()
        }
    }

    /// Port of `_describe_like_comment` (L153-164).
    /// 4-way ladder on comment_content / comment_author_name.
    fn describe_like_comment(&self) -> String {
        let comment_content = self.arg("comment_content");
        let comment_author = self.arg("comment_author_name");

        if !comment_content.is_empty() && !comment_author.is_empty() {
            format!("点赞了{comment_author}的评论：「{comment_content}」")
        } else if !comment_content.is_empty() {
            format!("点赞了一条评论：「{comment_content}」")
        } else if !comment_author.is_empty() {
            format!("点赞了{comment_author}的一条评论")
        } else {
            "点赞了一条评论".to_string()
        }
    }

    /// Port of `_describe_dislike_comment` (L166-177).
    /// 4-way ladder on comment_content / comment_author_name.
    fn describe_dislike_comment(&self) -> String {
        let comment_content = self.arg("comment_content");
        let comment_author = self.arg("comment_author_name");

        if !comment_content.is_empty() && !comment_author.is_empty() {
            format!("踩了{comment_author}的评论：「{comment_content}」")
        } else if !comment_content.is_empty() {
            format!("踩了一条评论：「{comment_content}」")
        } else if !comment_author.is_empty() {
            format!("踩了{comment_author}的一条评论")
        } else {
            "踩了一条评论".to_string()
        }
    }

    /// Port of `_describe_search` (L179-182).
    ///
    /// `query` = `action_args["query"] or action_args["keyword"]` (first non-empty).
    fn describe_search(&self) -> String {
        let query_raw = self.arg("query");
        let query = if !query_raw.is_empty() { query_raw } else { self.arg("keyword") };
        if !query.is_empty() {
            format!("搜索了「{query}」")
        } else {
            "进行了搜索".to_string()
        }
    }

    /// Port of `_describe_search_user` (L184-187).
    ///
    /// `query` = `action_args["query"] or action_args["username"]` (first non-empty).
    fn describe_search_user(&self) -> String {
        let query_raw = self.arg("query");
        let query = if !query_raw.is_empty() { query_raw } else { self.arg("username") };
        if !query.is_empty() {
            format!("搜索了用户「{query}」")
        } else {
            "搜索了用户".to_string()
        }
    }

    /// Port of `_describe_mute` (L189-195).
    fn describe_mute(&self) -> String {
        let target_user_name = self.arg("target_user_name");
        if !target_user_name.is_empty() {
            format!("屏蔽了用户「{target_user_name}」")
        } else {
            "屏蔽了一个用户".to_string()
        }
    }

    /// Port of `_describe_generic` (L197-199).
    /// Fallback for unknown action types.
    fn describe_generic(&self) -> String {
        format!("执行了{}操作", self.action_type)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a test `AgentActivity` with the given `action_type` and JSON args object.
    fn activity(action_type: &str, args: serde_json::Value) -> AgentActivity {
        AgentActivity {
            platform: "twitter".to_string(),
            agent_id: 1,
            agent_name: "Alice".to_string(),
            action_type: action_type.to_string(),
            action_args: args.as_object().unwrap().clone(),
            round_num: 1,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    // ── to_episode_text prefix ────────────────────────────────────────────────

    #[test]
    fn test_to_episode_text_prefix() {
        let a = activity("CREATE_POST", json!({"content": "hello"}));
        let text = a.to_episode_text();
        assert!(text.starts_with("Alice: "), "expected 'Alice: ' prefix, got: {text:?}");
    }

    #[test]
    fn test_to_episode_text_full_format() {
        let a = activity("CREATE_POST", json!({"content": "hello"}));
        assert_eq!(a.to_episode_text(), "Alice: 发布了一条帖子：「hello」");
    }

    // ── unknown action_type → generic ─────────────────────────────────────────

    #[test]
    fn test_unknown_action_type_generic() {
        let a = activity("DANCE", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 执行了DANCE操作");
    }

    // ── CREATE_POST ───────────────────────────────────────────────────────────

    #[test]
    fn test_create_post_with_content() {
        let a = activity("CREATE_POST", json!({"content": "大家好"}));
        assert_eq!(a.to_episode_text(), "Alice: 发布了一条帖子：「大家好」");
    }

    #[test]
    fn test_create_post_no_content() {
        let a = activity("CREATE_POST", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 发布了一条帖子");
    }

    // ── LIKE_POST (4 branches) ────────────────────────────────────────────────

    #[test]
    fn test_like_post_content_and_author() {
        let a = activity("LIKE_POST", json!({"post_content": "好文章", "post_author_name": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了Bob的帖子：「好文章」");
    }

    #[test]
    fn test_like_post_content_only() {
        let a = activity("LIKE_POST", json!({"post_content": "好文章"}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了一条帖子：「好文章」");
    }

    #[test]
    fn test_like_post_author_only() {
        let a = activity("LIKE_POST", json!({"post_author_name": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了Bob的一条帖子");
    }

    #[test]
    fn test_like_post_neither() {
        let a = activity("LIKE_POST", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了一条帖子");
    }

    // ── DISLIKE_POST (4 branches) ─────────────────────────────────────────────

    #[test]
    fn test_dislike_post_content_and_author() {
        let a =
            activity("DISLIKE_POST", json!({"post_content": "差文章", "post_author_name": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了Bob的帖子：「差文章」");
    }

    #[test]
    fn test_dislike_post_content_only() {
        let a = activity("DISLIKE_POST", json!({"post_content": "差文章"}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了一条帖子：「差文章」");
    }

    #[test]
    fn test_dislike_post_author_only() {
        let a = activity("DISLIKE_POST", json!({"post_author_name": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了Bob的一条帖子");
    }

    #[test]
    fn test_dislike_post_neither() {
        let a = activity("DISLIKE_POST", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了一条帖子");
    }

    // ── REPOST (4 branches) ───────────────────────────────────────────────────

    #[test]
    fn test_repost_content_and_author() {
        let a = activity(
            "REPOST",
            json!({"original_content": "原文", "original_author_name": "Carol"}),
        );
        assert_eq!(a.to_episode_text(), "Alice: 转发了Carol的帖子：「原文」");
    }

    #[test]
    fn test_repost_content_only() {
        let a = activity("REPOST", json!({"original_content": "原文"}));
        assert_eq!(a.to_episode_text(), "Alice: 转发了一条帖子：「原文」");
    }

    #[test]
    fn test_repost_author_only() {
        let a = activity("REPOST", json!({"original_author_name": "Carol"}));
        assert_eq!(a.to_episode_text(), "Alice: 转发了Carol的一条帖子");
    }

    #[test]
    fn test_repost_neither() {
        let a = activity("REPOST", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 转发了一条帖子");
    }

    // ── QUOTE_POST (4-way base + quote_content suffix + or-fallback) ──────────

    #[test]
    fn test_quote_post_content_author_and_quote() {
        let a = activity(
            "QUOTE_POST",
            json!({
                "original_content": "原文",
                "original_author_name": "Carol",
                "quote_content": "我的评论"
            }),
        );
        assert_eq!(a.to_episode_text(), "Alice: 引用了Carol的帖子「原文」，并评论道：「我的评论」");
    }

    #[test]
    fn test_quote_post_content_and_author_no_quote() {
        let a = activity(
            "QUOTE_POST",
            json!({"original_content": "原文", "original_author_name": "Carol"}),
        );
        assert_eq!(a.to_episode_text(), "Alice: 引用了Carol的帖子「原文」");
    }

    #[test]
    fn test_quote_post_original_content_only() {
        let a = activity("QUOTE_POST", json!({"original_content": "原文"}));
        assert_eq!(a.to_episode_text(), "Alice: 引用了一条帖子「原文」");
    }

    #[test]
    fn test_quote_post_original_author_only() {
        let a = activity("QUOTE_POST", json!({"original_author_name": "Carol"}));
        assert_eq!(a.to_episode_text(), "Alice: 引用了Carol的一条帖子");
    }

    #[test]
    fn test_quote_post_neither_no_quote() {
        let a = activity("QUOTE_POST", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 引用了一条帖子");
    }

    /// quote_content absent but content present → or-fallback uses content.
    #[test]
    fn test_quote_post_or_fallback_uses_content() {
        let a = activity("QUOTE_POST", json!({"content": "备用评论"}));
        assert_eq!(a.to_episode_text(), "Alice: 引用了一条帖子，并评论道：「备用评论」");
    }

    /// quote_content present (non-empty) → takes precedence over content.
    #[test]
    fn test_quote_post_or_fallback_quote_content_wins() {
        let a = activity("QUOTE_POST", json!({"quote_content": "优先评论", "content": "备用评论"}));
        assert_eq!(a.to_episode_text(), "Alice: 引用了一条帖子，并评论道：「优先评论」");
    }

    /// quote_content is empty string → falls back to content (Python falsy or-chain).
    #[test]
    fn test_quote_post_or_fallback_empty_quote_content_uses_content() {
        let a = activity("QUOTE_POST", json!({"quote_content": "", "content": "备用评论"}));
        assert_eq!(a.to_episode_text(), "Alice: 引用了一条帖子，并评论道：「备用评论」");
    }

    // ── FOLLOW ────────────────────────────────────────────────────────────────

    #[test]
    fn test_follow_with_name() {
        let a = activity("FOLLOW", json!({"target_user_name": "Dave"}));
        assert_eq!(a.to_episode_text(), "Alice: 关注了用户「Dave」");
    }

    #[test]
    fn test_follow_no_name() {
        let a = activity("FOLLOW", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 关注了一个用户");
    }

    // ── CREATE_COMMENT (outer content branch + 4-way inner) ──────────────────

    #[test]
    fn test_create_comment_content_post_content_and_author() {
        let a = activity(
            "CREATE_COMMENT",
            json!({
                "content": "我的看法",
                "post_content": "原帖",
                "post_author_name": "Eve"
            }),
        );
        assert_eq!(a.to_episode_text(), "Alice: 在Eve的帖子「原帖」下评论道：「我的看法」");
    }

    #[test]
    fn test_create_comment_content_post_content_only() {
        let a = activity("CREATE_COMMENT", json!({"content": "我的看法", "post_content": "原帖"}));
        assert_eq!(a.to_episode_text(), "Alice: 在帖子「原帖」下评论道：「我的看法」");
    }

    #[test]
    fn test_create_comment_content_post_author_only() {
        let a =
            activity("CREATE_COMMENT", json!({"content": "我的看法", "post_author_name": "Eve"}));
        assert_eq!(a.to_episode_text(), "Alice: 在Eve的帖子下评论道：「我的看法」");
    }

    #[test]
    fn test_create_comment_content_no_post_info() {
        let a = activity("CREATE_COMMENT", json!({"content": "我的看法"}));
        assert_eq!(a.to_episode_text(), "Alice: 评论道：「我的看法」");
    }

    #[test]
    fn test_create_comment_no_content() {
        let a = activity("CREATE_COMMENT", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 发表了评论");
    }

    // ── LIKE_COMMENT (4 branches) ─────────────────────────────────────────────

    #[test]
    fn test_like_comment_content_and_author() {
        let a = activity(
            "LIKE_COMMENT",
            json!({"comment_content": "好评论", "comment_author_name": "Frank"}),
        );
        assert_eq!(a.to_episode_text(), "Alice: 点赞了Frank的评论：「好评论」");
    }

    #[test]
    fn test_like_comment_content_only() {
        let a = activity("LIKE_COMMENT", json!({"comment_content": "好评论"}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了一条评论：「好评论」");
    }

    #[test]
    fn test_like_comment_author_only() {
        let a = activity("LIKE_COMMENT", json!({"comment_author_name": "Frank"}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了Frank的一条评论");
    }

    #[test]
    fn test_like_comment_neither() {
        let a = activity("LIKE_COMMENT", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 点赞了一条评论");
    }

    // ── DISLIKE_COMMENT (4 branches) ──────────────────────────────────────────

    #[test]
    fn test_dislike_comment_content_and_author() {
        let a = activity(
            "DISLIKE_COMMENT",
            json!({"comment_content": "差评论", "comment_author_name": "Frank"}),
        );
        assert_eq!(a.to_episode_text(), "Alice: 踩了Frank的评论：「差评论」");
    }

    #[test]
    fn test_dislike_comment_content_only() {
        let a = activity("DISLIKE_COMMENT", json!({"comment_content": "差评论"}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了一条评论：「差评论」");
    }

    #[test]
    fn test_dislike_comment_author_only() {
        let a = activity("DISLIKE_COMMENT", json!({"comment_author_name": "Frank"}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了Frank的一条评论");
    }

    #[test]
    fn test_dislike_comment_neither() {
        let a = activity("DISLIKE_COMMENT", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 踩了一条评论");
    }

    // ── SEARCH_POSTS (query / keyword or-fallback) ────────────────────────────

    #[test]
    fn test_search_posts_with_query() {
        let a = activity("SEARCH_POSTS", json!({"query": "Rust"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了「Rust」");
    }

    /// query absent, keyword present → or-fallback uses keyword.
    #[test]
    fn test_search_posts_keyword_fallback() {
        let a = activity("SEARCH_POSTS", json!({"keyword": "开源"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了「开源」");
    }

    /// query is non-empty → takes precedence over keyword.
    #[test]
    fn test_search_posts_query_wins_over_keyword() {
        let a = activity("SEARCH_POSTS", json!({"query": "Rust", "keyword": "开源"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了「Rust」");
    }

    /// query empty string → falls back to keyword.
    #[test]
    fn test_search_posts_empty_query_uses_keyword() {
        let a = activity("SEARCH_POSTS", json!({"query": "", "keyword": "开源"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了「开源」");
    }

    #[test]
    fn test_search_posts_no_query_no_keyword() {
        let a = activity("SEARCH_POSTS", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 进行了搜索");
    }

    // ── SEARCH_USER (query / username or-fallback) ────────────────────────────

    #[test]
    fn test_search_user_with_query() {
        let a = activity("SEARCH_USER", json!({"query": "Alice"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了用户「Alice」");
    }

    /// query absent, username present → or-fallback uses username.
    #[test]
    fn test_search_user_username_fallback() {
        let a = activity("SEARCH_USER", json!({"username": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了用户「Bob」");
    }

    /// query non-empty → takes precedence over username.
    #[test]
    fn test_search_user_query_wins_over_username() {
        let a = activity("SEARCH_USER", json!({"query": "Alice", "username": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了用户「Alice」");
    }

    /// query empty string → falls back to username.
    #[test]
    fn test_search_user_empty_query_uses_username() {
        let a = activity("SEARCH_USER", json!({"query": "", "username": "Bob"}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了用户「Bob」");
    }

    #[test]
    fn test_search_user_no_query_no_username() {
        let a = activity("SEARCH_USER", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 搜索了用户");
    }

    // ── MUTE ─────────────────────────────────────────────────────────────────

    #[test]
    fn test_mute_with_name() {
        let a = activity("MUTE", json!({"target_user_name": "Troll"}));
        assert_eq!(a.to_episode_text(), "Alice: 屏蔽了用户「Troll」");
    }

    #[test]
    fn test_mute_no_name() {
        let a = activity("MUTE", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 屏蔽了一个用户");
    }

    // ── Serialization round-trip (serde) ──────────────────────────────────────

    #[test]
    fn test_serde_round_trip() {
        let a = activity("LIKE_POST", json!({"post_content": "hello", "post_author_name": "Bob"}));
        let json_str = serde_json::to_string(&a).expect("serialize");
        let b: AgentActivity = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(a.to_episode_text(), b.to_episode_text());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GraphMemoryUpdater — Sub-cycle (b) port of `ZepGraphMemoryUpdater` (S-515..S-530)
// ─────────────────────────────────────────────────────────────────────────────
//
// Symbol mapping (DECISION-14 / S-515..S-530):
//
// | Source symbol (Python)                  | Lines    | teri target                              |
// |-----------------------------------------|----------|------------------------------------------|
// | S-515 `ZepGraphMemoryUpdater` class     | 202      | `GraphMemoryUpdater` struct              |
// | S-516 `__init__`                        | 232-269  | `GraphMemoryUpdater::new` (no api_key)  |
// | S-517 `PLATFORM_DISPLAY_NAMES` +        | 220-223, | `platform_display_name` fn (PORTED)     |
// |        `_get_platform_display_name`     | 271-273  |                                          |
// | S-518 `BATCH_SIZE`                      | 217      | `const BATCH_SIZE: usize = 5`           |
// | S-519 `MAX_RETRIES`                     | 229      | `[≠]` literal retry-loop (non-contractual)|
// | S-520 `RETRY_DELAY`                     | 230      | `[≠]` network backoff delay              |
// | S-521 `SEND_INTERVAL`                   | 226      | `[≠]` Zep network rate-limit            |
// | S-522 `start`                           | 275-291  | `start` — locale-capture + spawn        |
// | S-523 `stop`                            | 293-308  | `stop` async — drop tx + join + log     |
// | S-524 `add_activity`                    | 310-338  | `add_activity` — DO_NOTHING skip + send |
// | S-525 `add_activity_from_dict`          | 340-362  | `add_activity_from_dict`                |
// | S-526 `_worker_loop`                    | 364-394  | spawned worker future                   |
// | S-527 `_send_batch_activities`          | 396-433  | `flush_batch` — combined_text + extend  |
// | S-528 `_flush_remaining`               | 435-458  | worker drain-on-close + per-platform    |
// | S-529 `get_stats`                       | 460-476  | `get_stats` → `UpdaterStats` (serde)    |
//
// `[≠]` adjudications (DECISION-14 Decision 4):
// - `[≠]` S-519/S-520 SEND_INTERVAL — Zep network rate-limit; non-contractual in-process
// - `[≠]` S-520 MAX_RETRIES/RETRY_DELAY literal retry-loop — Zep-network transient-retry;
//          in-process keeps failed_count + continue-on-error, drops network retry cadence
// - `[≠]` S-516 ZEP_API_KEY check — native graph is keyless; substrate-absent
// - `[≠]` Zep coreference entity-resolution — teri merges by exact name; no entity dropped
//
// PORTED (not `[≠]`):
// - BATCH_SIZE=5, platform display names ("世界1"/"世界2"), combined_text "\n".join,
//   DO_NOTHING skip, event_type skip, all 5 stat counters, get_stats, _flush_remaining
//   leftovers, per-platform independent batching.

use crate::graph::KnowledgeGraph;
use crate::llm::LlmClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// Batch size: number of same-platform activities to accumulate before flushing.
///
/// Port of `ZepGraphMemoryUpdater.BATCH_SIZE = 5` (S-518, L217).
const BATCH_SIZE: usize = 5;

/// Resolve a platform identifier to its console display name.
///
/// Port of `ZepGraphMemoryUpdater.PLATFORM_DISPLAY_NAMES` + `_get_platform_display_name`
/// (S-517, L220-223, L271-273). PORTED (used in log lines — observable output).
///
/// `twitter` → `"世界1"`, `reddit` → `"世界2"`, anything else → the input string slice.
fn platform_display_name(platform: &str) -> &str {
    match platform.to_lowercase().as_str() {
        "twitter" => "世界1",
        "reddit" => "世界2",
        _ => platform,
    }
}

/// Atomic counters shared between the producer side (`add_activity`) and the worker task
/// (`flush_batch`, `_flush_remaining`).
///
/// All counters use `Relaxed` ordering — they are updated by at most two parties (producer
/// and worker) and read by `get_stats`, where a snapshot (not a strict linearisation) is
/// the only observable requirement (matches Python's non-atomic incrementing style).
#[derive(Debug, Default)]
struct UpdaterCounters {
    /// Activities actually enqueued (excludes DO_NOTHING skips).
    total_activities: AtomicI64,
    /// Batches successfully flushed to the graph.
    total_sent: AtomicI64,
    /// Activities successfully flushed to the graph (sum of batch sizes).
    total_items_sent: AtomicI64,
    /// Batches that failed to flush (extend_from_text error; worker continues).
    failed_count: AtomicI64,
    /// Activities skipped because `action_type == "DO_NOTHING"`.
    skipped_count: AtomicI64,
    /// Approximate queue depth: incremented on send, decremented when worker pops.
    /// Best-effort — mpsc::UnboundedReceiver has no public len().
    queued: AtomicUsize,
}

/// Stats snapshot returned by [`GraphMemoryUpdater::get_stats`].
///
/// Byte-identical JSON keys to MiroFish `ZepGraphMemoryUpdater.get_stats()` (S-529, L465-476).
/// The key `graph_id` maps to `graph_label` (the teri analog; no Zep server handle).
#[derive(Debug, Clone, Serialize)]
pub struct UpdaterStats {
    /// The `graph_label` string (teri analog of Python's `graph_id`).
    pub graph_id: String,
    /// Batch size constant (always 5).
    pub batch_size: usize,
    /// Activities enqueued (excludes DO_NOTHING).
    pub total_activities: i64,
    /// Batches successfully flushed.
    pub batches_sent: i64,
    /// Activities successfully flushed.
    pub items_sent: i64,
    /// Batches that failed to flush.
    pub failed_count: i64,
    /// Activities skipped (DO_NOTHING).
    pub skipped_count: i64,
    /// Approximate queue depth.
    pub queue_size: usize,
    /// Per-platform buffer sizes (activities accumulated but not yet batched).
    pub buffer_sizes: HashMap<String, usize>,
    /// Whether the worker task is running.
    pub running: bool,
}

/// Snapshot of per-platform buffer sizes, written by the worker and read by `get_stats`.
///
/// The worker task owns the actual buffers; it updates this snapshot after every mutation
/// so `get_stats` can read it without entering the worker's ownership domain.
type BufferSnapshot = Arc<Mutex<HashMap<String, usize>>>;

/// Async background graph-memory updater.
///
/// Port of `ZepGraphMemoryUpdater` (S-515, L202-476, MiroFish).
///
/// Accumulates [`AgentActivity`] records per platform and flushes them in batches of
/// [`BATCH_SIZE`] = 5 to the underlying [`KnowledgeGraph`] via
/// [`KnowledgeGraph::extend_from_text`].  Decoupled via a tokio mpsc channel so the
/// simulation hot-path never blocks on LLM extraction.
///
/// # Generic `L: LlmClient` note (DECISION-14)
///
/// DECISION-14 proposed `Arc<dyn LlmClient>`, but `LlmClient` has generic methods
/// (`complete_json<T>`, `chat_json<T>`) making it non-dyn-compatible in Rust.  Per the
/// task brief: "if `LlmClient` isn't dyn-safe, use a generic `<L: LlmClient + Send + Sync
/// + 'static>` and note it."  The observable contract is identical; the type parameter is
///   the implementation detail.
///
/// # Substrate differences from Python (`[≠]`)
/// - No `ZEP_API_KEY` validation (keyless substrate).
/// - No `SEND_INTERVAL` sleep (Zep network rate-limit; non-contractual in-process).
/// - No literal `MAX_RETRIES` / `RETRY_DELAY` retry-loop (Zep-network transient-retry;
///   `failed_count + continue-on-error` IS ported).
/// - Entity merge by EXACT name (no Zep coreference/fuzzy resolution; no entity dropped).
///
/// # Observable contract (PORTED)
/// All 5 stat counters, `get_stats`, `add_activity` DO_NOTHING skip, `add_activity_from_dict`
/// event_type skip, `combined_text = episode_texts.join("\n")`, per-platform independent
/// batching at `BATCH_SIZE`, `_flush_remaining` on `stop`, platform display names in logs.
pub struct GraphMemoryUpdater<L: LlmClient + Send + Sync + 'static> {
    /// The target knowledge graph (shared, async-mutex-protected).
    graph: Arc<Mutex<KnowledgeGraph>>,
    /// LLM client for entity/relation extraction.
    llm: Arc<L>,
    /// Label used in log lines (was `graph_id` in Python; the teri analog).
    graph_label: String,
    /// Sender half of the activity channel.  Dropped on `stop()` to signal the worker.
    tx: Option<mpsc::UnboundedSender<AgentActivity>>,
    /// Worker task handle.  Joined on `stop()`.
    worker: Option<JoinHandle<()>>,
    /// Whether the worker is running.
    running: Arc<AtomicBool>,
    /// Counters — shared between producer and worker.
    counters: Arc<UpdaterCounters>,
    /// Buffer size snapshot — written by worker, read by get_stats.
    buffer_snapshot: BufferSnapshot,
    /// Workstream B (U6): optional vector index; when set, each flushed batch's new facts are
    /// re-embedded into the redb store under `graph_namespace(graph_label)`.
    vector_index: Option<crate::services::graph_builder::GraphVectorIndex>,
}

impl<L: LlmClient + Send + Sync + 'static> GraphMemoryUpdater<L> {
    /// Construct a new updater.
    ///
    /// Port of `ZepGraphMemoryUpdater.__init__` (S-516, L232-269).
    ///
    /// The Python `api_key` / `ZEP_API_KEY` check is `[≠]` substrate-absent (the native
    /// graph requires no API key).
    ///
    /// Call [`start`](Self::start) before adding activities.
    pub fn new(graph: Arc<Mutex<KnowledgeGraph>>, llm: Arc<L>, graph_label: String) -> Self {
        info!(
            "GraphMemoryUpdater 初始化完成: graph_label={}, batch_size={}",
            graph_label, BATCH_SIZE
        );
        // Seed the buffer-size snapshot with the two initial platform keys (twitter + reddit),
        // matching MiroFish's `self._platform_buffers = {'twitter': [], 'reddit': []}` in
        // `__init__` (L252-255).  This ensures `get_stats().buffer_sizes` always contains
        // both keys (value 0) even before any activity has been received by the worker.
        let mut initial_snapshot = HashMap::new();
        initial_snapshot.insert("twitter".to_string(), 0usize);
        initial_snapshot.insert("reddit".to_string(), 0usize);
        Self {
            graph,
            llm,
            graph_label,
            tx: None,
            worker: None,
            running: Arc::new(AtomicBool::new(false)),
            counters: Arc::new(UpdaterCounters::default()),
            buffer_snapshot: Arc::new(Mutex::new(initial_snapshot)),
            vector_index: None,
        }
    }

    /// Workstream B (U6): attach a vector index so flushed batches re-embed their new facts
    /// (builder-style). `None` is a no-op (keyword search still surfaces accrued facts).
    pub fn with_vector_index(
        mut self,
        vector_index: Option<crate::services::graph_builder::GraphVectorIndex>,
    ) -> Self {
        self.vector_index = vector_index;
        self
    }

    /// Start the background worker task.
    ///
    /// Port of `ZepGraphMemoryUpdater.start` (S-522, L275-291).
    ///
    /// Idempotent: if already running, returns immediately.
    /// Captures the current locale before spawning (U-050 site: `get_locale()` + `with_locale`).
    pub fn start(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            return;
        }

        // U-050: capture locale before the spawn so the worker runs under the same locale.
        let locale = crate::i18n::get_locale();

        let (tx, rx) = mpsc::unbounded_channel::<AgentActivity>();
        self.tx = Some(tx);
        self.running.store(true, Ordering::Relaxed);

        let graph = Arc::clone(&self.graph);
        let llm = Arc::clone(&self.llm);
        let graph_label = self.graph_label.clone();
        let running = Arc::clone(&self.running);
        let counters = Arc::clone(&self.counters);
        let buffer_snapshot = Arc::clone(&self.buffer_snapshot);
        let vector_index = self.vector_index.clone();

        let handle = tokio::spawn(crate::i18n::with_locale(locale, async move {
            worker_loop(
                rx,
                graph,
                llm,
                graph_label,
                running,
                counters,
                buffer_snapshot,
                vector_index,
            )
            .await;
        }));

        self.worker = Some(handle);
        info!("GraphMemoryUpdater 已启动: graph_label={}", self.graph_label);
    }

    /// Stop the background worker task and flush any remaining activities.
    ///
    /// Port of `ZepGraphMemoryUpdater.stop` (S-523, L293-308).
    ///
    /// Drops the sender (signals the worker `recv()` returns `None`) then awaits the worker
    /// to complete its final flush.  Mirrors the Python `join(timeout=10)` safety cap via
    /// `tokio::time::timeout(10s, handle)` — the timeout is a safety guard, not a contract.
    pub async fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);

        // Drop the sender — this signals the worker's recv() loop to drain and exit.
        self.tx.take();

        // Await the worker (with a 10s safety cap matching Python's join(timeout=10)).
        if let Some(handle) = self.worker.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(10), handle).await;
        }

        let total_activities = self.counters.total_activities.load(Ordering::Relaxed);
        let total_sent = self.counters.total_sent.load(Ordering::Relaxed);
        let total_items_sent = self.counters.total_items_sent.load(Ordering::Relaxed);
        let failed_count = self.counters.failed_count.load(Ordering::Relaxed);
        let skipped_count = self.counters.skipped_count.load(Ordering::Relaxed);

        info!(
            "GraphMemoryUpdater 已停止: graph_label={}, total_activities={}, batches_sent={}, \
             items_sent={}, failed={}, skipped={}",
            self.graph_label,
            total_activities,
            total_sent,
            total_items_sent,
            failed_count,
            skipped_count
        );
    }

    /// Add an agent activity to the queue for batched graph update.
    ///
    /// Port of `ZepGraphMemoryUpdater.add_activity` (S-524, L310-338).
    ///
    /// `DO_NOTHING` activities are skipped BEFORE enqueue (`skipped_count += 1`, return).
    /// All other activities are sent to the worker channel and `total_activities` is incremented.
    /// Runs on the producer side (simulation hot-path); never blocks.
    pub fn add_activity(&self, activity: AgentActivity) {
        // Contractual filter: skip DO_NOTHING before enqueue.
        if activity.action_type == "DO_NOTHING" {
            self.counters.skipped_count.fetch_add(1, Ordering::Relaxed);
            return;
        }

        if let Some(tx) = &self.tx {
            // Ignore send errors if the worker is gone (stop() was called).
            let _ = tx.send(activity);
            self.counters.total_activities.fetch_add(1, Ordering::Relaxed);
            self.counters.queued.fetch_add(1, Ordering::Relaxed);
        }

        debug!("添加活动到图谱队列: graph_label={}", self.graph_label);
    }

    /// Build an [`AgentActivity`] from a JSON dict and enqueue it.
    ///
    /// Port of `ZepGraphMemoryUpdater.add_activity_from_dict` (S-525, L340-362).
    ///
    /// Entries containing an `event_type` key are skipped (they are simulation-event markers,
    /// not agent-action records).  All other entries are converted to [`AgentActivity`] using
    /// the same field defaults as Python (`data.get("key", default)`).
    pub fn add_activity_from_dict(&self, data: &serde_json::Value, platform: &str) {
        // Skip event-type entries (contractual, L349-350).
        if data.get("event_type").is_some() {
            return;
        }

        // Construct AgentActivity from the dict fields with Python-identical defaults.
        // `round` key → `round_num` (Python `data.get("round", 0)`).
        let agent_id = data.get("agent_id").and_then(|v| v.as_i64()).unwrap_or(0);
        let agent_name = data.get("agent_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let action_type =
            data.get("action_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let action_args =
            data.get("action_args").and_then(|v| v.as_object()).cloned().unwrap_or_default();
        let round_num = data.get("round").and_then(|v| v.as_i64()).unwrap_or(0);
        // Default timestamp: chrono RFC 3339 now (mirrors Python's `datetime.now().isoformat()`).
        let timestamp = data
            .get("timestamp")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

        let activity = AgentActivity {
            platform: platform.to_string(),
            agent_id,
            agent_name,
            action_type,
            action_args,
            round_num,
            timestamp,
        };

        self.add_activity(activity);
    }

    /// Return a stats snapshot.
    ///
    /// Port of `ZepGraphMemoryUpdater.get_stats` (S-529, L460-476).
    ///
    /// JSON key names are byte-identical to the Python source for API contract parity.
    /// `graph_id` maps to `graph_label` (the teri analog; no Zep server handle).
    /// `queue_size` is best-effort (approximated via an atomic counter).
    pub async fn get_stats(&self) -> UpdaterStats {
        let buffer_sizes = self.buffer_snapshot.lock().await.clone();
        UpdaterStats {
            graph_id: self.graph_label.clone(),
            batch_size: BATCH_SIZE,
            total_activities: self.counters.total_activities.load(Ordering::Relaxed),
            batches_sent: self.counters.total_sent.load(Ordering::Relaxed),
            items_sent: self.counters.total_items_sent.load(Ordering::Relaxed),
            failed_count: self.counters.failed_count.load(Ordering::Relaxed),
            skipped_count: self.counters.skipped_count.load(Ordering::Relaxed),
            queue_size: self.counters.queued.load(Ordering::Relaxed),
            buffer_sizes,
            running: self.running.load(Ordering::Relaxed),
        }
    }
}

// ─── Worker task (private) ────────────────────────────────────────────────────

/// The spawned worker loop.
///
/// Port of `ZepGraphMemoryUpdater._worker_loop` (S-526, L364-394) +
/// `_send_batch_activities` (S-527, L396-433) + `_flush_remaining` (S-528, L435-458).
///
/// Runs under the locale captured at `start()` via `with_locale` (U-050).
///
/// Receive loop: each activity is placed in per-platform buffers. When a platform's buffer
/// reaches `BATCH_SIZE`, the first `BATCH_SIZE` activities are drained and flushed.
/// When the sender is dropped (`stop()` drops `tx`), `recv()` returns `None`; the loop exits
/// and the drain-and-flush (`_flush_remaining`) is performed for all non-empty buffers.
///
/// `[≠]` SEND_INTERVAL between flushes: the 0.5 s Zep network rate-limit sleep is omitted
/// (in-process extraction has no remote rate to throttle; non-contractual; DECISION-14 §4).
#[allow(clippy::too_many_arguments)]
async fn worker_loop<L: LlmClient + Send + Sync + 'static>(
    mut rx: mpsc::UnboundedReceiver<AgentActivity>,
    graph: Arc<Mutex<KnowledgeGraph>>,
    llm: Arc<L>,
    graph_label: String,
    running: Arc<AtomicBool>,
    counters: Arc<UpdaterCounters>,
    buffer_snapshot: BufferSnapshot,
    vector_index: Option<crate::services::graph_builder::GraphVectorIndex>,
) {
    // Per-platform activity buffers — exclusively owned by the worker (no lock needed).
    // Seeded with the two initial platforms matching Python's `_platform_buffers` (L252-255).
    let mut platform_buffers: HashMap<String, Vec<AgentActivity>> = {
        let mut m = HashMap::new();
        m.insert("twitter".to_string(), Vec::new());
        m.insert("reddit".to_string(), Vec::new());
        m
    };

    // Main receive loop (port of _worker_loop while loop, L367-394).
    while let Some(activity) = rx.recv().await {
        counters.queued.fetch_sub(1, Ordering::Relaxed);

        let platform = activity.platform.to_lowercase();
        platform_buffers.entry(platform.clone()).or_default().push(activity);

        // Update the buffer-size snapshot for get_stats.
        update_buffer_snapshot(&platform_buffers, &buffer_snapshot).await;

        // If this platform has reached BATCH_SIZE, drain and flush.
        if platform_buffers.get(&platform).map_or(0, Vec::len) >= BATCH_SIZE {
            let batch: Vec<AgentActivity> = platform_buffers
                .get_mut(&platform)
                .map(|buf| buf.drain(..BATCH_SIZE).collect())
                .unwrap_or_default();

            update_buffer_snapshot(&platform_buffers, &buffer_snapshot).await;

            flush_batch(
                batch,
                &platform,
                &graph,
                &llm,
                &graph_label,
                &counters,
                vector_index.as_ref(),
            )
            .await;
            // [≠] SEND_INTERVAL 0.5 s sleep omitted (Zep network rate-limit; non-contractual).
        }
    }

    // Channel closed (stop() dropped the sender) — drain and flush remaining buffers.
    // Port of `_flush_remaining` (S-528, L435-458):
    // Since buffers live in the worker (no separate queue-to-buffer drain step needed),
    // we simply flush every non-empty per-platform buffer once.
    running.store(false, Ordering::Relaxed);

    for (platform, buffer) in &mut platform_buffers {
        if !buffer.is_empty() {
            let platform_name = platform_display_name(platform);
            info!("发送{}平台剩余的 {} 条活动", platform_name, buffer.len());
            let batch: Vec<AgentActivity> = std::mem::take(buffer);
            flush_batch(
                batch,
                platform,
                &graph,
                &llm,
                &graph_label,
                &counters,
                vector_index.as_ref(),
            )
            .await;
        }
    }

    update_buffer_snapshot(&platform_buffers, &buffer_snapshot).await;
}

/// Update the buffer-size snapshot from the worker's current buffers.
async fn update_buffer_snapshot(
    platform_buffers: &HashMap<String, Vec<AgentActivity>>,
    snapshot: &BufferSnapshot,
) {
    let mut guard = snapshot.lock().await;
    *guard = platform_buffers.iter().map(|(k, v)| (k.clone(), v.len())).collect();
}

/// Flush a batch of activities to the knowledge graph.
///
/// Port of `ZepGraphMemoryUpdater._send_batch_activities` (S-527, L396-433).
///
/// Observable contract:
/// - `combined_text = batch.iter().map(to_episode_text).collect().join("\n")` (EXACT, observable).
/// - On success: `total_sent += 1`, `total_items_sent += batch.len()`, info log w/ display_name.
/// - On error: `failed_count += 1`, error log, worker continues (non-fatal).
///
/// `[≠]` MAX_RETRIES/RETRY_DELAY literal retry-loop: the 3-attempt Zep-network retry cadence
/// is omitted (DECISION-14 §4). The *resilience* (continue-on-error + failed_count) IS ported.
#[allow(clippy::too_many_arguments)]
async fn flush_batch<L: LlmClient + Send + Sync + 'static>(
    activities: Vec<AgentActivity>,
    platform: &str,
    graph: &Arc<Mutex<KnowledgeGraph>>,
    llm: &Arc<L>,
    graph_label: &str,
    counters: &Arc<UpdaterCounters>,
    vector_index: Option<&crate::services::graph_builder::GraphVectorIndex>,
) {
    if activities.is_empty() {
        return;
    }

    // combined_text: observable join — exact port of L407-409.
    let episode_texts: Vec<String> =
        activities.iter().map(AgentActivity::to_episode_text).collect();
    let combined_text = episode_texts.join("\n");

    debug!("批量内容预览: {}...", &combined_text[..combined_text.len().min(200)]);

    // Extend the graph with the combined text.
    // Holds the Mutex across the LLM await — required (async Mutex), and the lock is held
    // only while extraction is active (one flush at a time in the single worker).
    let result = {
        let mut g = graph.lock().await;
        g.extend_from_text(&combined_text, llm.as_ref()).await
    };

    match result {
        Ok(_stats) => {
            counters.total_sent.fetch_add(1, Ordering::Relaxed);
            counters.total_items_sent.fetch_add(activities.len() as i64, Ordering::Relaxed);
            let platform_name = platform_display_name(platform);
            info!(
                "成功批量发送 {} 条{}活动到图谱 {}",
                activities.len(),
                platform_name,
                graph_label
            );

            // Workstream B (U6): re-embed the accrued episode so sim-accrued facts become
            // searchable by cosine (keyword search already surfaces them — this is the cosine
            // upgrade). Best-effort: an embed failure is logged inside append_graph_vector.
            if let Some(idx) = vector_index {
                crate::services::graph_backend::append_graph_vector(
                    graph_label,
                    &combined_text,
                    &idx.embedder,
                    &idx.store,
                )
                .await;
            }
        }
        Err(e) => {
            // Non-fatal: increment failed_count and continue (port of L433, L392-394).
            counters.failed_count.fetch_add(1, Ordering::Relaxed);
            error!("批量发送到图谱失败: graph_label={}, error={}", graph_label, e);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests for GraphMemoryUpdater (sub-cycle b)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod updater_tests {
    use super::*;
    use crate::error::{Result, TeriError};
    use crate::graph::KnowledgeGraph;
    use crate::llm::{ChatMessage, ChatOptions, LlmClient};
    use async_trait::async_trait;
    use serde_json::json;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // ── Mock LLM ─────────────────────────────────────────────────────────────

    /// Deterministic mock LLM for updater tests.
    ///
    /// Alternates between two canned responses: the first `complete()` call returns
    /// `entity_response`, the second returns `relation_response`, cycling.
    struct MockLlm {
        entity_response: String,
        relation_response: String,
        call_count: tokio::sync::Mutex<usize>,
    }

    impl MockLlm {
        fn new(entity_response: &str, relation_response: &str) -> Self {
            Self {
                entity_response: entity_response.to_string(),
                relation_response: relation_response.to_string(),
                call_count: tokio::sync::Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            let mut count = self.call_count.lock().await;
            let response = if *count % 2 == 0 {
                self.entity_response.clone()
            } else {
                self.relation_response.clone()
            };
            *count += 1;
            Ok(response)
        }

        async fn complete_json<T: serde::de::DeserializeOwned>(&self, prompt: &str) -> Result<T> {
            let text = self.complete(prompt).await?;
            serde_json::from_str(&text).map_err(|e| TeriError::Llm(format!("mock json parse: {e}")))
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }

        async fn chat(&self, _messages: &[ChatMessage], _opts: &ChatOptions) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }

        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _messages: &[ChatMessage],
            _opts: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    /// Build a test activity with the given platform and action_type.
    fn make_activity(platform: &str, action_type: &str) -> AgentActivity {
        AgentActivity {
            platform: platform.to_string(),
            agent_id: 1,
            agent_name: "Agent1".to_string(),
            action_type: action_type.to_string(),
            action_args: serde_json::Map::new(),
            round_num: 1,
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    // ── platform_display_name ─────────────────────────────────────────────────

    #[test]
    fn test_platform_display_name_twitter() {
        assert_eq!(platform_display_name("twitter"), "世界1");
        assert_eq!(platform_display_name("TWITTER"), "世界1");
    }

    #[test]
    fn test_platform_display_name_reddit() {
        assert_eq!(platform_display_name("reddit"), "世界2");
        assert_eq!(platform_display_name("Reddit"), "世界2");
    }

    #[test]
    fn test_platform_display_name_unknown() {
        assert_eq!(platform_display_name("discord"), "discord");
    }

    // ── add_activity: DO_NOTHING skip ─────────────────────────────────────────

    #[tokio::test]
    async fn test_add_activity_do_nothing_skipped() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "test-graph".to_string());
        updater.start();

        let a = make_activity("twitter", "DO_NOTHING");
        updater.add_activity(a);

        let stats = updater.get_stats().await;
        assert_eq!(stats.skipped_count, 1, "DO_NOTHING must increment skipped_count");
        assert_eq!(stats.total_activities, 0, "DO_NOTHING must not increment total_activities");

        updater.stop().await;
    }

    // ── add_activity: real activities are enqueued ────────────────────────────

    #[tokio::test]
    async fn test_add_activity_real_action_enqueued() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "test-graph".to_string());
        updater.start();

        let a = make_activity("twitter", "CREATE_POST");
        updater.add_activity(a);

        // Give the worker a moment to pop (no flush threshold yet; just counters).
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let stats = updater.get_stats().await;
        assert_eq!(stats.total_activities, 1);
        assert_eq!(stats.skipped_count, 0);

        updater.stop().await;
    }

    // ── add_activity_from_dict: event_type entries skipped ────────────────────

    #[tokio::test]
    async fn test_add_activity_from_dict_event_type_skipped() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "test-graph".to_string());
        updater.start();

        let data = json!({ "event_type": "SIMULATION_START", "round": 1 });
        updater.add_activity_from_dict(&data, "twitter");

        let stats = updater.get_stats().await;
        // event_type entries are skipped BEFORE add_activity, so no counter is bumped.
        assert_eq!(stats.total_activities, 0);
        assert_eq!(stats.skipped_count, 0);

        updater.stop().await;
    }

    // ── add_activity_from_dict: valid entry is enqueued ───────────────────────

    #[tokio::test]
    async fn test_add_activity_from_dict_valid_entry() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "test-graph".to_string());
        updater.start();

        let data = json!({
            "agent_id": 42,
            "agent_name": "Bob",
            "action_type": "LIKE_POST",
            "action_args": { "post_content": "hello" },
            "round": 3
        });
        updater.add_activity_from_dict(&data, "reddit");

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let stats = updater.get_stats().await;
        assert_eq!(stats.total_activities, 1);

        updater.stop().await;
    }

    // ── Batching at BATCH_SIZE=5 triggers a flush ─────────────────────────────

    #[tokio::test]
    async fn test_batch_flush_at_batch_size() {
        // LLM returns a simple entity for every entity-extraction call, empty relations.
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new(r#"[{"name": "Entity1", "kind": "Concept"}]"#, "[]"));
        let mut updater =
            GraphMemoryUpdater::new(Arc::clone(&graph), llm, "batch-test".to_string());
        updater.start();

        // Add exactly BATCH_SIZE activities on the same platform.
        for _ in 0..BATCH_SIZE {
            updater.add_activity(make_activity("twitter", "CREATE_POST"));
        }

        // Give the worker enough time to flush the batch.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let stats = updater.get_stats().await;
        assert_eq!(stats.total_activities, BATCH_SIZE as i64);
        assert_eq!(stats.batches_sent, 1, "exactly one batch should have been flushed");
        assert_eq!(stats.items_sent, BATCH_SIZE as i64);
        assert_eq!(stats.failed_count, 0);

        updater.stop().await;
    }

    // ── Per-platform independent batching ─────────────────────────────────────

    #[tokio::test]
    async fn test_per_platform_independent_batching() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new(r#"[{"name": "Entity1", "kind": "Concept"}]"#, "[]"));
        let mut updater =
            GraphMemoryUpdater::new(Arc::clone(&graph), llm, "platform-test".to_string());
        updater.start();

        // Add 5 twitter + 3 reddit activities.
        for _ in 0..BATCH_SIZE {
            updater.add_activity(make_activity("twitter", "CREATE_POST"));
        }
        for _ in 0..3 {
            updater.add_activity(make_activity("reddit", "CREATE_POST"));
        }

        // Give worker time to flush the twitter batch (reddit not yet at threshold).
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let stats = updater.get_stats().await;
        assert_eq!(stats.batches_sent, 1, "only twitter batch should be flushed");
        assert_eq!(stats.items_sent, BATCH_SIZE as i64);

        updater.stop().await;

        // After stop, reddit leftovers should have been flushed too.
        let stats_final = updater.get_stats().await;
        assert_eq!(stats_final.batches_sent, 2, "stop should flush reddit leftovers");
        assert_eq!(stats_final.items_sent, (BATCH_SIZE + 3) as i64);
    }

    // ── _flush_remaining: sub-BATCH_SIZE leftovers sent on stop ──────────────

    #[tokio::test]
    async fn test_flush_remaining_on_stop() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater =
            GraphMemoryUpdater::new(Arc::clone(&graph), llm, "flush-test".to_string());
        updater.start();

        // Add 3 activities (< BATCH_SIZE — no flush during running).
        for _ in 0..3 {
            updater.add_activity(make_activity("twitter", "CREATE_POST"));
        }

        // Stop triggers _flush_remaining.
        updater.stop().await;

        let stats = updater.get_stats().await;
        assert_eq!(stats.total_activities, 3);
        assert_eq!(stats.batches_sent, 1, "_flush_remaining must flush the sub-batch");
        assert_eq!(stats.items_sent, 3);
    }

    // ── combined_text join is observable ─────────────────────────────────────

    #[tokio::test]
    async fn test_combined_text_join() {
        // Use a mock LLM that captures the prompts.
        use std::sync::Mutex as StdMutex;

        struct CapturingLlm {
            prompts: Arc<StdMutex<Vec<String>>>,
        }

        #[async_trait]
        impl LlmClient for CapturingLlm {
            async fn complete(&self, prompt: &str) -> Result<String> {
                self.prompts.lock().unwrap().push(prompt.to_string());
                // Alternate: entity response then relation response.
                let count = self.prompts.lock().unwrap().len();
                if count % 2 == 1 {
                    Ok(r#"[{"name": "TestEntity", "kind": "Concept"}]"#.to_string())
                } else {
                    Ok("[]".to_string())
                }
            }
            async fn complete_json<T: serde::de::DeserializeOwned>(&self, p: &str) -> Result<T> {
                let t = self.complete(p).await?;
                serde_json::from_str(&t).map_err(|e| TeriError::Llm(e.to_string()))
            }
            async fn stream(
                &self,
                _: &str,
            ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
                Err(TeriError::Llm("not used".into()))
            }
            async fn chat_json<T: serde::de::DeserializeOwned>(
                &self,
                _: &[ChatMessage],
                _: &ChatOptions,
            ) -> Result<T> {
                Err(TeriError::Llm("not used".into()))
            }
        }

        let prompts = Arc::new(StdMutex::new(Vec::new()));
        let llm = Arc::new(CapturingLlm { prompts: Arc::clone(&prompts) });
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let mut updater = GraphMemoryUpdater::new(Arc::clone(&graph), llm, "join-test".to_string());
        updater.start();

        // Add 5 activities to trigger a flush.
        let activities = vec![
            make_activity("twitter", "CREATE_POST"),
            make_activity("twitter", "LIKE_POST"),
            make_activity("twitter", "FOLLOW"),
            make_activity("twitter", "REPOST"),
            make_activity("twitter", "MUTE"),
        ];
        // Build expected combined text.
        let expected_text: Vec<String> = activities.iter().map(|a| a.to_episode_text()).collect();
        let expected_combined = expected_text.join("\n");

        for a in activities {
            updater.add_activity(a);
        }

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        updater.stop().await;

        // The first prompt sent to the LLM should contain the combined text.
        let captured = prompts.lock().unwrap();
        assert!(!captured.is_empty(), "LLM must have received at least one prompt");
        assert!(
            captured[0].contains(&expected_combined),
            "first prompt must contain the combined episode text joined by \\n;\n\
             expected combined text:\n{expected_combined}\n\
             actual first prompt:\n{}",
            captured[0]
        );
    }

    // ── get_stats key set ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_get_stats_key_set() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "stats-test".to_string());
        updater.start();

        let stats = updater.get_stats().await;

        // Verify all contractual keys are present (observable JSON contract).
        let json = serde_json::to_value(&stats).expect("serialize stats");
        for key in &[
            "graph_id",
            "batch_size",
            "total_activities",
            "batches_sent",
            "items_sent",
            "failed_count",
            "skipped_count",
            "queue_size",
            "buffer_sizes",
            "running",
        ] {
            assert!(json.get(key).is_some(), "get_stats JSON must have key '{key}'");
        }
        assert_eq!(stats.graph_id, "stats-test");
        assert_eq!(stats.batch_size, BATCH_SIZE);
        assert!(stats.running);

        updater.stop().await;

        let stats_after = updater.get_stats().await;
        assert!(!stats_after.running, "running must be false after stop");
    }

    // ── start() is idempotent ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_start_idempotent() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "idempotent-test".to_string());

        updater.start();
        updater.start(); // Second call must be a no-op.

        let stats = updater.get_stats().await;
        assert!(stats.running);

        updater.stop().await;
    }

    // ── S-530 regression: buffer_sizes always contains twitter + reddit ───────
    //
    // MiroFish seeds `self._platform_buffers = {'twitter': [], 'reddit': []}` in
    // `__init__` (L252-255), so `get_stats()` at L463 ALWAYS returns at least
    // {"twitter": 0, "reddit": 0} — even before any activity has been received.
    // This test pins that contractual guarantee.

    /// Right after `new()` + `start()`, before any activity, both platform keys
    /// must be present with value 0.
    #[tokio::test]
    async fn test_buffer_sizes_seeded_twitter_reddit_at_start() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "seed-test".to_string());
        updater.start();

        let stats = updater.get_stats().await;

        assert!(
            stats.buffer_sizes.contains_key("twitter"),
            "buffer_sizes must contain 'twitter' key immediately after start(); got: {:?}",
            stats.buffer_sizes
        );
        assert!(
            stats.buffer_sizes.contains_key("reddit"),
            "buffer_sizes must contain 'reddit' key immediately after start(); got: {:?}",
            stats.buffer_sizes
        );
        assert_eq!(
            stats.buffer_sizes["twitter"], 0,
            "twitter buffer size must be 0 with no activities"
        );
        assert_eq!(
            stats.buffer_sizes["reddit"], 0,
            "reddit buffer size must be 0 with no activities"
        );

        updater.stop().await;
    }

    /// Only DO_NOTHING/event_type activities received — twitter and reddit still present at 0.
    #[tokio::test]
    async fn test_buffer_sizes_seeded_after_do_nothing_activities() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "do-nothing-seed-test".to_string());
        updater.start();

        // DO_NOTHING activities are skipped before enqueue, so the worker never sees them.
        updater.add_activity(make_activity("twitter", "DO_NOTHING"));
        updater.add_activity(make_activity("reddit", "DO_NOTHING"));

        // event_type activities are also skipped.
        let event_data = serde_json::json!({ "event_type": "SIMULATION_START", "round": 1 });
        updater.add_activity_from_dict(&event_data, "twitter");

        // Brief yield to let the worker settle (though nothing was enqueued).
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let stats = updater.get_stats().await;

        assert!(
            stats.buffer_sizes.contains_key("twitter"),
            "twitter key must be present even with only DO_NOTHING activities; got: {:?}",
            stats.buffer_sizes
        );
        assert!(
            stats.buffer_sizes.contains_key("reddit"),
            "reddit key must be present even with only DO_NOTHING activities; got: {:?}",
            stats.buffer_sizes
        );
        assert_eq!(stats.buffer_sizes["twitter"], 0);
        assert_eq!(stats.buffer_sizes["reddit"], 0);

        updater.stop().await;
    }

    /// A third-platform activity adds its key; twitter and reddit remain present.
    #[tokio::test]
    async fn test_buffer_sizes_third_platform_adds_key() {
        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new("[]", "[]"));
        let mut updater = GraphMemoryUpdater::new(graph, llm, "third-platform-test".to_string());
        updater.start();

        // Add one activity on a novel platform.
        updater.add_activity(make_activity("discord", "CREATE_POST"));

        // Give the worker time to receive and update the snapshot.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stats = updater.get_stats().await;

        // All three platform keys present.
        assert!(
            stats.buffer_sizes.contains_key("twitter"),
            "twitter must still be present after a discord activity; got: {:?}",
            stats.buffer_sizes
        );
        assert!(
            stats.buffer_sizes.contains_key("reddit"),
            "reddit must still be present after a discord activity; got: {:?}",
            stats.buffer_sizes
        );
        assert!(
            stats.buffer_sizes.contains_key("discord"),
            "discord key must appear after a discord activity; got: {:?}",
            stats.buffer_sizes
        );

        // discord has exactly 1 buffered activity (< BATCH_SIZE, so not flushed yet).
        assert_eq!(stats.buffer_sizes["discord"], 1);
        // twitter and reddit still at 0 (no activities sent on those platforms).
        assert_eq!(stats.buffer_sizes["twitter"], 0);
        assert_eq!(stats.buffer_sizes["reddit"], 0);

        updater.stop().await;
    }

    /// Workstream B (U6): with a vector index attached, a flushed batch re-embeds the accrued
    /// episode into the redb store under the graph's namespace — so the sim-accrued fact becomes
    /// searchable by cosine.
    #[tokio::test]
    async fn test_flushed_batch_reembeds_episode_vector() {
        use httpmock::prelude::*;
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/embeddings");
            then.status(200).header("Content-Type", "application/json").body(
                r#"{"object":"list","data":[{"object":"embedding","embedding":[1.0,0.0],"index":0}]}"#,
            );
        });
        let emb = std::sync::Arc::new(crate::embedding::EmbeddingClient::new(
            &crate::config::LlmConfig {
                base_url: server.base_url(),
                api_key: String::new(),
                model: "m".into(),
                embed_model: "e".into(),
                timeout_secs: 5,
                max_retries: 0,
                max_tokens: 2048,
                provider: crate::config::LlmProvider::Openai,
            },
        ));
        let tmp = tempfile::TempDir::new().unwrap();
        let store = std::sync::Arc::new(crate::memory::MemoryStore::new(tmp.path()).unwrap());
        let vector_index = Some(crate::services::graph_builder::GraphVectorIndex {
            embedder: emb,
            store: store.clone(),
        });

        let graph = Arc::new(Mutex::new(KnowledgeGraph::new()));
        let llm = Arc::new(MockLlm::new(r#"[{"name": "Carol", "kind": "Person"}]"#, "[]"));
        let graph_label = "episode-graph".to_string();
        let mut updater = GraphMemoryUpdater::new(Arc::clone(&graph), llm, graph_label.clone())
            .with_vector_index(vector_index);
        updater.start();

        for _ in 0..BATCH_SIZE {
            updater.add_activity(make_activity("twitter", "CREATE_POST"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        updater.stop().await;

        // The accrued episode must now be a searchable vector under the graph namespace.
        let ns = crate::services::graph_backend::graph_namespace(&graph_label);
        let stored = store.read_vec(ns, 100).await.unwrap();
        assert!(
            !stored.is_empty(),
            "flushed batch must re-embed at least one episode vector into the graph namespace"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GraphMemoryManager — Sub-cycle (c) port of `ZepGraphMemoryManager` (S-531..S-539)
// ─────────────────────────────────────────────────────────────────────────────
//
// Symbol mapping (S-531..S-539):
//
// | Source symbol (Python)                        | Lines   | teri target                                        |
// |-----------------------------------------------|---------|-----------------------------------------------------|
// | S-531 `ZepGraphMemoryManager` class           | 479     | `GraphMemoryManager<L>` struct (instance, not global)|
// | S-532 `ZepGraphMemoryManager._updaters`       | 486     | `updaters: tokio::sync::Mutex<HashMap<...>>`        |
// | S-533 `ZepGraphMemoryManager._lock`           | 487     | Folded into the Mutex — no separate observable      |
// | S-537 `ZepGraphMemoryManager._stop_all_done`  | 528     | `stop_all_done: AtomicBool`                         |
// | S-534 `ZepGraphMemoryManager.create_updater`  | 490-511 | `GraphMemoryManager::create_updater` (async)        |
// | S-535 `ZepGraphMemoryManager.get_updater`     | 514-516 | `GraphMemoryManager::get_updater` → `Option<UpdaterStats>` |
// | S-536 `ZepGraphMemoryManager.stop_updater`    | 519-525 | `GraphMemoryManager::stop_updater` (async)          |
// | S-538 `ZepGraphMemoryManager.stop_all`        | 531-546 | `GraphMemoryManager::stop_all` (async, idempotent)  |
// | S-539 `ZepGraphMemoryManager.get_all_stats`   | 549-554 | `GraphMemoryManager::get_all_stats` (async)         |
//
// Design decisions (map-onto, not downgrades):
//
// 1. CLASS → INSTANCE STRUCT: Python's class-level dict (`_updaters`) is a singleton within one
//    process. Rust cannot have global mutable statics over generic types (`L: LlmClient` is not
//    dyn-safe due to `complete_json<T>`/`chat_json<T>`). Mapped to an instance struct generic over
//    `L`, held in app state. Observable contract — ONE registry per process, keyed by
//    simulation_id, idempotent `stop_all` — is FULLY PRESERVED.
//
// 2. `_lock` FOLDS INTO `Mutex`: Python uses `threading.Lock` separately from the dict.
//    In Rust, `tokio::sync::Mutex<HashMap<...>>` fuses both. No separate observable is lost.
//
// 3. `tokio::sync::Mutex` for `updaters` (not `std::sync::Mutex`): `stop_updater`, `stop_all`,
//    `create_updater`, and `get_all_stats` all call `updater.stop()` or `updater.get_stats()`
//    which are `async`. Holding a `std::sync::MutexGuard` across `.await` is a deadlock hazard
//    and the compiler rejects it. `tokio::sync::Mutex` is the correct choice.
//
// 4. `stop_all_done` uses `AtomicBool` (not another Mutex): The idempotency check is a simple
//    boolean flag. In Python the check `if cls._stop_all_done: return` is outside the lock (L534).
//    An `AtomicBool` with `Relaxed` compare-exchange gives the same single-path semantics.
//    Note: Python does NOT check the flag under the lock; it checks before acquiring the lock.
//    We use `compare_exchange` with `AcqRel`/`Acquire` for correct cross-thread visibility.
//
// 5. `get_updater` return type → `Option<UpdaterStats>`: Returning `&GraphMemoryUpdater<L>`
//    through the async Mutex to the caller is impossible (the guard doesn't outlive the lock
//    scope). The Python callers use `get_updater` mainly to check existence and read stats;
//    `get_all_stats` is the primary read path. We return a stats snapshot, which is faithful
//    to the observable purpose (presence-check + stats). Returning `Option<()>` would lose stats;
//    returning a clone is impractical (GraphMemoryUpdater contains JoinHandle which isn't Clone).
//    `Option<UpdaterStats>` is the faithful, composable choice.
//
// 6. `create_updater` return type → `Result<(), TeriError>`: The Python returns the updater,
//    but callers interact with it via `get_updater`/`get_all_stats` thereafter. Returning the
//    updater from Rust would require either cloning (impossible — JoinHandle) or moving it out
//    of the Mutex (can't — other callers need it). Returning `()` is access-path-faithful and
//    documented here. The registry IS the access path.

use crate::error::TeriError;

/// Registry of per-simulation graph memory updaters.
///
/// Port of `ZepGraphMemoryManager` (S-531, L479-554, MiroFish).
///
/// Manages one `GraphMemoryUpdater<L>` per simulation, keyed by `simulation_id`.
/// Methods `create_updater`, `stop_updater`, `stop_all`, and `get_all_stats` are all
/// `async` because they call `GraphMemoryUpdater::stop`/`get_stats` which are async.
///
/// # Singleton → instance mapping (`[≠]`-class / map-onto)
///
/// Python's class-level singleton (`_updaters` class dict + `_lock` class lock) is
/// mapped to an instance struct (held in app state, e.g. `Arc<GraphMemoryManager<L>>`).
/// The observable contract — ONE registry per process, keyed by simulation_id, idempotent
/// `stop_all` — is **fully preserved**. This is a faithful map-onto, not a downgrade.
///
/// # `_lock` fold
///
/// Python's separate `threading.Lock` (`_lock`, S-533) has no separate observable beyond
/// mutual exclusion, which is provided by the `tokio::sync::Mutex` wrapping the HashMap.
/// It folds cleanly into the Mutex; nothing is dropped.
pub struct GraphMemoryManager<L: LlmClient + Send + Sync + 'static> {
    /// Map of simulation_id → updater.  Port of `_updaters` (S-532) + `_lock` (S-533).
    updaters: tokio::sync::Mutex<HashMap<String, GraphMemoryUpdater<L>>>,
    /// Workstream B (U6): optional vector index. When set, every updater this manager creates
    /// re-embeds the entities/edge facts accrued from a flushed activity batch into the redb
    /// store under the graph's namespace, so sim-accrued facts become searchable by cosine.
    /// `None` (the default) ⇒ no re-embed (keyword search still surfaces them — no-downgrade).
    vector_index: Option<crate::services::graph_builder::GraphVectorIndex>,
    /// Idempotency guard for `stop_all`. Port of `_stop_all_done` (S-537).
    ///
    /// Set to `true` on the first call to `stop_all`; subsequent calls return immediately.
    /// Python checks this flag BEFORE acquiring `_lock` (L534); we use `AtomicBool` with
    /// `compare_exchange(AcqRel/Acquire)` for the same semantics.
    stop_all_done: AtomicBool,
}

impl<L: LlmClient + Send + Sync + 'static> GraphMemoryManager<L> {
    /// Create a new, empty manager.
    pub fn new() -> Self {
        Self {
            updaters: tokio::sync::Mutex::new(HashMap::new()),
            stop_all_done: AtomicBool::new(false),
            vector_index: None,
        }
    }

    /// Workstream B (U6): construct a manager whose updaters re-embed sim-accrued graph facts
    /// into the vector store. `None` is equivalent to [`new`](Self::new).
    pub fn with_vector_index(
        vector_index: Option<crate::services::graph_builder::GraphVectorIndex>,
    ) -> Self {
        Self {
            updaters: tokio::sync::Mutex::new(HashMap::new()),
            stop_all_done: AtomicBool::new(false),
            vector_index,
        }
    }

    /// Create (or replace) the updater for `simulation_id`.
    ///
    /// Port of `ZepGraphMemoryManager.create_updater` (S-534, L490-511).
    ///
    /// If an updater already exists for `simulation_id`, it is **stopped first** (Python
    /// L503-504: `cls._updaters[simulation_id].stop()`), then replaced.  The new updater
    /// is constructed via `GraphMemoryUpdater::new(...)` and `.start()`-ed before insertion.
    ///
    /// # Return type decision
    ///
    /// Python returns the updater directly (L511).  In Rust, the updater lives inside the
    /// Mutex; returning a reference out of the guard is impossible, and `GraphMemoryUpdater<L>`
    /// is not `Clone` (contains `JoinHandle`).  Callers access the updater through
    /// `get_updater` / `get_all_stats`.  Returning `()` is the faithful composable choice.
    pub async fn create_updater(
        &self,
        simulation_id: &str,
        graph: Arc<tokio::sync::Mutex<KnowledgeGraph>>,
        llm: Arc<L>,
        graph_label: String,
    ) -> Result<(), TeriError> {
        let mut updaters = self.updaters.lock().await;

        // If one already exists, stop it first (Python L503-504).
        if let Some(old) = updaters.get_mut(simulation_id) {
            info!("停止旧图谱记忆更新器: simulation_id={}", simulation_id);
            old.stop().await;
        }

        let mut updater = GraphMemoryUpdater::new(graph, llm, graph_label)
            .with_vector_index(self.vector_index.clone());
        updater.start();
        updaters.insert(simulation_id.to_string(), updater);

        info!("创建图谱记忆更新器: simulation_id={}", simulation_id);
        Ok(())
    }

    /// Return a stats snapshot for `simulation_id`, or `None` if not registered.
    ///
    /// Port of `ZepGraphMemoryManager.get_updater` (S-535, L514-516).
    ///
    /// Python returns the `ZepGraphMemoryUpdater` instance directly (`cls._updaters.get(id)`).
    /// In Rust the updater lives behind a `tokio::sync::Mutex`; a `&` cannot be returned to
    /// the caller without holding the lock for the caller's entire use, which creates a
    /// deadlock footgun.  The observable purpose of `get_updater` is to check existence and
    /// read state; returning `Option<UpdaterStats>` is the faithful, composable equivalent.
    /// For callers that only need existence, `None` vs `Some(_)` maps directly.
    /// The primary bulk-read path is `get_all_stats`.
    pub async fn get_updater(&self, simulation_id: &str) -> Option<UpdaterStats> {
        let updaters = self.updaters.lock().await;
        if let Some(updater) = updaters.get(simulation_id) {
            // get_stats requires &self (not &mut); we hold an immutable reference through
            // the Mutex guard, so this is valid.
            Some(updater.get_stats().await)
        } else {
            None
        }
    }

    /// Stop and remove the updater for `simulation_id`.  No-op if absent.
    ///
    /// Port of `ZepGraphMemoryManager.stop_updater` (S-536, L519-525).
    ///
    /// Python: `if simulation_id in cls._updaters: stop(); del _updaters[id]`.
    /// Rust: `HashMap::remove` returns `Option`; stop if `Some`, skip if `None`.
    pub async fn stop_updater(&self, simulation_id: &str) {
        let mut updaters = self.updaters.lock().await;
        if let Some(mut updater) = updaters.remove(simulation_id) {
            updater.stop().await;
            info!("已停止图谱记忆更新器: simulation_id={}", simulation_id);
        }
    }

    /// Stop all updaters.  **Idempotent**: the second and subsequent calls are no-ops.
    ///
    /// Port of `ZepGraphMemoryManager.stop_all` (S-538, L531-546).
    ///
    /// Observable contract:
    /// - If already called once (`stop_all_done` is `true`), returns immediately without
    ///   acquiring the lock (Python L534-535: `if cls._stop_all_done: return`).
    /// - On the first call: sets the flag, stops each updater (catch-log-continue on error —
    ///   Python L541-544 `try/except`), then clears the map.
    ///
    /// This is the U-049 shutdown-handler entry point.
    pub async fn stop_all(&self) {
        // Idempotency check (outside the lock, matching Python L534 which also checks before
        // acquiring `_lock`). `compare_exchange` atomically flips false→true; if it was already
        // true, another goroutine/task has or is running stop_all — return immediately.
        if self
            .stop_all_done
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            // Already done — return immediately (Python L534-535).
            return;
        }

        let mut updaters = self.updaters.lock().await;

        for (simulation_id, mut updater) in updaters.drain() {
            // Per-updater error catch-log-continue (Python L541-544 try/except).
            // In Rust, `stop()` returns `()` (not `Result`), but panics are possible;
            // we use `catch_unwind` is not needed for async. The async `stop()` itself
            // logs errors internally and is designed to be non-panicking.
            // Use a short join timeout by forwarding to stop() which already has a 10s cap.
            info!("停止图谱记忆更新器: simulation_id={}", simulation_id);
            // stop() is non-panicking by design (errors are logged inside); call directly.
            updater.stop().await;
        }

        // Map is now drained (equivalent to Python's `cls._updaters.clear()`).
        info!("已停止所有图谱记忆更新器");
    }

    /// Fire one action dict into the live updater for `simulation_id`, if one is registered.
    ///
    /// This is the manager-side analog of MiroFish's monitor path
    /// (`simulation_runner.py:_read_action_log` L583-684):
    /// ```python
    /// graph_updater = ZepGraphMemoryManager.get_updater(state.simulation_id)
    /// ...
    /// if graph_updater:
    ///     graph_updater.add_activity_from_dict(action_data, platform)
    /// ```
    /// MiroFish gets the updater instance out of the registry and calls `add_activity_from_dict`
    /// on it. In teri the updater lives behind the manager's `Mutex` (a `&` cannot escape the
    /// guard — see [`get_updater`]'s return-type note), so the call is performed *through* the
    /// manager while the guard is held. This is sound because
    /// [`GraphMemoryUpdater::add_activity_from_dict`] takes `&self` and never blocks (it is a
    /// non-blocking mpsc `send` to the worker — see [`GraphMemoryUpdater::add_activity`]); no
    /// `.await` happens under the guard beyond the lock acquisition itself.
    ///
    /// No-op (silently) when no updater is registered for `simulation_id` — faithful to Python
    /// where `get_updater` returns `None` and the `if graph_updater:` guard skips the call.
    /// The `event_type`-skip and field-default logic live in `add_activity_from_dict` (U-021,
    /// S-525), so this method forwards the raw parsed JSON dict unchanged.
    pub async fn fire_activity_from_dict(
        &self,
        simulation_id: &str,
        data: &serde_json::Value,
        platform: &str,
    ) {
        let updaters = self.updaters.lock().await;
        if let Some(updater) = updaters.get(simulation_id) {
            updater.add_activity_from_dict(data, platform);
        }
    }

    /// Return stats for every registered updater.
    ///
    /// Port of `ZepGraphMemoryManager.get_all_stats` (S-539, L549-554).
    ///
    /// Python: `{sim_id: updater.get_stats() for sim_id, updater in cls._updaters.items()}`.
    /// Rust: collect a `HashMap<String, UpdaterStats>` by calling `get_stats()` (async) on
    /// each updater while holding the Mutex (single-writer, no concurrent mutation concern).
    pub async fn get_all_stats(&self) -> HashMap<String, UpdaterStats> {
        let updaters = self.updaters.lock().await;
        let mut result = HashMap::with_capacity(updaters.len());
        for (sim_id, updater) in updaters.iter() {
            result.insert(sim_id.clone(), updater.get_stats().await);
        }
        result
    }
}

impl<L: LlmClient + Send + Sync + 'static> Default for GraphMemoryManager<L> {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests for GraphMemoryManager (sub-cycle c)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod manager_tests {
    use super::*;
    use crate::error::{Result, TeriError};
    use crate::graph::KnowledgeGraph;
    use crate::llm::{ChatMessage, ChatOptions, LlmClient};
    use async_trait::async_trait;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    // ── Reuse MockLlm from updater_tests (redefined locally for module isolation) ──

    struct MockLlm;

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, _prompt: &str) -> Result<String> {
            Ok("[]".to_string())
        }
        async fn complete_json<T: serde::de::DeserializeOwned>(&self, p: &str) -> Result<T> {
            let t = self.complete(p).await?;
            serde_json::from_str(&t).map_err(|e| TeriError::Llm(e.to_string()))
        }
        async fn stream(
            &self,
            _: &str,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<String>> + Send>>> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat(&self, _: &[ChatMessage], _: &ChatOptions) -> Result<String> {
            Err(TeriError::Llm("not used".into()))
        }
        async fn chat_json<T: serde::de::DeserializeOwned>(
            &self,
            _: &[ChatMessage],
            _: &ChatOptions,
        ) -> Result<T> {
            Err(TeriError::Llm("not used".into()))
        }
    }

    fn make_graph() -> Arc<TokioMutex<KnowledgeGraph>> {
        Arc::new(TokioMutex::new(KnowledgeGraph::new()))
    }

    fn make_llm() -> Arc<MockLlm> {
        Arc::new(MockLlm)
    }

    // ── create_updater registers and starts ───────────────────────────────────

    #[tokio::test]
    async fn test_create_updater_registers() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        manager
            .create_updater("sim-1", make_graph(), make_llm(), "graph-1".to_string())
            .await
            .expect("create_updater must succeed");

        // get_updater returns Some (updater is registered and running).
        let stats = manager.get_updater("sim-1").await;
        assert!(stats.is_some(), "updater must be present after create_updater");
        assert!(stats.unwrap().running, "updater must be running after create_updater");
    }

    // ── create_updater on an existing id stops + replaces the old ────────────

    #[tokio::test]
    async fn test_create_updater_replaces_existing() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        // Create first updater.
        manager
            .create_updater("sim-1", make_graph(), make_llm(), "graph-a".to_string())
            .await
            .expect("first create must succeed");

        // Add an activity so we can distinguish the new updater's stats.
        {
            let updaters = manager.updaters.lock().await;
            let updater = updaters.get("sim-1").unwrap();
            updater.add_activity(AgentActivity {
                platform: "twitter".to_string(),
                agent_id: 1,
                agent_name: "A".to_string(),
                action_type: "CREATE_POST".to_string(),
                action_args: serde_json::Map::new(),
                round_num: 1,
                timestamp: "2024-01-01T00:00:00Z".to_string(),
            });
        }

        // Create second updater with same id — must stop old, start new.
        manager
            .create_updater("sim-1", make_graph(), make_llm(), "graph-b".to_string())
            .await
            .expect("second create must succeed");

        // The new updater has graph_id = "graph-b" and zero total_activities.
        let stats = manager.get_updater("sim-1").await.expect("must still be present");
        assert_eq!(stats.graph_id, "graph-b", "new updater must have the new graph_label");
        assert_eq!(stats.total_activities, 0, "new updater starts with zero activities");
        assert!(stats.running, "new updater must be running");
    }

    // ── get_all_stats returns one entry per registered updater ────────────────

    #[tokio::test]
    async fn test_get_all_stats_returns_all_entries() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        manager
            .create_updater("sim-a", make_graph(), make_llm(), "graph-a".to_string())
            .await
            .unwrap();
        manager
            .create_updater("sim-b", make_graph(), make_llm(), "graph-b".to_string())
            .await
            .unwrap();

        let all_stats = manager.get_all_stats().await;

        assert_eq!(all_stats.len(), 2, "get_all_stats must return one entry per updater");
        assert!(all_stats.contains_key("sim-a"), "must contain sim-a");
        assert!(all_stats.contains_key("sim-b"), "must contain sim-b");
        assert_eq!(all_stats["sim-a"].graph_id, "graph-a");
        assert_eq!(all_stats["sim-b"].graph_id, "graph-b");

        // Cleanup.
        manager.stop_all().await;
    }

    // ── stop_updater removes the updater ─────────────────────────────────────

    #[tokio::test]
    async fn test_stop_updater_removes() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        manager
            .create_updater("sim-1", make_graph(), make_llm(), "graph-1".to_string())
            .await
            .unwrap();

        // Verify registered.
        assert!(manager.get_updater("sim-1").await.is_some());

        // Stop and remove.
        manager.stop_updater("sim-1").await;

        // Now absent.
        assert!(
            manager.get_updater("sim-1").await.is_none(),
            "updater must be absent after stop_updater"
        );
    }

    // ── stop_updater absent is a no-op ────────────────────────────────────────

    #[tokio::test]
    async fn test_stop_updater_absent_is_noop() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        // Must not panic or error on an unregistered simulation_id.
        manager.stop_updater("nonexistent").await;

        // Registry still empty.
        let all_stats = manager.get_all_stats().await;
        assert!(all_stats.is_empty(), "manager must remain empty after noop stop_updater");
    }

    // ── stop_all is idempotent ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_stop_all_idempotent() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        manager
            .create_updater("sim-1", make_graph(), make_llm(), "graph-1".to_string())
            .await
            .unwrap();
        manager
            .create_updater("sim-2", make_graph(), make_llm(), "graph-2".to_string())
            .await
            .unwrap();

        // First call stops both and clears the map.
        manager.stop_all().await;

        assert!(
            manager.get_all_stats().await.is_empty(),
            "after stop_all, registry must be empty"
        );

        // Second call must be a no-op (does not panic, does not double-stop anything).
        manager.stop_all().await;

        // Registry still empty.
        assert!(
            manager.get_all_stats().await.is_empty(),
            "stop_all is idempotent: registry remains empty on second call"
        );
    }

    // ── stop_all clears the registry ─────────────────────────────────────────

    #[tokio::test]
    async fn test_stop_all_clears_registry() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        for i in 0..3 {
            manager
                .create_updater(&format!("sim-{i}"), make_graph(), make_llm(), format!("graph-{i}"))
                .await
                .unwrap();
        }

        assert_eq!(manager.get_all_stats().await.len(), 3);

        manager.stop_all().await;

        assert!(
            manager.get_all_stats().await.is_empty(),
            "stop_all must clear the entire registry"
        );
    }

    // ── get_updater absent returns None ───────────────────────────────────────

    #[tokio::test]
    async fn test_get_updater_absent_returns_none() {
        let manager = GraphMemoryManager::<MockLlm>::new();

        assert!(
            manager.get_updater("not-registered").await.is_none(),
            "get_updater must return None for unregistered simulation_id"
        );
    }

    // ── Default trait ─────────────────────────────────────────────────────────

    #[test]
    fn test_default_is_empty() {
        let manager = GraphMemoryManager::<MockLlm>::default();
        // Default manager is empty; we check synchronously via the AtomicBool.
        assert!(
            !manager.stop_all_done.load(Ordering::Relaxed),
            "fresh manager must have stop_all_done=false"
        );
    }
}
