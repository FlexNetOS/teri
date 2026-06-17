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
        self.action_args
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
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
        let quote_content = if !quote_content_raw.is_empty() {
            quote_content_raw
        } else {
            self.arg("content")
        };

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
        let query = if !query_raw.is_empty() {
            query_raw
        } else {
            self.arg("keyword")
        };
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
        let query = if !query_raw.is_empty() {
            query_raw
        } else {
            self.arg("username")
        };
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
        assert!(
            text.starts_with("Alice: "),
            "expected 'Alice: ' prefix, got: {text:?}"
        );
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
        assert_eq!(
            a.to_episode_text(),
            "Alice: 发布了一条帖子：「大家好」"
        );
    }

    #[test]
    fn test_create_post_no_content() {
        let a = activity("CREATE_POST", json!({}));
        assert_eq!(a.to_episode_text(), "Alice: 发布了一条帖子");
    }

    // ── LIKE_POST (4 branches) ────────────────────────────────────────────────

    #[test]
    fn test_like_post_content_and_author() {
        let a = activity(
            "LIKE_POST",
            json!({"post_content": "好文章", "post_author_name": "Bob"}),
        );
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
        let a = activity(
            "DISLIKE_POST",
            json!({"post_content": "差文章", "post_author_name": "Bob"}),
        );
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
        assert_eq!(
            a.to_episode_text(),
            "Alice: 引用了Carol的帖子「原文」，并评论道：「我的评论」"
        );
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
        let a = activity(
            "QUOTE_POST",
            json!({"content": "备用评论"}),
        );
        assert_eq!(
            a.to_episode_text(),
            "Alice: 引用了一条帖子，并评论道：「备用评论」"
        );
    }

    /// quote_content present (non-empty) → takes precedence over content.
    #[test]
    fn test_quote_post_or_fallback_quote_content_wins() {
        let a = activity(
            "QUOTE_POST",
            json!({"quote_content": "优先评论", "content": "备用评论"}),
        );
        assert_eq!(
            a.to_episode_text(),
            "Alice: 引用了一条帖子，并评论道：「优先评论」"
        );
    }

    /// quote_content is empty string → falls back to content (Python falsy or-chain).
    #[test]
    fn test_quote_post_or_fallback_empty_quote_content_uses_content() {
        let a = activity(
            "QUOTE_POST",
            json!({"quote_content": "", "content": "备用评论"}),
        );
        assert_eq!(
            a.to_episode_text(),
            "Alice: 引用了一条帖子，并评论道：「备用评论」"
        );
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
        assert_eq!(
            a.to_episode_text(),
            "Alice: 在Eve的帖子「原帖」下评论道：「我的看法」"
        );
    }

    #[test]
    fn test_create_comment_content_post_content_only() {
        let a = activity(
            "CREATE_COMMENT",
            json!({"content": "我的看法", "post_content": "原帖"}),
        );
        assert_eq!(
            a.to_episode_text(),
            "Alice: 在帖子「原帖」下评论道：「我的看法」"
        );
    }

    #[test]
    fn test_create_comment_content_post_author_only() {
        let a = activity(
            "CREATE_COMMENT",
            json!({"content": "我的看法", "post_author_name": "Eve"}),
        );
        assert_eq!(
            a.to_episode_text(),
            "Alice: 在Eve的帖子下评论道：「我的看法」"
        );
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
