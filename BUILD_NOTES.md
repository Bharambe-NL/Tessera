# Build notes

Every decision taken where a spec was silent, where two specs disagreed, or where a spec value was
marked a placeholder. Doc 12 section "How to ask" requires this file. Each entry names the spec
section it answers.

---

## Product and stack

### BN-001 Product name is Tessera

**Spec** 11 section 1 (open question 1), which leaves the name open with candidates Wondering, Atlas,
Cartouche, Loupe.

**Decision** `tessera` as the crate and package identifier, "Tessera" in UI copy, from the first commit.

**Reason** Chosen by the author at planning. It works as a noun for a board, matches the repository,
and avoids the collision between the working name "Canvas" and the HTML canvas element that the
frontend uses throughout. Doc 11 open question 1 is closed.

### BN-002 Rust core with Tauri 2

**Spec** 10 section 3, which picks Tauri 2 and names Electron as the fallback.

**Decision** Rust core, Tauri 2 shell, as specified. Toolchain: rustup 1.29, rustc 1.98.0 stable,
target `x86_64-pc-windows-msvc`, against the MSVC 14.29.30133 build tools and Windows SDK
10.0.19041 already present on the machine.

**Reason** Confirmed by the author at planning.

### BN-003 UI is plain TypeScript modules with Vite, no framework

**Spec** 10 section 3 defers the choice to the build prompt ("Svelte or plain TS modules"); 11 section
4 forbids a component library.

**Decision** TypeScript ES modules bundled by Vite. No reactive framework.

**Reason** The prototype is already event driven and carries its own render diff
(`canvas-prototype.html:584`). A reactive framework would own the DOM that the canvas transform layer
needs to own, and doc 11 section 4 rules out a component library anyway.

### BN-004 Walking skeleton before the remaining spec phases

**Spec** 12 sequences phases 0 to 11 with no runnable product before phase 8.

**Decision** Phases 0, 1 and 2 as specified, then milestone 3, a thin end to end slice (Router,
Synthesizer, Visualizer, deterministic Verifier, canvas), then the remaining phases in spec order.

**Reason** Chosen by the author at planning. The slice changes no schema and no acceptance gate. It
front loads the integration risk in the RPC boundary and the event driven UI, which are the two places
where a late discovery would be expensive.

---

## Model policy

### BN-005 Alias resolution

**Spec** 01 section 5, which states its model names are placeholders for the architecture spec to fix.

**Decision**

| Alias | Model | Stages |
|---|---|---|
| `small` | `claude-haiku-4-5` | route, screening, rerank |
| `medium` | `claude-sonnet-5` | plan, verify, exercise, tutor |
| `frontier` | `claude-opus-5` | synthesize, visualize |
| `vision` | `claude-opus-5` | read |

**Reason** Current model identifiers. The spec's `claude-sonnet-4-6` and `claude-opus-4-8` are real but
superseded.

### BN-006 The vision alias resolves to Anthropic, not Google

**Spec** 01 section 5 points `vision` at a Google model; 12 phase 11 requires a fresh install to reach
a first verified deep card with one model key and one search key.

**Decision** `vision` defaults to an Anthropic model in the same provider as the other three aliases.

**Reason** A Google key for vision is a second model key, which contradicts the phase 11 acceptance
criterion. Every current Claude model is vision capable, so the default costs nothing. The Google,
OpenAI, Mistral and Ollama adapters still ship per doc 10 section 3; they are simply not required to
start. Doc 07 section A14 open question 1 (a vision fallback alias) is unaffected and stays user
configured, off by default.

### BN-007 Thinking and effort per stage

**Spec** silent. 01 section 5 fixes aliases per stage but not call parameters.

**Decision** Adaptive thinking on every stage. Effort carries the depth signal: `low` for route,
`high` for synthesize and verify, `xhigh` for research synthesis.

**Reason** The fixed thinking budget parameter is removed on the models in BN-005, so effort is the
only depth control available. Recorded here because it changes cost, which doc 10 section 14 models.

---

## Where the specs disagreed with each other

### BN-008 Trash lives in Home, not the rail

**Spec** 09 section 3 lists Trash as a rail item; 11 section 6 and 12 phase 8 place it as a Home filter.

**Decision** Home filter.

**Reason** Doc 13 records the move as adopted and marks doc 09's rail list for correction in v0.2.

### BN-009 Nine events missing from the doc 01 vocabulary

**Spec** 01 section 6.3 lists the v1 event vocabulary; doc 13 records nine event names used by the
agent specs that are absent from it.

**Decision** `context.stale_noted.v1`, `entity.resolved.v1`, `hook.denied.v1`, `citation.verdict.v1`,
`card.blocked.v1`, `visual.declined.v1`, `source.proposed.v1`, `exercise.item_reported.v1` and
`run.compacted.v1` are in the schema from the first commit, alongside the seven Learn mode events from
doc 14 section 2.

**Reason** Doc 13 states they belong in doc 01 v0.2. Adding them later would be a schema migration for
no reason.

---

## Where the prototype disagreed with the specs

Doc 12 states the specs win and the disagreement is recorded here.

### BN-010 No model side web search

**Spec** 10 section 7 disables tool use inside the model call so retrieval is always the core's job and
provenance is uniform. The prototype declares the `web_search` server tool
(`canvas-prototype.html:397`).

**Decision** Server side search tools are never declared. Retrievers are the only path to a passage.

**Reason** A model side search produces a claim with no Passage row behind it, so the Verifier's
support check has nothing to check and a Citation cannot be bound. It breaks the audit trail at the
root.

### BN-011 Synthesizer and Visualizer are two calls, not one

**Spec** 06 gives each its own packet, schema and failure taxonomy. The prototype asks for answer and
visual in one call (`canvas-prototype.html:398`).

**Decision** Two agents, two calls.

**Reason** Doc 06 section B1 makes the Visualizer read only the Synthesizer's grounded
`structured_summary`, never the raw passages. That is what stops a visual from claiming more than the
prose, and it only works if the calls are separate.

### BN-012 Storage is SQLite with an event log, not a JSON blob

**Spec** 01 section 8. The prototype persists the whole database as one JSON string through
`window.storage` or `localStorage` (`canvas-prototype.html:349`).

**Decision** SQLite in WAL mode with an append only Event table and projections written in the same
transaction.

**Reason** Doc 10 section 4 requires that a crash cannot leave a projection and its event apart. A
debounced whole database write cannot offer that.

### BN-013 The offline sample dictionary is dropped

**Spec** 12 phase 2 specifies a deterministic mock provider. The prototype falls back to a five entry
`SAMPLES` dictionary when the API is unreachable (`canvas-prototype.html:363`).

**Decision** The mock provider is a test fixture reached only under `provenance.source: test`. A user
facing provider failure surfaces as a failure, per each agent's taxonomy.

**Reason** Doc 06 section A10 requires that an unreachable provider never silently becomes an answer.
Sample text presented as an answer is exactly that.

---

## Measured findings

### BN-014 M0 canvas gate passes at 200 cards, ceiling between 400 and 800

**Spec** 12 phase 0 ("60 fps pan at 200 cards on a mid range laptop; if not, record the finding and
switch the layer to canvas rendering for edges and ink before continuing") and 10 open question 1.

**Decision** Keep DOM rendering for cards, edges and ink. No canvas layer.

**Measured** Windows 11, WebView2 151.0.4129.101, release build, 165 Hz display. Fixture cards carry
prose, findings, a visual with a populated block index, citations and a sources disclosure, so they
weigh what real cards weigh.

| Cards | Pan fps | p50 | p95 | p99 | Dropped | First render | Verdict |
|---|---|---|---|---|---|---|---|
| 200 | 124.2 | 6.10 ms | 6.20 ms | 66.6 ms | 3/240 | 272 ms | pass |
| 400 | 101.5 | 6.10 ms | 18.10 ms | 30.4 ms | 3/240 | 472 ms | pass |
| 800 | 52.4 | 18.10 ms | 30.30 ms | 90.9 ms | 12/240 | 845 ms | fail |
| 1600 | 25.5 | 36.40 ms | 42.50 ms | 121.2 ms | 240/240 | 1731 ms | fail |

**Reason** At 200 cards p50 equals the display refresh interval, so the pan is refresh locked rather
than work locked: the board is not the bottleneck at the gate's card count, which is the strongest
form of a pass. The ceiling sits between 400 and 800 cards, where p50 crosses one 60 Hz budget.

Two things follow. First, doc 10 open question 1 is closed for Tauri: no Electron fallback and no
canvas layer. Second, if a board is ever expected past roughly 500 cards, the cheap fix is viewport
culling (skip the transform write for cards outside the visible rectangle), not canvas rendering.
Culling is a change inside `renderCards`; canvas rendering would cost the accessibility tree, text
selection and the highlight to branch affordance, so it is the second resort, not the first.

**Not yet measured** macOS. Doc 12 phase 0 asks for both platforms and no Mac was available. The
gate runs from `TESSERA_GATE=200 TESSERA_GATE_OUT=<path> tessera-app`, so it reruns unchanged there.

---

## Open questions resolved as proposed

Doc 12's rule is to decide and continue. Every spec open question carrying a "Proposal:" line is
resolved as that proposal, without restating it here. That covers 01 questions 1 to 4 (which doc 02
already treats as resolved), 02 questions 2 and 3, 03 questions 1 to 3, 04 question 1, 05 questions 1
and 2, 06 questions A1, B1 and B2, 07 questions A1, B1 and B2, 08 question 1, 09 questions 1 and 2,
and 14 questions 1 and 2.

Four are measurements, not decisions, and resolve inside a milestone:

| Question | Resolves at |
|---|---|
| 10 question 1, Tauri webview canvas performance | M0 gate |
| 10 question 2, local embedding model choice and size | M6, on synthetic recall |
| 05 question 1 and 07 question B3, a local model for the support check on sensitive folders | M8, against an Ollama alias |

One stays open by choice: 02 question 1, the Dutch share of the internal folder. The generator takes it
as a `--lang-mix` parameter defaulting to the spec's 10 percent.
