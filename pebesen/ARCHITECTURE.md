# Architecture

## Design Philosophy

Three principles that inform every architectural decision:

1. **Structure at input, not at retrieval.** Forcing topic discipline when a message is written eliminates the need to reconstruct context later. This is a fundamentally different tradeoff than post-hoc tagging, search, or AI summarization.

2. **Value attribution must be continuous, not retroactive.** Contribution tracking accumulates from day one. Revenue distribution computed against historical contribution data requires that data to exist. You cannot backfill fairness.

3. **The knowledge graph is the moat.** The structured relationship between spaces, streams, topics, messages, authors, expertise domains, and contribution weight is what makes Pebesen irreplaceable. Every schema decision either strengthens or weakens this graph.

---

## System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                        Clients                              │
│          SvelteKit SPA          Public SSR (SEO)            │
└──────────────────────┬──────────────────────────────────────┘
                       │ HTTP + WebSocket
┌──────────────────────▼──────────────────────────────────────┐
│                    pebesen-api (Axum)                        │
│   REST handlers · WS upgrade · Auth middleware · Rate limit │
└──────┬───────────┬───────────┬──────────────┬───────────────┘
       │           │           │              │
┌──────▼──┐  ┌─────▼───┐  ┌───▼────┐  ┌──────▼──────┐
│pebesen- │  │pebesen- │  │pebesen-│  │pebesen-     │
│core     │  │db       │  │search  │  │notifications│
│Domain   │  │sqlx     │  │Meili + │  │Email +      │
│types,   │  │queries, │  │pgvector│  │digest jobs  │
│business │  │migrations│  │        │  │             │
│logic    │  │         │  │        │  │             │
└─────────┘  └────┬────┘  └───┬────┘  └─────────────┘
                  │            │
       ┌──────────▼──┐  ┌──────▼──────────┐
       │ PostgreSQL  │  │  Meilisearch    │
       │ + pgvector  │  │  (full-text)    │
       └─────────────┘  └─────────────────┘
                  │
       ┌──────────▼──┐
       │    Redis    │
       │ PubSub +    │
       │ Session +   │
       │ Cache       │
       └─────────────┘
```

---

## Crate Structure

```
pebesen/
├── crates/
│   ├── pebesen-api/          # Axum handlers, middleware, WebSocket
│   ├── pebesen-core/         # Domain types, business logic, no I/O
│   ├── pebesen-db/           # sqlx queries, migrations
│   ├── pebesen-search/       # Meilisearch client, indexer, vector ops
│   ├── pebesen-notifications/# Email, digest scheduler
│   ├── pebesen-intelligence/ # B2B intelligence API (Phase 2)
│   └── pebesen-bin/          # CLI binaries: reindex, backfill, export
├── frontend/                 # SvelteKit + TypeScript + TailwindCSS
├── migrations/               # sqlx migrations, numbered, ordered
└── docker-compose.yml
```

---

## Database Schema

Schema is presented in migration order. Every table that will exist in Phase 3 is designed now, even if populated in later phases. No retroactive schema breaks.

### Core Primitives (Phase 0)

```sql
-- 0001: Extensions
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";
CREATE EXTENSION IF NOT EXISTS "vector";        -- pgvector: from day one

-- 0002: Users
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username        TEXT NOT NULL,
    display_name    TEXT NOT NULL,
    email           TEXT NOT NULL,
    password_hash   TEXT NOT NULL,
    settings        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_users_email    ON users(lower(email));
CREATE UNIQUE INDEX idx_users_username ON users(lower(username));

-- 0003: Spaces
CREATE TABLE spaces (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    slug        TEXT NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    visibility  TEXT NOT NULL CHECK (visibility IN ('public', 'private')),
    tier        TEXT NOT NULL DEFAULT 'community'
                    CHECK (tier IN ('community', 'standard', 'scale')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX idx_spaces_slug ON spaces(lower(slug));

-- 0004: Memberships
CREATE TABLE memberships (
    user_id     UUID NOT NULL REFERENCES users(id),
    space_id    UUID NOT NULL REFERENCES spaces(id),
    role        TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'guest')),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, space_id)
);
CREATE INDEX idx_memberships_space ON memberships(space_id);

-- 0005: Streams
CREATE TABLE streams (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    space_id    UUID NOT NULL REFERENCES spaces(id),
    name        TEXT NOT NULL,
    description TEXT,
    visibility  TEXT NOT NULL DEFAULT 'public',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (space_id, lower(name))
);
CREATE INDEX idx_streams_space ON streams(space_id);

-- 0006: Topics
CREATE TABLE topics (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    stream_id   UUID NOT NULL REFERENCES streams(id),
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open'
                    CHECK (status IN ('open', 'resolved', 'archived')),
    summary     TEXT,
    summary_rendered TEXT,
    created_by  UUID NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_active TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (stream_id, lower(name))
);
CREATE INDEX idx_topics_stream_active ON topics(stream_id, last_active DESC);
CREATE INDEX idx_topics_stream_status ON topics(stream_id, status);

-- 0007: Messages
CREATE TABLE messages (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    topic_id    UUID NOT NULL REFERENCES topics(id),
    author_id   UUID NOT NULL REFERENCES users(id),
    content     TEXT NOT NULL CHECK (length(content) > 0),
    rendered    TEXT,
    embedding   vector(384),              -- populated async after insert
    edited_at   TIMESTAMPTZ,
    deleted_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_messages_topic_time ON messages(topic_id, created_at);
CREATE INDEX idx_messages_author     ON messages(author_id);
CREATE INDEX idx_messages_embedding  ON messages
    USING ivfflat (embedding vector_cosine_ops)  -- built after 1000+ rows
    WITH (lists = 100);
```

### Contribution Primitive (Phase 0 — schema only, populated from day one)

```sql
-- 0008: Expertise Domains
-- Controlled vocabulary. Seeded at startup. Admin-extensible.
CREATE TABLE expertise_domains (
    id      UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    slug    TEXT NOT NULL UNIQUE,
    name    TEXT NOT NULL,
    parent  UUID REFERENCES expertise_domains(id)   -- for domain hierarchies
);

-- 0009: Topic Domain Tags
CREATE TABLE topic_domains (
    topic_id    UUID NOT NULL REFERENCES topics(id),
    domain_id   UUID NOT NULL REFERENCES expertise_domains(id),
    tagged_by   UUID NOT NULL REFERENCES users(id),
    tagged_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (topic_id, domain_id)
);

-- 0010: Contributions
-- Every meaningful platform action is a contribution record.
-- This is the primitive from which revenue attribution, expertise
-- signals, and the B2B intelligence API are all derived.
CREATE TABLE contributions (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id         UUID NOT NULL REFERENCES users(id),
    space_id        UUID NOT NULL REFERENCES spaces(id),
    domain_id       UUID REFERENCES expertise_domains(id),
    type            TEXT NOT NULL CHECK (type IN (
                        'message',          -- authored a message
                        'topic_open',       -- opened a topic
                        'topic_resolve',    -- marked a topic resolved
                        'moderation',       -- moderation action taken
                        'summary',          -- wrote a topic summary
                        'reaction_net'      -- net positive reactions received (aggregated daily)
                    )),
    reference_id    UUID,                  -- nullable: message_id, topic_id, etc.
    weight          NUMERIC(10,4) NOT NULL DEFAULT 1.0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_contributions_user_space  ON contributions(user_id, space_id);
CREATE INDEX idx_contributions_space_time  ON contributions(space_id, created_at DESC);
CREATE INDEX idx_contributions_domain      ON contributions(domain_id) WHERE domain_id IS NOT NULL;

-- 0011: Contributor Stats (materialized, refreshed every 6h)
-- Pre-aggregated for API performance. Source of truth is contributions table.
CREATE MATERIALIZED VIEW contributor_stats AS
SELECT
    user_id,
    space_id,
    domain_id,
    COUNT(*)                                        AS total_contributions,
    SUM(weight)                                     AS total_weight,
    MAX(created_at)                                 AS last_active,
    COUNT(*) FILTER (WHERE type = 'message')        AS message_count,
    COUNT(*) FILTER (WHERE type = 'moderation')     AS moderation_count
FROM contributions
GROUP BY user_id, space_id, domain_id;

CREATE UNIQUE INDEX idx_contributor_stats_pk
    ON contributor_stats(user_id, space_id, COALESCE(domain_id, uuid_nil()));
```

### Read State (Phase 0)

```sql
-- 0012: Read Positions
CREATE TABLE read_positions (
    user_id             UUID NOT NULL REFERENCES users(id),
    topic_id            UUID NOT NULL REFERENCES topics(id),
    last_read_message_id UUID,
    last_read_at        TIMESTAMPTZ,
    muted               BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (user_id, topic_id)
);
CREATE INDEX idx_read_positions_user ON read_positions(user_id);

-- 0013: Notifications
CREATE TABLE notifications (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id     UUID NOT NULL REFERENCES users(id),
    type        TEXT NOT NULL,
    payload     JSONB NOT NULL,
    read_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_notifications_user_unread
    ON notifications(user_id) WHERE read_at IS NULL;
```

### Identity Layer (Phase 1)

```sql
-- 0014: OAuth Identities
CREATE TABLE oauth_identities (
    user_id          UUID NOT NULL REFERENCES users(id),
    provider         TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    UNIQUE (provider, provider_user_id)
);

-- 0015: Verified Credentials
-- Self-sovereign: user submits proof, platform stores verification state.
-- Admins do not award credentials. Verification is domain-scoped.
CREATE TABLE verified_credentials (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id         UUID NOT NULL REFERENCES users(id),
    domain_id       UUID NOT NULL REFERENCES expertise_domains(id),
    verification_type TEXT NOT NULL CHECK (verification_type IN (
                        'license_number',    -- e.g. medical license
                        'institution_email', -- @university.edu pattern
                        'orcid',             -- researcher ORCID
                        'github_org',        -- member of org
                        'manual_review'      -- human review queue
                    )),
    verification_data JSONB NOT NULL,        -- encrypted at app layer
    status          TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'verified', 'revoked')),
    verified_at     TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_verified_credentials_user   ON verified_credentials(user_id);
CREATE INDEX idx_verified_credentials_domain ON verified_credentials(domain_id)
    WHERE status = 'verified';
```

### Revenue Layer (Phase 2)

```sql
-- 0016: Revenue Events
-- Append-only ledger. Source of truth for revenue attribution.
CREATE TABLE revenue_events (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    space_id    UUID NOT NULL REFERENCES spaces(id),
    type        TEXT NOT NULL CHECK (type IN (
                    'subscription',
                    'intelligence_api',
                    'verified_credential'
                )),
    amount_cents BIGINT NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end   TIMESTAMPTZ NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 0017: Contributor Payouts
CREATE TABLE contributor_payouts (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id         UUID NOT NULL REFERENCES users(id),
    space_id        UUID NOT NULL REFERENCES spaces(id),
    period_start    TIMESTAMPTZ NOT NULL,
    period_end      TIMESTAMPTZ NOT NULL,
    contribution_weight NUMERIC(10,4) NOT NULL,
    amount_cents    BIGINT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'processing', 'paid', 'failed')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 0019: Space Predictions (Phase 3 — schema slot reserved in Phase 2)
-- Receives feedback from Teri simulations via CommunityFeedback write-back interface.
-- actioned_at is the calibration signal: predictions acted on and confirmed accurate
-- improve Teri's confidence weighting for that community in future simulations.
CREATE TABLE space_predictions (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    space_id        UUID NOT NULL REFERENCES spaces(id),
    source          TEXT NOT NULL DEFAULT 'teri',
    type            TEXT NOT NULL CHECK (type IN (
                        'topic_signal',           -- topic likely to emerge or accelerate
                        'contributor_trajectory', -- contributor likely to become domain anchor
                        'health_risk'             -- community dependency or churn risk
                    )),
    payload         JSONB NOT NULL,
    confidence      NUMERIC(4,3),
    horizon_days    INTEGER,
    generated_at    TIMESTAMPTZ NOT NULL,
    expires_at      TIMESTAMPTZ,
    actioned_at     TIMESTAMPTZ    -- null until moderator acts; used for Teri calibration
);
CREATE INDEX idx_space_predictions_space     ON space_predictions(space_id);
CREATE INDEX idx_space_predictions_type_exp  ON space_predictions(space_id, type, expires_at)
    WHERE actioned_at IS NULL;
```

---

## Contribution Weight Model

Weight is computed per contribution type. Initial defaults — calibrated against Phase 0 community data before Phase 2 payout launch:

| Type | Base Weight | Notes |
|---|---|---|
| `message` | 1.0 | Floor. Adjusted by net reactions (Phase 2) |
| `topic_open` | 2.0 | Creating structure has higher value than filling it |
| `topic_resolve` | 1.5 | Closing the loop matters |
| `moderation` | 3.0 | Highest weight. Moderation is the hardest unpaid labor |
| `summary` | 4.0 | Highest single-action value. Summaries compound for all future readers |
| `reaction_net` | 0.5 | Daily aggregate. Net positive only. Floors at 0. |

**Moderation quality constraint.** The `moderation` weight (3.0) is the highest per-action weight because moderation is the hardest unpaid labor. However, weight must not reward hostile gatekeeping. The following moderation actions do NOT generate contribution records: closing questions as duplicates, downvoting without comment, or removing content without documented reason. Only actions that improve community structure (banning spam, resolving conflicts, approving new members, documenting moderation decisions) generate weight. This distinction is enforced at the application layer, not the schema layer — the schema records what the application sends.

**Why this revenue model works when Quora's failed.** Quora ran contributor revenue sharing tied to advertising revenue — an unstable pool determined by advertiser markets, not community value. Top earners made a few hundred dollars/month; most made less. The incentive pointed at impressions, not quality. Pebesen's payout pool is space subscription revenue — stable, directly proportional to the value the community provides to its members, and set by the space owner who has the clearest view of that value. The mechanism is identical in name and different in everything that matters.

Payout for a contributor in a space for a period:

```
payout = (contributor_weight / total_space_weight) × (space_revenue × payout_fraction)
```

`payout_fraction` is a space-level setting, defaulting to 0.20 (20% of space revenue). Space owners set this within platform-defined bounds (min 0.10, max 0.40).

---

## Real-Time Architecture

WebSocket events flow through Redis pub/sub. Channel per space: `space:{space_id}`.

```
Client → WS upgrade → ConnectionState { user_id, subscribed_spaces }
       → read_loop: subscribe | unsubscribe | catch_up | pong
       → write_loop: Redis subscriber + internal mpsc

Redis channels publish:
  { type, space_id, payload }

Event types:
  message_created | message_updated | message_deleted
  topic_created | topic_updated | topic_status_changed
  read_position_updated | presence_heartbeat (opt-in only)
  contribution_recorded (for real-time weight display — Phase 2)
```

---

## Search Architecture

Two complementary search surfaces:

### Keyword Search (Meilisearch)

Index: `messages`

```
searchableAttributes:  [content, topic_name, stream_name, author_display_name]
filterableAttributes:  [space_id, stream_id, topic_id, author_id, created_at,
                        topic_status, domain_ids]
sortableAttributes:    [created_at, contribution_weight]
```

Async indexer in `pebesen-search`: batches up to 100 tasks or 500ms, 3-retry exponential backoff.

### Semantic Search (pgvector)

Embedding model: `all-MiniLM-L6-v2` (384 dimensions) via `fastembed-rs`.

Embeddings are computed async after message insert. The IVFFlat index is created after 1,000 rows. Before that, exact search.

Used for:
- "Similar topics" across communities (Phase 2 public directory)
- B2B intelligence clustering (Phase 2)
- Cross-space expert discovery (Phase 2)

---

## Intelligence API Architecture (Phase 2)

The `pebesen-intelligence` crate exposes a separate API surface, rate-limited and key-authenticated, distinct from the user-facing API.

```
GET  /v1/intelligence/spaces/:slug/domains
     → top expertise domains by contributor weight, message density

GET  /v1/intelligence/spaces/:slug/domains/:domain/signal
     → weekly aggregated signal: message volume, contributor count,
       sentiment proxy (net reaction ratio), topic open/resolve rate

GET  /v1/intelligence/spaces/:slug/experts
     → verified + high-weight contributors per domain
       (respects contributor opt-out flag)

GET  /v1/intelligence/spaces/:slug/topics/trending
     → topics with accelerating last_active + message velocity

POST /v1/intelligence/spaces/:slug/predictions
     → receive Teri simulation output (CommunityFeedback write-back)
     → requires space-scoped intelligence API key
     → payload: { type, payload, confidence, horizon_days, generated_at, expires_at }
     → inserts into space_predictions table
     → returns { prediction_id }

POST /v1/intelligence/spaces/:slug/predictions/:id/action
     → moderator marks a prediction as actioned
     → sets actioned_at = NOW()
     → this is the calibration signal fed back to Teri for confidence adjustment
```

Privacy constraints enforced at query layer:
- All counts anonymized below threshold of 5 contributors
- Individual contributor data only returned if `intelligence_opt_in = true` in user settings
- Space must explicitly enable intelligence API in settings
- No raw message content in any intelligence endpoint

---

## Revenue Attribution Flow

```
Space subscription payment
    → revenue_events INSERT (type=subscription, amount, period)
    → attribution_job runs at period end
        → reads contributor_stats for space + period
        → computes payout per contributor (weight-proportional)
        → writes contributor_payouts rows
        → triggers payout processing (Stripe Connect or equivalent)
```

Contributor opt-out: setting `receive_payouts = false` routes their share back to the space owner. No silent redistribution.

---

## Authentication

- **Access token**: JWT, 15-minute TTL, signed with `JWT_SECRET`, in-memory only (never localStorage)
- **Refresh token**: UUID v4, stored in Redis, 30-day TTL, rotated on use, `httpOnly; Secure; SameSite=Strict` cookie
- **Bot tokens**: hashed API keys, scoped to space, rate-limited at 60 msg/min
- **Intelligence API keys**: separate key namespace, space-scoped, usage-logged for billing

---

## Access Control

Three levels:

```
Platform:  superadmin (internal only)
Space:     owner | admin | member | guest
Stream:    inherits space + optional stream-level private membership
```

Topic access derives from stream access. No topic-level ACL in Phase 0 (added if demand is validated).

Public read: unauthenticated access to public spaces, streams, topics, messages, and search. No write, no subscription to WS, no unread state.

---

## Non-Functional Targets

| Metric | Target | Phase |
|---|---|---|
| Message delivery P99 | < 100ms | 0 |
| Topic autocomplete P99 | < 50ms | 0 |
| Search response P95 | < 200ms | 0 |
| Semantic search P95 | < 500ms | 2 |
| Intelligence API P95 | < 1s | 2 |
| Single-binary self-host RAM | < 512MB | 0 |
| Docker Compose cold start | < 30s | 0 |

---

## Platform Health Metrics

**Traffic is not community.** Quora has 400M MAU driven 82% by organic search. Revenue in 2023: $20M. The gap between those numbers is the cost of optimizing for reach instead of depth.

Pebesen's primary health signals, in priority order:

| Metric | Definition | Anti-metric (never optimize for this) |
|---|---|---|
| Engaged members | Users who posted, resolved a topic, or wrote a summary in last 30 days | Monthly active visitors |
| Knowledge depth | Topics with ≥1 reply + domain tag + contributor weight > 0 | Total message count |
| Contributor retention | % of top-10 contributors per space still active 90 days later | New registrations |
| Space self-sufficiency | Spaces where hosting revenue ≥ hosting cost | Total spaces |
| Payout legibility | % of contributors who can correctly explain their own weight | Total payout amount |

The last metric is non-negotiable. If a moderator cannot explain in plain language why their contribution weight is what it is, the model has failed regardless of mathematical correctness. This must be validated in user testing before Phase 2 payout launch.

---

## Deployment Topology

### Self-Hosted (AGPL)
```
docker compose up -d
  pebesen-app    (Rust binary)
  postgres       (16-alpine + pgvector extension)
  redis          (7-alpine)
  meilisearch    (latest)
  caddy          (TLS termination)
```

### Hosted (Commercial)
Same topology per tenant. No multi-tenant data mixing at DB layer. Tenant isolation at the space level with space-scoped API keys and row-level security where applicable.

---

## Security Baseline

- Argon2id for password hashing (memory: 64MB, iterations: 3, parallelism: 4)
- Constant-time password comparison
- No user enumeration on login failure
- Rate limiting on all auth endpoints (10 req/min/IP)
- HTML sanitization on all Markdown output (strip script/iframe/on* attributes)
- External links: `target="_blank" rel="noopener noreferrer"`
- Verification data in `verified_credentials` encrypted at application layer before storage
- Intelligence API: space-owner opt-in required, contributor opt-out respected
- DMs: E2E encrypted (X25519 + XChaCha20-Poly1305), no plaintext stored, not indexed
