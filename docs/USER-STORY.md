# Teri — User Story

How a human uses Teri, end to end. This is the product narrative the Web UI and CLI both serve.

## Who

- **The decision-maker** — a policy lead, comms/PR strategist, product or markets analyst who needs
  to rehearse "what happens if…" before committing to a real-world move.
- **The creator** — a writer or worldbuilder deducing a story's ending or exploring a scenario.
- **The community steward** — a moderator/operator of a Pebesen space who wants early warning on
  topic momentum, contributor churn, and space-health risks (the teri↔pebesen loop).

## The core job-to-be-done

> "I have some seed material and a question about the future. Give me a defensible prediction *and*
> a living world I can interrogate — not a single oracle sentence."

## The journey (5 steps = the 5-stage pipeline)

1. **Seed & ask.** The user uploads seed material (PDF / Markdown / text / JSON / URL) — or points
   Teri at a live community signal — and writes the prediction question in natural language
   ("How will this policy affect public sentiment in 30 days?").
2. **Watch the world build (Step 1 – Graph).** Teri extracts an ontology and a knowledge graph; the
   user sees the entity/relation graph render live (d3 `GraphPanel`).
3. **Review the cast (Step 2 – Environment).** Personas are generated from graph entities and the
   simulation is configured (time model, platforms, initial events). The user can review the agent
   roster and config before running.
4. **Run the simulation (Step 3 – Simulation).** Thousands of agents interact across dual platforms
   (Twitter + Reddit) over time-stepped ticks. The user watches the tick stream live and may inject
   God's-eye variables mid-run to test counterfactuals.
5. **Read & interrogate (Steps 4–5 – Report + Interaction).** A ReportAgent synthesizes a structured
   prediction report (summary, timeline, highlights, confidence). The user then **chats with the
   ReportAgent** for follow-ups, or **chats with any individual agent** in the simulated world to
   understand *why* the swarm moved the way it did.

The whole run is revisitable from the **History Database** — every prior simulation, graph, and
report is browsable and re-openable.

## What the user gets

- A **prediction report** grounded in an auditable simulated world (not a black-box answer).
- A **living world** to interrogate — agents, timeline, and the knowledge graph behind them.
- **Honesty by construction:** Teri refuses to run on a stub/canned inference backend, and labels
  `confidence` as synthesized report metadata, not calibrated probability (unless a calibration loop
  is wired). It is a scenario engine, not an omniscient oracle.

## Two front doors

- **CLI** (`teri run --seed … --query …`) — scriptable, one-shot, returns the report + `verdict.json`.
- **Web UI** (`teri serve` + the Vue studio on :8374) — the guided 5-step experience above, with
  live graph, tick, and log streaming over SSE.

## The community loop (steward story)

For a Pebesen steward, seeds arrive as **live community signal** (domains, contributors, topics,
sentiment) via `CommunityAdapter`, and Teri's predictions flow **back** to the platform via
`CommunityFeedback` — topic-momentum signals, contributor trajectories ("rising anchor",
"at-risk churn"), and space-health risks. When a moderator actions a prediction, that ground-truth
event calibrates future confidence: the model self-tunes per community over time.
