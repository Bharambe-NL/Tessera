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

### BN-015 An info flag does not make a card flagged

**Spec** 01 section 4.2: "`flagged` means at least one open Flag exists." 07 section B5: "`card_status`
is `flagged` when any flag has severity warn or block."

**Decision** Severity warn or block. An info flag shows as a chip on the card header and does not
change the card's status or put it in the Flags queue.

**Reason** The two rules disagree, and the difference is not academic: `fast_mode_notice` is an info
rule that fires on every fast card (doc 07 section B8.1), and `verifier_below_threshold` fires on
every deep card until M8. Under doc 01's reading every card in the product would be flagged and the
Flags queue would hold nothing anyone has to decide, which is the opposite of what doc 09 section 6
sorts it for. Found because the projection followed doc 01 and the Verifier followed doc 07, so a
replay changed a card's status. Doc 01 section 4.2 should be corrected in v0.2.

### BN-016 A card with no citations scores zero confidence

**Spec** 07 section B8.6 computes `card_confidence` as supported citations over all citations (0.5),
no block flags (0.25), no stale sources (0.15), visual blocks all bound (0.1). It does not say what
the first term means when there are no citations at all.

**Decision** In deep and research, a card with no citations scores 0.

**Reason** The other three terms reward the absence of problems the card had no opportunity to have.
A deep card that retrieved nothing would otherwise score 0.5 and, per doc 09 section 4's confidence
dot, present as no worse than a card with sources. Doc 06 section A10 already fixes the Synthesizer's
confidence at 0 for that case; this keeps the Verifier from raising it back.

### BN-017 The generator writes prose from templates, not from a model

**Spec** 02 section 5.1 has a model write the paragraph prose from the fact
statements, then a deterministic pass confirm every planted value appears verbatim.
02 section 9 caches model prose by (seed, prompt hash) and states that with a cold
cache the prose may differ but the facts, plantings and labels do not.

**Decision** Templates by default. A model backend can be added behind a flag without
moving a fact.

**Reason** Doc 02 section 9's guarantee is about the ledger, and the deterministic
verification pass is what actually makes the corpus usable. Templates make
`gen build --seed 42` free, offline and byte identical, which is what CI needs and
what doc 02 section 10.4's run to run diff depends on. Prose fluency buys realism in
the passages a model reads; it does not buy a single point of any metric in doc 02
section 10.2. If it turns out to matter, the flag is a small change and the corpus
name already carries the generator version.

### BN-018 Every early flag is written, not only the blocking ones

**Spec** 03 section 7 emits `flag.raised.v1` per early flag. 03 section 10
`override_conflict` stops the run only when a `block` flag is present.

**Decision** The pipeline writes a Flag row for every early flag the Router raises;
only a `block` one cancels the run.

**Reason** A bug, found by the eval harness rather than by reading. The pipeline wrote
early flags only on the blocking path, so `advice_request` at severity warn reached
the Synthesizer and the Verifier, changed how the answer was written, and never
appeared in the Flags queue. Doc 02 section 10.2's flag recall read 0 on twenty advice
bait questions that had in fact been handled correctly, which is how it surfaced.

### BN-019 A metric with nothing to measure reports n/a, never zero

**Spec** 12 phase 3's acceptance: "the harness runs end to end on the mock provider
and reports every metric as 0 or n/a." Doc 02 section 10.2 defines the metrics but not
what an empty denominator means.

**Decision** `n/a` when the denominator is zero, and `n/a` for fact recall while the
retrievers do not exist, with the reason on the row.

**Reason** 0 means the pipeline tried and got none right; `n/a` means it was never
asked. Reporting the second as the first would make an unbuilt stage look like a
broken one, and would leave fact recall failing its threshold from now until M6, which
trains everyone to ignore a red line that will one day mean something. The deep path
in this build correctly reports that it found no sources, so it recalled nothing
because it asserted nothing.

### BN-020 An OpenAI-compatible adapter, and what depth means on it

**Spec** 10 section 3 lists anthropic, openai, google, mistral and ollama. It does not
mention Moonshot's Kimi. Pattern 21 exists so a provider is an adapter rather than a
change to any agent.

**Decision** One adapter for the whole OpenAI-compatible family, taking a base url, a
provider id and a model. It serves Moonshot, OpenAI and Ollama. Anthropic keeps its own
adapter, because its request shape is not in this family.

**Reason** The eval sweep is 400 questions, which is a real cost at frontier rates, and
the author asked for the bulk of it to run on Kimi. Doc 10 section 3's list is about
which providers to support, not about which wire formats exist, and three of the five
speak this one.

Two consequences worth stating rather than discovering.

BN-007 carries depth through `output_config.effort`, which this family does not have.
Temperature is the only dial, so a cheaper tier here is a *different model* rather than
a different setting, and `single_provider` takes three model ids for that reason. A run
that leaves them all the same is measuring one model three times, which is fine as long
as the report says so.

This family also does not accept a schema, only `response_format: json_object`. That
is doc 10 section 7's "else schema prompting plus validation" path: the prompt carries
the schema and the schema guard catches what the model got wrong. Expect more retries
here than on a provider with real structured output, and expect that difference to show
up in the per provider token counts rather than being hidden.

### BN-021 Keys are pasted into the keychain, never into an argument

**Spec** 01 section 4.16 and 12 operating principle 7: model keys live in the OS
keychain, the database never holds a secret, and no secret appears in any file.

**Decision** `tessera-keys set <key_ref>` reads the key from the terminal without echo
and writes it straight to the keychain. It is never accepted as a command line argument.
`check` proves the key works and lists the models the provider actually offers.

**Reason** A key passed as an argument lands in the shell history, in the process table,
and in whatever terminal scrollback later gets pasted into a bug report. The spec
forbids a secret in a file; an argument is worse than a file, because nobody thinks to
clean it up. `check` exists because model names move: guessing one produces a 404 that
reads like an outage, and confirming the key before a 400 question sweep is cheaper than
discovering it halfway through.

### BN-022 The Anthropic adapter checks what a model accepts

**Spec** 01 section 5's policy resolves `small` to a cheap model; BN-005 makes that
`claude-haiku-4-5`. BN-007 carries depth through adaptive thinking and
`output_config.effort`.

**Decision** The adapter holds an allowlist of model families that accept adaptive
thinking and effort, and sends neither to anything outside it. A model nobody has heard
of gets the conservative request.

**Reason** A bug, and one no test could have caught. Adaptive thinking and effort both
arrived with the 4.6 generation; sending either to Haiku 4.5 is a 400, not a silently
ignored field. The adapter sent them unconditionally, so every Router call, on every
card, would have failed against the real provider while passing every mock test in the
suite. It surfaced on the first live call `tessera-keys check` made.

The allowlist direction matters. A denylist of known-old models would treat a model
released after this was written as modern, and fail on it; an allowlist treats it as old
and merely leaves some capability unused, which is the failure worth having.

Structured output is deliberately not gated the same way: it is not part of that
generation, and dropping it alongside thinking would push every call onto the schema
prompting path for no reason.

### BN-023 What a live provider taught that the mock could not

**Spec** 12 phase 4 onward reports numbers from a real provider. Doc 12 operating
principle 6 runs the eval from phase 3.

Three failures surfaced on the first live calls, none of which any mock test could have
caught, because a mock accepts whatever it is sent.

**Adaptive thinking on an older model.** BN-022. A 400 on every Router call.

**Temperature on a reasoning model.** The OpenAI-compatible adapter sent
`temperature: 0.2`, on the reasoning that a low temperature suits structured
extraction. Kimi K2.6 answers "only 1 is allowed for this model" with a 400. Temperature
is now not sent at all: the provider's default is correct on every model in the family,
including the ones that would have accepted a value.

**Output budget consumed by reasoning.** The agents size `max_tokens` for the content
they expect, and the Router asks for 1,200, which is generous for a classification
block. A reasoning model spends that budget thinking and stops at the limit having
written nothing, which arrives as `finish_reason: length` with empty content. The
adapter now adds headroom on top of the caller's figure rather than replacing it, so an
agent that asks for a long answer still gets one.

The pattern across all three is the same, and it is worth naming: **send a model only
what it is known to accept.** Every one of these was a parameter set because it seemed
useful, on a model that did not take it. The conservative request works everywhere; the
optimistic one works until it meets a model nobody tested against.

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
