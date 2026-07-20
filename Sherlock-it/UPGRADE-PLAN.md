# FlexNetOS/teri Upgrade Refs

----

[cluaiz/cluaize: An open-source, high-performance local AI inference engine.](https://github.com/cluaiz/cluaize)

----

Create a prompt for FlexNetOS/teri. Use github issues to write the prompt. The prompt must instruct codex to create wire in the "Add" repo and the repos you chose from "optional":

Add:
- github.com/fabio-rovai/brain-in-the-fish
- github.com/MOZARTINOS/mirofish-guide

Options:
- [cluaiz/cluaize: An open-source, high-performance local AI inference engine.](https://github.com/cluaiz/cluaize)
- github.com/ericcurtin/inferrs
- arxiv.org/html/2603.16642v1
- [SHA888/tinystories-burn-charlm: Character-level transformer language model in Rust](https://github.com/SHA888/tinystories-burn-charlm)
- [FlexNetOS/cellm: Mobile-native LLM serving engine research in Rust. Paged KV cache, multi-session scheduling, and Metal/Vulkan kernels for on-device inference under 512MB RAM.](https://github.com/FlexNetOS/cellm)
- [cryscan/web-rwkv: Implementation of the RWKV language model in pure WebGPU/Rust.](https://github.com/cryscan/web-rwkv)
- [SHA888/orbisfati: A universal metaphysical computation engine](https://github.com/SHA888/orbisfati)
- [SHA888/kekawa](https://github.com/SHA888/kekawa)
- [SHA888/tinystories-burn-charlm: Character-level transformer language model in Rust](https://github.com/SHA888/tinystories-burn-charlm)
- [SHA888 (Kresna)](https://github.com/SHA888)
- [algorithmicsuperintelligence/openevolve at 80945ed82886d5c4ff2f3d22436765d50cb61266](https://github.com/algorithmicsuperintelligence/openevolve/tree/80945ed82886d5c4ff2f3d22436765d50cb61266)
- [Piebald-AI/splitrail: Fast, cross-platform, real-time token usage tracker and cost monitor for Gemini CLI / Claude Code / Codex CLI / Qwen Code / Cline / Roo Code / Kilo Code / GitHub Copilot / OpenCode / Pi Agent / Piebald.](https://github.com/Piebald-AI/splitrail)
- https://github.com/666ghj/MiroFish/pull/325](https://github.com/666ghj/MiroFish/pull/325
- https://github.com/666ghj/BettaFish](https://github.com/666ghj/BettaFish
- https://github.com/amadad/mirofish](https://github.com/amadad/mirofish
- https://github.com/intercom/gtm-mirofish-demo](https://github.com/intercom/gtm-mirofish-demo
- https://github.com/intercom/gtm-mirofish-demo
- haqq.ai/blog/legal-ai-72-agent-simulation-predictions

The Rust database ecosystem experienced significant updates throughout the second quarter of 2026 (April–June). Driven heavily by local-first architectures, embedded databases, and multi-model engines designed for AI applications, developers saw massive structural upgrades across crates.io and GitHub.

Here are 20 of the highest trending database tools, crates, and skills updated during this period:

* **SurrealDB Core Engine**
* **Reference URL:** [https://github.com/surrealdb/surrealdb](https://github.com/surrealdb/surrealdb)
* **Short Description:** A scalable, distributed, multi-model document-graph database engine written entirely in Rust. It rolled out a steady stream of updates (v3.1.0 through v3.1.5) throughout June 2026, fixing transaction isolation edge cases, expanding RocksDB time-travel features, and upgrading query ergonomics.
* **Use Case:** Ideal for intelligent systems requiring vector search, graph relationships, and transactional document schemas inside a single engine.


* **KiteSQL**
* **Reference URL:** [https://github.com/KipData/KiteSQL](https://github.com/KipData/KiteSQL)
* **Short Description:** A lightweight, embedded relational database and native Rust data API inspired by MyRocks and SQLite. It provides typed ORM models, optimistic transactions, and direct SQL execution. Updated heavily on June 24, 2026.
* **Use Case:** Perfect for running self-contained, high-performance relational engines inside CLI tools, local desktop apps, or JavaScript/WebAssembly environments.


* **FrankenSQLite**
* **Reference URL:** [https://github.com/dicklesworthstone/frankensqlite](https://github.com/dicklesworthstone/frankensqlite)
* **Short Description:** A ground-up independent reimplementation of SQLite in pure, safe Rust. It brings parallel write capabilities via page-level Multi-Version Concurrency Control (MVCC) while preserving byte-level file-format compatibility. Underwent significant dependency updates and transaction handling expansions in June 2026.
* **Use Case:** Highly concurrent embedded software that reads and writes heavy volume to standard `.db` SQLite files simultaneously without blocking execution.


* **ai-memory**
* **Reference URL:** [https://crates.io/crates/ai-memory](https://crates.io/crates/ai-memory)
* **Short Description:** An executable background binary and Model Context Protocol (MCP) server built with Tokio, Axum, and a SQLite database. Its major v0.7.0 "attested-cortex" release on June 25, 2026, introduced Ed25519 signature verification and a cross-row SHA-256 hash chain on data events.
* **Use Case:** Providing cryptographic proof, auditing, and immutable long-term memory streams for local AI agents and coding assistants.


* **Surrealist**
* **Reference URL:** [https://github.com/surrealdb/surrealist](https://github.com/surrealdb/surrealist)
* **Short Description:** The official visual user interface and database management workbench for SurrealDB. The v3.9.1 release on April 30, 2026, completely overhauled the database schema designer, added visual dataset browsers, and integrated an AI-assisted query sidekick.
* **Use Case:** Visually exploring data connections, prototyping SurrealQL queries, and designing tables without manually managing migrations.


* **sea-clickhouse**
* **Reference URL:** [https://crates.io/crates/sea-clickhouse](https://crates.io/crates/sea-clickhouse)
* **Short Description:** A soft fork of `clickhouse.rs` that introduces complete integration for SeaQuery values, dynamic row types, and Apache Arrow column-oriented streaming. Shipped v0.14 on April 13, 2026.
* **Use Case:** High-throughput analytical streaming where you need to fetch results as schema-less vectors rather than strict, static structs.


* **crudcrate**
* **Reference URL:** [https://crates.io/crates/crudcrate](https://crates.io/crates/crudcrate)
* **Short Description:** A productivity crate tailored for the Axum web framework. It uses procedural macros to instantly generate boilerplate REST handler functions, filter parsers, and pagination structures from any pre-defined Sea-ORM entity. Updated to v0.9.2 on June 10, 2026.
* **Use Case:** Drastically shrinking back-end development times when building administrative web panels or basic REST APIs over relational databases.


* **saps**
* **Reference URL:** [https://crates.io/crates/saps](https://crates.io/crates/saps)
* **Short Description:** A modern framework that unifies Svelte, Axum, Postgres, and SQLx. It includes macro utilities that boot a real, ephemeral PostgreSQL database instance locally on demand directly inside a test execution pool. Shipped on June 2, 2026.
* **Use Case:** Writing rapid full-stack integration tests with completely isolated, parallelized, and self-contained database pipelines.


* **mneme-mcp**
* **Reference URL:** [https://crates.io/crates/mneme-mcp](https://crates.io/crates/mneme-mcp)
* **Short Description:** A local-first, standalone memory toolkit and storage daemon designed specifically for LLM workflows. It hit stable v1.1.1 on May 23, 2026, adding high-performance warm daemon processes over Unix sockets and hard size guardrails.
* **Use Case:** Acting as a persistent, low-latency relational memory substrate for context orchestration in local development setups.


* **oxirs-core**
* **Reference URL:** [https://crates.io/crates/oxirs-core](https://crates.io/crates/oxirs-core)
* **Short Description:** A zero-dependency, pure-Rust Resource Description Framework (RDF) data modeling layer and SPARQL graph database evaluation engine. It reached production-ready v0.3.1 on June 6, 2026.
* **Use Case:** Querying complex, decentralized semantic web graph networks and linked open data structures without heavy runtime dependencies.


* **vaultdb**
* **Reference URL:** [https://crates.io/crates/vaultdb](https://crates.io/crates/vaultdb)
* **Short Description:** A utility crate providing structured database-like operations (such as dynamic filtering, schema matching, and transactional appending) over standard Markdown files containing YAML frontmatter. Shipped on May 28, 2026.
* **Use Case:** Treating flat-file knowledge bases, logbooks, or Obsidian markdown vaults like an un-indexed relational table for automations.


* **oximedia-archive**
* **Reference URL:** [https://crates.io/crates/oximedia-archive](https://crates.io/crates/oximedia-archive)
* **Short Description:** A digital preservation workspace that uses SQLx and SQLite to run an embedded integrity logging database. Version 0.1.6 was published on April 26, 2026, bringing parallel file verification via Rayon.
* **Use Case:** Managing fixity tracking, automated checksum records (BLAKE3, SHA-256), and PREMIS preservation event logging for media workflows.


* **open-pincery**
* **Reference URL:** [https://crates.io/crates/open-pincery](https://crates.io/crates/open-pincery)
* **Short Description:** An agent orchestration architecture built around persistent append-only event logs. It relies heavily on PostgreSQL and `pgvector` for long-term memory context. Hit stable v1.0.0 on April 20, 2026.
* **Use Case:** Storing durable identities, work states, and semantic embedding vectors for long-running, autonomous AI workflows.


* **luhorm**
* **Reference URL:** [https://crates.io/crates/luhorm](https://crates.io/crates/luhorm)
* **Short Description:** A compile-time Object-Relational Mapper (ORM) that automatically spits out type-safe database access code by introspecting a live schema using Rust's `build.rs` script. Actively maintained through Q2 2026.
* **Use Case:** Projects requiring absolute compile-time guarantees that the database columns exactly match Rust structs without runtime reflection.


* **sqlx-paginated**
* **Reference URL:** [https://crates.io/crates/sqlx-paginated](https://crates.io/crates/sqlx-paginated)
* **Short Description:** A query builder extension for SQLx that streamlines case-insensitive searching, dynamic column sorting, and smart cursor/offset pagination. Its team initiated core development for MySQL support in Q2 2026.
* **Use Case:** Building secure administrative web endpoints that need flexible user-facing sorting and filtering while maintaining SQL injection shielding.


* **vision-graphql**
* **Reference URL:** [https://crates.io/crates/vision-graphql](https://www.google.com/search?q=https%3A%2F%2Fcrates.io%2Fcrates%2Fvision-graphql)
* **Short Description:** A compile-time optimized GraphQL abstraction layer for PostgreSQL. It resolves multi-depth, nested GraphQL queries into a single, optimized SQL string using parameterized binds. Core design benchmarks finalized on April 17, 2026.
* **Use Case:** Completely avoiding the classic N+1 database round-trip performance tax when resolving deep relational queries on the web.


* **SurrealDB Skill Core (`surreal-skills`)**
* **Reference URL:** [https://github.com/24601/surreal-skills](https://www.google.com/search?q=https%3A%2F%2Fgithub.com%2F24601%2Fsurreal-skills)
* **Short Description:** A curated developer skill template designed to feed database knowledge directly to AI coding environments. It packs automated scripts for health-checking database schemas and strict rules for SurrealQL composition. Updated on June 29, 2026.
* **Use Case:** Equipping modern terminal coding tools (like Claude Code, Copilot, or Cursor) with structural knowledge graphs of SurrealDB's data-modeling syntax.


* **surreal-sync**
* **Reference URL:** [https://github.com/surrealdb/surreal-sync](https://www.google.com/search?q=https%3A%2F%2Fgithub.com%2Fsurrealdb%2Fsurreal-sync)
* **Short Description:** A specialized internal clustering replication engine built by SurrealDB. It guarantees data distribution syncing, split-brain resistance, and state consistency across active nodes. Maintained heavily with continuous automated delivery sweeps throughout June 2026.
* **Use Case:** Underpinning distributed, real-time database sync protocols across massive geographically isolated cloud clusters.


* **revision**
* **Reference URL:** [https://github.com/surrealdb/revision](https://www.google.com/search?q=https%3A%2F%2Fgithub.com%2Fsurrealdb%2Frevision)
* **Short Description:** A dedicated Rust crate engineered for revision-tolerant serialization and deserialization. It allows binary database structures to safely read old or emerging data layouts without bricking. Continuously patched and updated in mid-2026.
* **Use Case:** Supporting backward-compatible schema evolutions inside data stores that save raw structures as bytes directly to disk.


* **prax-orm**
* **Reference URL:** [https://crates.io/crates/prax-orm](https://www.google.com/search?q=https%3A%2F%2Fcrates.io%2Fcrates%2Fprax-orm)
* **Short Description:** A Prisma-inspired, modern async ORM for Rust built on top of `tokio-postgres` and SQLx. It packs a fluent query builder, method-chaining syntax, and integrated vector search (`pgvector`) layers. Actively iterated on throughout Q2 2026.
* **Use Case:** Managing complex relational database operations across multiple backends (PostgreSQL, SQLite, MongoDB) using an ergonomic developer API.

---

