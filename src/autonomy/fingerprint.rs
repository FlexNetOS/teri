//! [`SignalFingerprint`] — a cheap, stable fingerprint of a [`CommunitySignal`]'s
//! *salient* fields, used by the orchestrator's debounce to decide whether a domain's
//! signal has *meaningfully* changed since it was last sensed.
//!
//! ## What counts as "changed"
//!
//! Two signals are considered the **same** (debounced — no new prediction run) when ALL
//! of these match:
//!
//! * `contributor_count`
//! * `topic_count`
//! * `active_topic_count`
//! * the **identity + lifecycle status** of each `recent_topic` (the `(id, status)` pairs,
//!   in order) — a topic opening/closing or a *new* hot topic appearing is a real delta.
//!
//! Deliberately **excluded** from the fingerprint:
//!
//! * `captured_at` — every poll has a fresh timestamp; including it would defeat debounce
//!   (every tick would look "changed"). Recency is carried in the signal itself, not the
//!   change-detection key.
//! * `recent_topics[*].last_active` / `created_at` — these tick on every poll for any live
//!   topic, so they are too noisy to gate a (costly) prediction run on. The *salient*
//!   momentum delta we react to is the set of live topics and their open/closed state, not
//!   sub-poll activity jitter. (TASK-AUTO-2's calibration loop is where finer recency
//!   weighting belongs; the DECIDE-layer debounce stays coarse on purpose.)
//!
//! The fingerprint is a stable string so it round-trips through the persisted
//! [`crate::autonomy::OrchestratorState`] and survives a process restart (continuity).

use crate::seed::community::CommunitySignal;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// A stable, cheap fingerprint of a [`CommunitySignal`]'s salient fields.
///
/// Equality is the debounce predicate: an incoming signal whose fingerprint equals the
/// last-seen fingerprint for a domain is treated as *unchanged* and skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalFingerprint(String);

impl SignalFingerprint {
    /// Compute the fingerprint of a signal from its salient fields (see module docs).
    pub fn of(signal: &CommunitySignal) -> Self {
        let mut s = String::new();
        // Counts — the headline momentum numbers.
        let _ = write!(
            s,
            "c={};t={};a={}",
            signal.contributor_count, signal.topic_count, signal.active_topic_count
        );
        // Recent-topic identity + lifecycle status, order-preserving. A new hot topic, or a
        // topic flipping open↔closed↔archived, is a meaningful delta worth predicting on.
        s.push_str(";topics=[");
        for t in &signal.recent_topics {
            // `id` is opaque and stable; `status` is the lifecycle we react to. Field
            // separators (`|`, `,`) keep distinct topic lists from colliding.
            let _ = write!(s, "{}|{},", t.id, t.status);
        }
        s.push(']');
        SignalFingerprint(s)
    }

    /// The stable string form (for logging / persistence inspection).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::community::CommunityTopic;
    use chrono::{Duration, Utc};

    fn topic(id: &str, status: &str) -> CommunityTopic {
        CommunityTopic {
            id: id.to_string(),
            stream_id: "s1".to_string(),
            name: format!("topic {id}"),
            status: status.to_string(),
            created_at: None,
            last_active: Some(Utc::now()),
        }
    }

    fn signal(
        contributors: u64,
        topics: u64,
        active: u64,
        recent: Vec<CommunityTopic>,
    ) -> CommunitySignal {
        CommunitySignal {
            domain_id: "d1".to_string(),
            domain_slug: "dom".to_string(),
            contributor_count: contributors,
            topic_count: topics,
            active_topic_count: active,
            recent_topics: recent,
            captured_at: Utc::now(),
        }
    }

    #[test]
    fn captured_at_is_excluded_from_the_fingerprint() {
        let mut a = signal(2, 5, 3, vec![topic("t1", "open")]);
        let mut b = a.clone();
        // Same salient fields, different capture time → SAME fingerprint (debounce works).
        a.captured_at = Utc::now();
        b.captured_at = Utc::now() + Duration::hours(1);
        assert_eq!(SignalFingerprint::of(&a), SignalFingerprint::of(&b));
    }

    #[test]
    fn last_active_jitter_is_excluded() {
        let a = signal(2, 5, 3, vec![topic("t1", "open")]);
        let mut b = a.clone();
        // Only the recent topic's last_active moved — sub-poll jitter, not a real delta.
        b.recent_topics[0].last_active = Some(Utc::now() + Duration::minutes(5));
        assert_eq!(SignalFingerprint::of(&a), SignalFingerprint::of(&b));
    }

    #[test]
    fn count_change_changes_the_fingerprint() {
        let a = signal(2, 5, 3, vec![topic("t1", "open")]);
        let b = signal(3, 5, 3, vec![topic("t1", "open")]); // contributor_count +1
        assert_ne!(SignalFingerprint::of(&a), SignalFingerprint::of(&b));

        let c = signal(2, 6, 3, vec![topic("t1", "open")]); // topic_count +1
        assert_ne!(SignalFingerprint::of(&a), SignalFingerprint::of(&c));

        let d = signal(2, 5, 4, vec![topic("t1", "open")]); // active_topic_count +1
        assert_ne!(SignalFingerprint::of(&a), SignalFingerprint::of(&d));
    }

    #[test]
    fn topic_status_flip_changes_the_fingerprint() {
        let a = signal(2, 5, 3, vec![topic("t1", "open")]);
        let b = signal(2, 5, 3, vec![topic("t1", "closed")]);
        assert_ne!(SignalFingerprint::of(&a), SignalFingerprint::of(&b));
    }

    #[test]
    fn new_topic_changes_the_fingerprint() {
        let a = signal(2, 5, 3, vec![topic("t1", "open")]);
        let b = signal(2, 5, 3, vec![topic("t1", "open"), topic("t2", "open")]);
        assert_ne!(SignalFingerprint::of(&a), SignalFingerprint::of(&b));
    }

    #[test]
    fn identical_salient_fields_match() {
        let a = signal(2, 5, 3, vec![topic("t1", "open"), topic("t2", "closed")]);
        let b = signal(2, 5, 3, vec![topic("t1", "open"), topic("t2", "closed")]);
        assert_eq!(SignalFingerprint::of(&a), SignalFingerprint::of(&b));
    }
}
