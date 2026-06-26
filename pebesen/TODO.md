# TODO

Phases follow the demand validation sequence (Stage 0 → 5).
Feature priority labels: `[MUST]` `[SHOULD]` `[NICE]`
Progress states: `[ ]` not started · `[~]` in progress · `[x]` done

**Business model is locked. See README.md and ARCHITECTURE.md before modifying any schema task.**

---

## Pre-Phase: Repository and Tooling Bootstrap ✅

**Status: COMPLETED**

### P.1 Rust Workspace ✅
- [x] Root `Cargo.toml` with workspace members
  - [x] `pebesen-api`, `pebesen-core`, `pebesen-db`, `pebesen-search`, `pebesen-notifications`, `pebesen-bin`
- [x] Shared workspace dependencies
- [x] `rustfmt.toml`, `clippy.toml`
- [x] `cargo build` succeeds on empty crates
- [x] `cargo install cargo-audit` — added to Makefile setup target

### P.2 Frontend ✅
- [x] SvelteKit + TypeScript + ESLint + Prettier
- [x] pnpm workspace root
- [x] `@sveltejs/adapter-node`, `tailwindcss`, `svelte-check`
- [x] `pnpm dev` starts without errors

### P.3 Database Bootstrap ✅
- [x] `docker-compose.yml` with postgres, redis, meilisearch, caddy, app (commented)
- [x] `.env.example` with all variables
- [x] Migration `0001_extensions.sql`: uuid-ossp, pg_trgm
- [x] **AMEND**: add `vector` extension to `0001_extensions.sql` — pgvector required from day one
- [x] `sqlx migrate run` succeeds

### P.4 CI Pipeline ✅
- [x] `.github/workflows/ci.yml`: rust, frontend, docker jobs
- [x] All CI jobs pass

### P.5 Developer Experience ✅
- [x] Root `Makefile` with dev, migrate, test, lint, reindex targets
- [x] `CONTRIBUTING.md`, `.gitignore`

---

## Phase 0 — MVP: Core Architecture Validation

**Goal:** A working instance where a single community can post and read messages in topic-organized streams, with full-history search, contribution tracking accumulating from day one, and no presence indicators.

**Gate to Phase 1:** 3–5 real communities actively using daily for 30 consecutive days. At least one community migrated from an existing platform. Contribution data accumulating cleanly.

---

### 0.0 Schema Foundation Amendments `[MUST]`

> These must complete before any other Phase 0 migration is written.
> The full schema is defined in ARCHITECTURE.md. Migrations implement it.

- [x] Amend `0001_extensions.sql`: add `CREATE EXTENSION IF NOT EXISTS "vector"`
- [x] Confirm `pgvector` available in postgres service in `docker-compose.yml`
  - [x] Use `pgvector/pgvector:pg16` image instead of `postgres:16-alpine`
- [x] Add `pebesen-intelligence` crate stub to workspace (empty, Phase 2 implementation)
- [x] Add `CONTRIBUTOR_PAYOUT_FRACTION` (default `0.20`) to `.env.example`
- [x] Add `INTELLIGENCE_API_ENABLED` (default `false`) to `.env.example`

---

### 0.0.1 Cold Start Plan `[MUST]`

> The first 10 communities determine what Pebesen becomes in the market's perception.
> AGPL and self-hosting help but do not solve cold start. This must be planned before Phase 0 gate.

- [ ] Identify 10 specific target communities by name — not segment descriptions, actual communities
  - [ ] Criteria: active on Zulip/Discord/Slack, publicly expressing friction, OSS or research, ≥20 active contributors
  - [ ] Document: current platform, documented pain, decision-maker contact
- [ ] Define outreach approach per community — not mass email, direct conversation with maintainers
- [ ] Define success condition: what does "adopted" mean? (daily use for 30 days, not just signup)
- [ ] Gate: at least 3 communities committed before Phase 0 MVP is declared ready
- [ ] Document cold start plan in `docs/cold-start.md` (internal, not public)

---

### 0.1 Identity and Auth `[MUST]`

#### 0.1.1 Database Migrations

- [x] Migration `0002_users.sql` — `users` table with settings JSONB
- [x] Migration `0003_spaces.sql` — add `tier TEXT DEFAULT 'community'` column
- [x] Migration `0004_memberships.sql`

#### 0.1.2 Core Domain Types (`pebesen-core`)

- [x] `User`, `Space`, `Membership`, `Role` structs
- [x] `AuthClaims` JWT payload struct
- [x] `AppError` enum implementing `IntoResponse`
- [ ] Add `SpaceTier` enum: `Community`, `Standard`, `Scale`
- [ ] Add `ContributionType` enum matching ARCHITECTURE.md

#### 0.1.3 DB Queries (`pebesen-db`)

- [x] `users::insert`, `find_by_email`, `find_by_id`, `find_by_username`
- [x] `spaces::insert`, `find_by_slug`
- [x] `memberships::insert`, `find`, `list_by_space`

#### 0.1.4 Auth Handlers (`pebesen-api`)

- [x] `POST /auth/register` — validate, hash Argon2id, insert, return 201
- [x] `POST /auth/login` — verify, issue JWT + refresh token
- [x] `POST /auth/refresh` — rotate refresh token
- [x] `POST /auth/logout` — delete refresh token, clear cookie

#### 0.1.5 Auth Middleware

- [x] `AuthUser` extractor (FromRequestParts)
- [x] `OptionalAuthUser` extractor

#### 0.1.6 Rate Limiting

- [x] `tower_governor` on auth endpoints — 10 req/min/IP

---

### 0.2 Spaces `[MUST]`

- [x] `POST /spaces` — create with slug + name, creator becomes Owner
- [x] `GET /spaces/:slug` — public or member-auth
- [x] `POST /spaces/:slug/join` — public spaces only
- [x] `GET /spaces/:slug/members` — cursor paginated
- [x] `SpaceDTO`, `MembershipDTO`

---

### 0.3 Streams `[MUST]`

- [x] Migration `0005_streams.sql`
- [x] `streams::insert`, `find_by_id`, `list_by_space`
- [x] `POST /spaces/:slug/streams` — admin/owner only
- [x] `GET /spaces/:slug/streams`
- [x] `PATCH /spaces/:slug/streams/:id`

---

### 0.4 Topics `[MUST]`

- [x] Migration `0006_topics.sql` — includes `summary`, `summary_rendered` columns from day one
- [x] `topics::insert`, `find_by_id`, `list_by_stream`, `search_by_name_prefix`, `update_last_active`, `set_status`, `rename`
- [x] `POST /streams/:id/topics`
- [x] `GET /streams/:id/topics` — status filter + cursor pagination
- [x] `PATCH /topics/:id` — rename
- [x] `PATCH /topics/:id/status`
- [x] `GET /streams/:id/topics?q=:prefix` — autocomplete < 50ms

---

### 0.5 Messages `[MUST]`

#### 0.5.1 Migration

- [x] Migration `0007_messages.sql`
- [ ] **AMEND**: add `embedding vector(384)` column to messages table
- [ ] **AMEND**: add IVFFlat index (conditional on row count — see ARCHITECTURE.md)

#### 0.5.2 Markdown Rendering

- [x] `pulldown-cmark` in `pebesen-core`
- [x] `render_markdown` with sanitization and link hardening

#### 0.5.3 DB Queries

- [x] `messages::insert` (calls `topics::update_last_active`)
- [x] `messages::get_page` (cursor, author join, exclude soft-deleted)
- [x] `messages::find_by_id`
- [x] `messages::update_content`
- [x] `messages::soft_delete`

#### 0.5.4 Handlers

- [x] `POST /topics/:id/messages`
- [x] `GET /topics/:id/messages`
- [x] `PATCH /messages/:id`
- [x] `DELETE /messages/:id`

---

### 0.6 Contribution Primitive `[MUST]`

> This is new in the re-architecture. Must complete before Phase 0 gate.
> Contribution data must accumulate from the first real message sent.
> No revenue is distributed in Phase 0 — but the data must exist.

#### 0.6.1 Migrations

- [ ] Migration `0008_expertise_domains.sql`
  - [ ] `expertise_domains` table (id, slug, name, parent)
  - [ ] Seed: 20 initial domains covering OSS, research, medicine, law, engineering, education
- [ ] Migration `0009_topic_domains.sql`
  - [ ] `topic_domains` table (topic_id, domain_id, tagged_by, tagged_at)
- [ ] Migration `0010_contributions.sql`
  - [ ] `contributions` table — see ARCHITECTURE.md for full spec
  - [ ] Indexes on user_id+space_id, space_id+created_at, domain_id
- [ ] Migration `0011_contributor_stats.sql`
  - [ ] `contributor_stats` materialized view
  - [ ] Unique index for fast upsert on refresh

#### 0.6.2 Core Types

- [ ] `ExpertiseDomain`, `TopicDomain`, `Contribution`, `ContributionType` structs in `pebesen-core`
- [ ] `ContributionWeight` constants matching ARCHITECTURE.md weight table
- [ ] `contributor_stats::refresh(pool)` — `REFRESH MATERIALIZED VIEW CONCURRENTLY`

#### 0.6.3 DB Queries (`pebesen-db`)

- [ ] `contributions::record(pool, user_id, space_id, type, reference_id, domain_id) -> Contribution`
  - [ ] Weight auto-assigned by type using `ContributionWeight` constants
- [ ] `contributions::list_by_user_space(pool, user_id, space_id, since) -> Vec<Contribution>`
- [ ] `contributor_stats::get_by_space(pool, space_id, limit) -> Vec<ContributorStat>`
- [ ] `contributor_stats::get_by_user_space(pool, user_id, space_id) -> Option<ContributorStat>`
- [ ] `expertise_domains::list(pool) -> Vec<ExpertiseDomain>`
- [ ] `topic_domains::insert(pool, topic_id, domain_id, user_id)`
- [ ] `topic_domains::list_by_topic(pool, topic_id) -> Vec<ExpertiseDomain>`

#### 0.6.4 Integration Points

Wire contribution recording into existing handlers:

- [ ] `POST /topics/:id/messages` → `contributions::record(type=message)`
- [ ] `POST /streams/:id/topics` → `contributions::record(type=topic_open)`
- [ ] `PATCH /topics/:id/status` to `resolved` → `contributions::record(type=topic_resolve, user=resolver)`
- [ ] `PATCH /topics/:id/summary` → `contributions::record(type=summary)`
- [ ] Moderation actions (delete, ban) → `contributions::record(type=moderation, user=moderator)`

#### 0.6.5 Scheduled Jobs

- [ ] `pebesen-bin/refresh_stats.rs` — runs every 6h via cron or tokio interval
  - [ ] Calls `REFRESH MATERIALIZED VIEW CONCURRENTLY contributor_stats`
  - [ ] Logs: duration, rows affected
- [ ] Add `make refresh-stats` to Makefile

#### 0.6.6 Domain Tagging API

- [ ] `POST /topics/:id/domains` — add domain tag to topic
  - [ ] Require auth + space membership
  - [ ] Max 5 domain tags per topic
  - [ ] Return `201` with updated domain list
- [ ] `DELETE /topics/:id/domains/:domain_id` — require admin/owner or tagger
- [ ] `GET /topics/:id/domains` — public if topic is public

---

### 0.7 WebSocket Real-Time `[MUST]`

#### 0.7.1 Connection Lifecycle

- [x] `GET /ws` — JWT auth, ConnectionState, DashMap
- [x] `read_loop`: subscribe, unsubscribe, catch_up, pong
- [x] `write_loop`: Redis subscriber + mpsc

#### 0.7.2 Redis Pub/Sub

- [x] Channel: `space:{space_id}`
- [x] Publish on: message CRUD, topic CRUD/status, read position
- [ ] Add event type: `contribution_recorded` — payload: `{user_id, type, space_id}` — used in Phase 2 for real-time weight display. Publish now, consume in Phase 2.

#### 0.7.3 Heartbeat

- [ ] Server ping every 30s, 10s response window, close on timeout

#### 0.7.4 Reconnection Protocol

- [ ] `catch_up` frame → missed messages per topic since given message id
- [ ] Batch response, limit 200 messages per topic

---

### 0.8 Per-Topic Read State `[MUST]`

- [ ] Migration `0012_read_positions.sql`
- [ ] `read_positions::upsert` — only update if message is newer
- [ ] `read_positions::get_unread_counts` — single query, exclude muted + deleted
- [ ] `read_positions::set_muted`
- [ ] `POST /topics/:id/read`
- [ ] `GET /spaces/:slug/unread` — Redis cached 30s TTL
- [ ] `POST /topics/:id/mute`, `DELETE /topics/:id/mute`

---

### 0.9 No Presence by Default `[MUST]`

- [ ] `PATCH /users/me/settings` — allowlisted keys only
- [ ] `POST /users/me/presence` — no-op if `show_presence != true`
- [ ] `GET /users/:id/presence` — both parties must opt in

---

### 0.10 Search `[MUST]`

#### 0.10.1 Meilisearch Index

- [ ] Create `messages` index on startup (idempotent)
- [ ] Include `domain_ids` as filterable attribute
- [ ] `SearchDocument` struct includes `domain_ids`, `contribution_weight`

#### 0.10.2 Async Indexer

- [ ] Batch up to 100 tasks or 500ms
- [ ] 3-retry exponential backoff
- [ ] Task variants: Add, Update, Delete

#### 0.10.3 Embedding Pipeline

- [ ] Add `fastembed` to `pebesen-search`
- [ ] `embed_message(content: &str) -> Vec<f32>` using `all-MiniLM-L6-v2`
- [ ] Async embed task: fires after message insert, updates `messages.embedding`
- [ ] On failure: log, retry once, skip (embedding is not blocking for message delivery)

#### 0.10.4 Reindex Binary

- [ ] `pebesen-bin/reindex.rs` — streams all messages, batches to Meilisearch
- [ ] Separate `pebesen-bin/reembed.rs` — backfills null embeddings

#### 0.10.5 Search Handler

- [ ] `GET /spaces/:slug/search` — q, stream_id, topic_id, author_id, domain_id, before, after
- [ ] Always inject `space_id` filter — no cross-space leakage
- [ ] Return hits with match highlights

---

### 0.11 Notifications `[MUST]`

- [ ] Migration `0013_notifications.sql`
- [ ] `GET /notifications`, `POST /notifications/read`
- [ ] Welcome digest on space join: top 5 active open topics per stream

---

### 0.12 Public Read Access `[MUST]`

- [ ] Audit all GET endpoints for correct `OptionalAuthUser` usage
- [ ] Integration tests: unauthenticated public, unauthenticated private → 403

---

### 0.13 Frontend: Phase 0 Views `[MUST]`

#### 0.13.1 Routing Structure

- [ ] Define SvelteKit route tree
  - [ ] `/(auth)/login`
  - [ ] `/(auth)/register`
  - [ ] `/(app)/+layout.svelte` — global WS connection, auth guard
  - [ ] `/(app)/s/[slug]/+layout.svelte` — space shell
  - [ ] `/(app)/s/[slug]/+page.svelte` — space home
  - [ ] `/(app)/s/[slug]/[stream_id]/+page.svelte` — topic list
  - [ ] `/(app)/s/[slug]/[stream_id]/[topic_id]/+page.svelte` — message feed
  - [ ] `/(app)/settings/+page.svelte`
  - [ ] `/(public)/s/[slug]/+page.svelte` — public read-only

#### 0.13.2 Global State Stores

- [ ] `stores/identity.ts` — current user, cleared on logout
- [ ] `stores/connection.ts` — WS state machine: `idle | connecting | connected | reconnecting | failed`
  - [ ] Auto-reconnect with exponential backoff (max 30s)
  - [ ] On reconnect: send `catch_up` frame
- [ ] `stores/spaces.ts` — joined spaces list, active space
- [ ] `stores/unread.ts` — `Map<topic_id, count>`, updated by WS events + mark-read
  - [ ] Derived: `totalUnread` — sum across all spaces
- [ ] `stores/contributions.ts` — own contribution weight per space, read-only Phase 0

#### 0.13.3 Auth Pages

- [ ] `/login`
  - [ ] Email + password fields
  - [ ] Submit → `POST /auth/login` → store access token in memory only (not localStorage)
  - [ ] On success: redirect to last visited space or `/`
  - [ ] Inline error on `401`
- [ ] `/register`
  - [ ] Username, email, password, confirm-password fields
  - [ ] Client-side validation before submit
  - [ ] Inline error on `409` — specify whether email or username is taken

#### 0.13.4 Space Shell Layout

- [ ] Left sidebar (240px, collapsible on mobile)
  - [ ] Space name + settings icon
  - [ ] Stream list: name + unread badge (sum of topic unreads in stream)
  - [ ] Active stream highlighted
  - [ ] "Create stream" button (admin/owner only)
  - [ ] Bottom: own contribution weight (numeric only, no framing), user avatar, username, settings link
- [ ] Main content: `<slot />` outlet
- [ ] Connection status bar (visible only when `connecting` or `reconnecting`)

#### 0.13.5 Topic List View

- [ ] List topics ordered by `last_active DESC`
- [ ] Each row: topic name, status badge, unread count, relative timestamp, domain tags
- [ ] Filter tabs: `Open` | `Resolved` | `All`
- [ ] "New topic" inline input: submit creates + navigates
- [ ] Empty state: "No open topics. Start the first one."
- [ ] Catch-up banner: "N unread topics" → opens `CatchUpQueue`

#### 0.13.6 CatchUpQueue Component

- [ ] Triggered by banner or `U` keyboard shortcut
- [ ] Ordered: mentions first, then `last_active DESC`
- [ ] Each item: stream name, topic name, unread count
- [ ] Keyboard: `↑/↓` navigate, `Enter` open, `S` skip (mark read), `Esc` close

#### 0.13.7 Message Feed View

- [ ] Topic header: name, status badge, status dropdown (any member), message count, domain tags
- [ ] Virtualized message list
  - [ ] Each: avatar, display name, timestamp, rendered HTML
  - [ ] Edited indicator: "(edited)" in muted text
  - [ ] Deleted: grey italic "This message was deleted"
  - [ ] Own messages: hover reveals edit + delete icons
- [ ] Auto-scroll to bottom on initial load
- [ ] Auto-scroll on new message only if already at bottom
- [ ] "Jump to bottom" button when scrolled up 200px+
- [ ] Mark read: triggered when topic open + user at bottom for 2 seconds
- [ ] Explicit "Mark as read" button in header

#### 0.13.8 Compose Box

- [ ] Topic selector: required, dropdown with autocomplete
  - [ ] Pre-filled from current topic view
  - [ ] Block send if no topic selected — show "Select a topic first"
- [ ] Textarea: Markdown input, auto-grows to 6 rows max
  - [ ] `Enter` sends, `Shift+Enter` newlines
  - [ ] `Ctrl+B` bold, `Ctrl+I` italic, `Ctrl+K` link
- [ ] Markdown toolbar: Bold, Italic, Code, Link, Lists
- [ ] Draft autosave: `sessionStorage` key `draft:{space_id}:{topic_id}`, debounced 500ms
- [ ] Restore draft on mount, clear on send

#### 0.13.9 Search UI

- [ ] `Ctrl+K` or `/` focuses search bar
- [ ] Debounce 300ms → `GET /spaces/:slug/search?q=`
- [ ] Results panel: grouped by topic, snippet with match highlighted, timestamp
- [ ] Filter: domain tag, stream, author, date range
- [ ] "No results" empty state

#### 0.13.10 Settings Page

- [ ] Display name edit
- [ ] Password change (current + new + confirm)
- [ ] Presence toggle: off by default, explanatory text
- [ ] Notification default: immediate / digest / muted
- [ ] Save → `PATCH /users/me/settings` — success/error toast

#### 0.13.11 Contribution Display (Phase 0 — read-only)

- [ ] Space sidebar footer: own contribution weight for this space as plain number
- [ ] Tooltip on hover: breakdown by type (messages, topics opened, summaries, moderation)
- [ ] No badges, levels, streaks, rankings, or progress bars anywhere in Phase 0 UI
- [ ] Code review gate: any gamification element is a blocking PR rejection

---

## Phase 1 — Retention and Identity

**Goal:** Reduce churn from multi-community fatigue, no lurker UX, and no neurodivergent accommodation. Begin self-sovereign credential infrastructure.

**Gate to Phase 2:** Communities report one of: (a) migrated knowledge from old platform, (b) onboarded member active 14+ days, (c) user active across 3+ communities. Contribution data passes quality check: no weight anomalies, distribution looks reasonable.

---

### 1.1 Verified Expertise — Self-Sovereign `[MUST]`

> Elevated from Phase 2 NICE. This is the primary moat. Build it before revenue distribution.

#### 1.1.1 Migrations

- [ ] Migration `0014_oauth_identities.sql`
- [ ] Migration `0015_verified_credentials.sql` — see ARCHITECTURE.md
  - [ ] `verification_type`: license_number, institution_email, orcid, github_org, manual_review
  - [ ] `status`: pending, verified, revoked
  - [ ] Verification data encrypted at app layer before storage

#### 1.1.2 Verification Flows

- [ ] `institution_email`: user submits institutional email → platform sends verification link → auto-verified on click
- [ ] `orcid`: OAuth2 with ORCID → fetch public profile → auto-verify researcher domains
- [ ] `github_org`: GitHub OAuth → verify org membership → auto-verify relevant tech domains
- [ ] `license_number`: user submits license number + jurisdiction → enters manual review queue
- [ ] `manual_review`: admin reviews queue — UI in space admin panel

#### 1.1.3 API

- [ ] `POST /users/me/credentials` — submit verification request
- [ ] `GET /users/me/credentials` — own credentials list
- [ ] `GET /users/:id/credentials?space_id=` — public verified badges for user in domain context
- [ ] `GET /admin/credentials/queue` — admin only, manual review queue
- [ ] `PATCH /admin/credentials/:id` — approve/revoke

#### 1.1.4 Credential → Contribution Domain Linkage

- [ ] When credential verified → backfill domain_id on recent contributions where domain was null and topic domain matches
- [ ] Verified contributor gets `weight_multiplier = 1.2` on `message` contributions in verified domain (configurable)

#### 1.1.5 Frontend

- [ ] Settings > Credentials — submit verification, view status
- [ ] Verified badge display on member profile within relevant domains only
- [ ] No cross-domain badge bleed (a verified doctor has no badge in a Rust programming space)

---

### 1.2 Unified Inbox `[SHOULD]`

- [ ] `GET /inbox` — unread mentions + keyword alerts across all spaces
- [ ] `/(app)/inbox/` route with global unread badge
- [ ] Filter: All | Mentions | Keywords | Replies
- [ ] Keyboard: `G I`

---

### 1.3 OAuth2 Login `[SHOULD]`

- [ ] Migration `0014_oauth_identities.sql` (same migration as 1.1.1)
- [ ] GitHub OAuth2: `/auth/github` + `/auth/github/callback`
- [ ] Google OAuth2: same pattern
- [ ] Settings > Account: connected accounts, unlink

---

### 1.4 Topic Summary Card `[SHOULD]`

- [ ] `PATCH /topics/:id/summary` — any member, max 1000 chars, renders Markdown
- [ ] Wire `contributions::record(type=summary)` on save
- [ ] Frontend: collapsible card above message feed, inline editor with preview

---

### 1.5 Notification Scheduling `[SHOULD]`

- [ ] `notification_preferences` + `keyword_alerts` tables
- [ ] Digest cron job per user schedule — email with grouped unread
- [ ] Settings > Notifications — global default, available hours, digest schedule, keyword alerts

---

### 1.6 Anonymous Reactions `[SHOULD]`

- [ ] Migration `message_reactions` (message_id, user_id, emoji, PRIMARY KEY on all three)
- [ ] `POST/DELETE /messages/:id/reactions`
- [ ] Wire into `contributions::record(type=reaction_net)` — daily aggregation job
  - [ ] Aggregation: for each author in each space, sum net reactions received that day, write single contribution record
- [ ] Frontend: reaction bar, emoji picker, real-time WS updates
- [ ] Space setting: `anonymous_reactions` — counts only, no individual visibility

---

### 1.7 Accessibility — Neurodivergent Baseline `[SHOULD]`

- [ ] High-contrast mode (CSS custom property overrides, `prefers-contrast: more`)
- [ ] Reduced-motion mode (all transitions in `@media (prefers-reduced-motion: reduce)`)
- [ ] Full keyboard navigation (Tab order, focus trap in modals)
- [ ] `?` shortcut map overlay
- [ ] ARIA: landmark roles, aria-label on icon buttons, aria-live on unread badges
- [ ] Screen reader test with VoiceOver — document results

---

### 1.8 Focus Mode + Read Mode `[SHOULD]`

- [ ] Focus (`F`): hides sidebar + unread badges, page title = topic name, sessionStorage
- [ ] Read (`R`): hides compose + hover actions, font 16px, line-height 1.8, sessionStorage

---

### 1.9 Data Export `[SHOULD]`

- [ ] `POST /spaces/:slug/export` → `202` with job_id
- [ ] Export job: JSON (ndjson per stream) + Markdown (one .md per topic) → zip
- [ ] `GET /spaces/:slug/export/:job_id` — poll
- [ ] Download link, 24h TTL, delete after download

---

### 1.10 Member Tenure Signal `[SHOULD]`

- [ ] `GET /spaces/:slug/members/:user_id` — joined_at, message count per stream, total
- [ ] Member profile card: "Member since {date}", top contribution domains
- [ ] No rank, score, XP, level, streak, or bar chart — enforced in code review gate

---

### 1.11 Draft Autosave `[SHOULD]`

- [ ] Key: `draft:{space_id}:{topic_id}` in sessionStorage (not localStorage)
- [ ] Debounced 500ms, clear on send, restore on mount

---

### 1.12 Mobile Web — Responsive Hardening `[SHOULD]`

- [ ] Audit all Phase 0+1 views at 375px, 390px, 428px
- [ ] Sidebar full-screen overlay on mobile, hamburger trigger
- [ ] iOS visualViewport keyboard handling for compose box
- [ ] All touch targets ≥ 44px
- [ ] PWA manifest + service worker (app shell + static asset cache)
- [ ] Test: iOS Safari + Android Chrome install

### 1.13 Moderation Quality Signal `[MUST]`

> Prevents the Stack Overflow hostile gatekeeping trap.
> Weight rewards moderation quantity by default. This task adds quality discrimination.

- [ ] Define moderation action taxonomy in `pebesen-core`:
  - [ ] `constructive`: ban spam, resolve conflict, approve member, document decision → generates weight
  - [ ] `structural`: archive topic, merge duplicate, rename stream → generates weight
  - [ ] `gatekeeping`: close as duplicate without comment, remove without reason → does NOT generate weight
- [ ] Enforce taxonomy at application layer: `contributions::record(type=moderation)` requires `subtype` param
- [ ] Add `moderation_subtype TEXT` column to `contributions` table (nullable, only for moderation type)
- [ ] Migration `0018_moderation_subtype.sql`
- [ ] Update contribution weight constants: constructive=3.0, structural=2.0, gatekeeping=0.0
- [ ] Admin dashboard: moderation action breakdown per moderator — makes patterns visible without punishing

---

### 1.14 Payout Legibility Validation `[MUST]`

> Quora's failure included opaque payouts contributors couldn't explain.
> Before Phase 2 launch, the model must pass a human-readability test.

- [ ] Write `docs/contributor-payouts.md` — plain language explanation
  - [ ] What counts as a contribution (by type, in plain English)
  - [ ] How weight is computed (table, no formulas in the intro)
  - [ ] How payout is calculated (worked example with real numbers)
  - [ ] How to opt out
  - [ ] What happens to opted-out share
- [ ] User test: show the doc to 3 non-technical moderators from Phase 0 communities
  - [ ] Ask: "Can you estimate roughly what you'd earn if your space subscribed at $X/month?"
  - [ ] Gate: all 3 can answer correctly without rereading — else revise doc and retest
- [ ] Contribution breakdown tooltip in UI (0.13.11) must match doc language exactly
- [ ] Legal review checkpoint: payout structure reviewed before any real money moves

**Goal:** Activate all four revenue streams. Contributor payouts go live. B2B intelligence API launches. Verified expertise subscription tier opens.

**Gate to Phase 3:** At least one community reports "we couldn't go back — our knowledge is here." At least one B2B intelligence subscriber. Payout infrastructure tested end-to-end with real amounts.

---

### 2.1 Subscription Billing `[MUST]`

- [ ] Integrate payment provider (Stripe or equivalent)
- [ ] Migration `0016_revenue_events.sql`
- [ ] Webhook handler: payment confirmed → `revenue_events::insert`
- [ ] Space tier upgrade/downgrade flow
- [ ] Frontend: Settings > Space > Billing

---

### 2.2 Contributor Revenue Sharing `[MUST]`

- [ ] Migration `0017_contributor_payouts.sql`
- [ ] Attribution job (runs at period end):
  - [ ] Reads `contributor_stats` for space + period
  - [ ] Computes payout per contributor: `(contributor_weight / total_weight) × (space_revenue × payout_fraction)`
  - [ ] Writes `contributor_payouts` rows
- [ ] Payout processing: Stripe Connect (or equivalent)
- [ ] Contributor opt-out: `receive_payouts = false` in user settings
  - [ ] Opt-out share routes to space owner, not silently redistributed
- [ ] `GET /users/me/payouts` — payout history with period breakdown
- [ ] `GET /spaces/:slug/payouts/summary` — owner/admin: total distributed, top recipients (anonymized below 5)
- [ ] Frontend: Settings > Payouts — connect bank, payout history, opt-out toggle

---

### 2.3 B2B Intelligence API `[MUST]`

> Separate crate: `pebesen-intelligence`. Separate API surface.

- [ ] API key management: `POST /v1/intelligence/keys` — space-owner only
- [ ] Space opt-in: `PATCH /spaces/:slug/settings` — `intelligence_api_enabled: bool`
- [ ] Implement endpoints (see ARCHITECTURE.md for full spec):
  - [ ] `GET /v1/intelligence/spaces/:slug/domains`
  - [ ] `GET /v1/intelligence/spaces/:slug/domains/:domain/signal`
  - [ ] `GET /v1/intelligence/spaces/:slug/experts`
  - [ ] `GET /v1/intelligence/spaces/:slug/topics/trending`
- [ ] Privacy constraints enforced at query layer:
  - [ ] Counts anonymized below threshold 5
  - [ ] Individual data only if `intelligence_opt_in = true`
  - [ ] No raw message content in any endpoint
- [ ] Rate limiting: 1000 req/day on free tier, configurable per key
- [ ] Usage logging for billing
- [ ] Frontend: Settings > Space > Intelligence API — enable toggle, key management, usage dashboard

---

### 2.4 Verified Expertise Subscription Tier `[MUST]`

- [ ] Subscription product: professionals pay monthly for active verification maintenance
- [ ] Free tier: one domain verification, standard weight multiplier
- [ ] Paid tier: multiple domain verifications, enhanced weight multiplier, priority manual review
- [ ] Billing integration for credential subscriptions

---

### 2.5 Expertise Map `[SHOULD]`

- [ ] `GET /spaces/:slug/experts?domain=` — top contributors per domain by `contributor_stats.total_weight`
- [ ] `GET /topics/:id/suggest-experts` — up to 3 experts in topic's domains
- [ ] Frontend: `/s/:slug/experts` — domain breakdown, contributor names, no ranking numbers
- [ ] "Ask an expert" CTA in topic header

---

### 2.6 Cross-Community Semantic Search `[SHOULD]`

- [ ] `GET /search/similar?topic_id=:id&limit=10`
  - [ ] Embed topic's first message → nearest via `<=>` pgvector operator
  - [ ] Filter to user's spaces only — no cross-space content leak
- [ ] "Similar discussions" panel in topic view

---

### 2.7 SEO-Indexable Public Streams `[SHOULD]`

- [ ] SvelteKit SSR for all `/(public)/` routes
- [ ] Server-side: title, description, canonical, og:* tags
- [ ] `robots.txt`: allow public spaces, disallow private + auth
- [ ] `GET /s/:slug/sitemap.xml` — paginated, max 50,000 URLs

---

### 2.8 MCP Connector `[SHOULD]`

> Elevated from Phase 3 NICE. This is a B2B revenue surface.

- [ ] Implement MCP server in `pebesen-bin/mcp.rs` (or `crates/pebesen-mcp`)
- [ ] Tools: `post_message`, `list_topics`, `search_messages`, `get_topic_summary`, `get_unread_topics`, `get_domain_experts`
- [ ] Auth: bot API keys (same namespace as webhook bots)
- [ ] Publish to MCP registry with README and example prompts
- [ ] Optional `mcp` service in `docker-compose.yml` (disabled by default)
- [ ] Commercial: MCP access part of Standard/Scale tier

---

### 2.9 E2E Encrypted DMs `[SHOULD]`

- [ ] `dm_keys` (user_id PK, public_key TEXT, created_at)
- [ ] `direct_messages` (id, sender_id, recipient_id, ciphertext, nonce, created_at)
- [ ] Key generation: X25519 keypair client-side, private key in sessionStorage only
- [ ] Encryption: ECDH shared secret → XChaCha20-Poly1305
- [ ] DMs not indexed in Meilisearch, not embedded, not in intelligence API
- [ ] Warning banner: "Messages are end-to-end encrypted. Pebesen cannot read them."

---

### 2.10 AI Topic Summaries `[NICE]`

- [ ] Background job: topics with 20+ messages, stale/missing summary, every 15 min
- [ ] Provider: `AI_PROVIDER=anthropic|ollama|none` (default `none`)
- [ ] Store in `topics.ai_summary`, manual summary always takes precedence
- [ ] Frontend: "AI-generated" label in muted text
- [ ] Space setting: `ai_summaries_enabled` — defaults to false

---

### 2.11 Self-Hosting Documentation `[MUST]`

- [ ] `docs/self-hosting.md`: prerequisites, step-by-step, SMTP, custom domain + TLS, troubleshooting
- [ ] `docs/upgrade.md`: pull, compose, rollback
- [ ] `docs/backup.md`: pg_dump, volume backup, schedule
- [ ] `docs/contributor-payouts.md`: how weights are computed, how payouts are calculated, opt-out instructions
- [ ] Publish to GHCR via CI on tag: `ghcr.io/SHA888/pebesen:latest` + semver
- [ ] Test full install on clean Ubuntu 24.04 LTS VPS (1GB RAM)

---

### 2.12 Prediction Ingestion — Schema Reservation `[SHOULD]`

> Schema slot reserved in Phase 2 so Phase 3 implementation requires no breaking migration.
> Implementation activates only when Teri has live deployments generating real signal.
> See ARCHITECTURE.md `space_predictions` table and Intelligence API endpoints.

- [ ] Migration `0019_space_predictions.sql` — create `space_predictions` table (see ARCHITECTURE.md)
- [ ] `POST /v1/intelligence/spaces/:slug/predictions` — receive Teri write-back
  - [ ] Require space-scoped intelligence API key
  - [ ] Validate payload shape: type, confidence, horizon_days, generated_at, expires_at
  - [ ] Insert into `space_predictions`
  - [ ] Return `{ prediction_id }`
- [ ] `POST /v1/intelligence/spaces/:slug/predictions/:id/action` — moderator marks actioned
  - [ ] Sets `actioned_at = NOW()`
  - [ ] This timestamp is the calibration signal fed back to Teri
- [ ] Frontend: space admin panel — pending predictions list (topic signals, contributor trajectories, health risks)
  - [ ] Each prediction: type, confidence, horizon, dismiss or mark actioned
  - [ ] Hidden by default until space owner enables `teri_integration` in settings

---

## Phase 3 — Network Effects

**Goal:** Cross-community discovery and developer ecosystem creating compounding value.

**Gate:** None — continuous. These are growth levers, not survival features.

---

### 3.1 Public Community Directory `[NICE]`

- [ ] `listed BOOLEAN DEFAULT FALSE` on spaces
- [ ] `GET /explore` — public, filters: sort by activity/members/created, language
- [ ] Frontend: `/explore` — space cards with domain tags

---

### 3.2 Bot / Webhook API `[NICE]`

- [ ] `bot_users` (id, space_id, name, api_key hashed, created_by)
- [ ] `webhooks` (id, space_id, stream_id, topic_id nullable, url, secret, events TEXT[])
- [ ] `POST /spaces/:slug/bots` — admin only, return plaintext key once
- [ ] Bot posts via `Authorization: Bot <api_key>`, 60 msg/min
- [ ] Outbound webhook: HMAC-SHA256 signed POST on new message
- [ ] Frontend: Settings > Space > Integrations

---

### 3.3 Smart Topic Name Suggestion `[NICE]`

- [ ] Client-side: extract first 80 chars, strip stop words, fuzzy match via Fuse.js
- [ ] "Did you mean: {topic_name}?" chip in compose box

---

### 3.4 Federation Research `[NICE]`

- [ ] Open GitHub Discussion: "Federation protocol evaluation"
- [ ] Decision gate: no implementation until 3+ community operators formally request it

---

## Permanent Backlog — Excluded by Design

| Feature | Reason |
|---|---|
| Gamification (XP, streaks, levels, leaderboards) | Directly harms neurodivergent users. Validated. |
| Algorithmic feed / recommended content | Contradicts async, low-pressure model |
| Ephemeral / disappearing messages in channels | Destroys institutional memory |
| NFT / token-gating | Zero validated demand in target segments |
| Built-in project management (tasks, boards) | Scope creep. Use webhook/bot integration |
| Public follower graph | Social pressure, not community coherence |
| Read receipts visible to sender | Documented anxiety harm. Always off. |
| Algorithmic notification ranking | Removes user control |
| Admin-awarded vanity badges | Replaced by self-sovereign credential system |
| Advertising of any kind | Structurally incompatible with the model |
| Per-seat pricing | Structurally incompatible with lurker-first design |

---

## Open Questions — Resolved

| Question | Decision | Rationale |
|---|---|---|
| Pricing model | Space-based flat fee tiers, not seat-based | Seat-based punishes lurker communities |
| Contributor payment mechanism | Stripe Connect (Phase 2) | Widest jurisdiction coverage |
| Payout fraction default | 20% of space revenue | Calibrate against Phase 0 community data |
| Mobile native app | Block until mobile web DAU/MAU > 40% | Measure in Phase 1 |
| Message length cap | Block until median measured in Phase 0 | Don't cap before data |
| Federation | Block on Phase 3 evaluation | Don't implement until 3+ operators request |
| Moderation tooling | Planning spike in Phase 1 planning | Required before public community launch |
| Intelligence API privacy threshold | 5 contributors minimum for any aggregate | Conservative; recalibrate in Phase 2 |
