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

## Decisions from the v0.2 documents

Five documents arrived on 2026-08-25: `HANDOFF.md` and `15-memory-v0.2.md` are new,
`01-data-model-v0.2.md` and `05-retriever-agents-v0.2.md` revise their v0.1 originals, and
`14-learn-mode-tutor-v0.2.md` is byte identical to v0.1, so doc 14 is unchanged.

### BN-024 The code identifier stays `tessera`

**Spec** `HANDOFF.md` section 3: "Use identifier `canvas` in code until confirmed; keep the
product name in one config constant."

**Decision** Keep `tessera`. Adopt the constant.

**Why** That document was written before the naming question was put to the owner. It was put
and answered, the answer was Tessera, and BN-001 recorded it on the first commit, so the
condition the instruction waits on is already met. Ten crate names, every path and every import
now carry it. The instruction's second half is good advice at any name and is adopted: the
product name lives in one constant, so changing it stays a one line edit.

### BN-025 A pack may set a minimum depth, which answers two open questions at once

**Spec** 03 question 3 ("whether `depth_hints` should be able to force a minimum depth the user
cannot lower"; proposal: the pack may set a minimum and the UI shows why fast is unavailable)
and 06 question A2 ("whether fast mode should be allowed on the finance pack at all"), which
the document itself ties to the first.

**Decision** Both as proposed. The pack sets a minimum depth per question type; finance sets it
to `deep` for regulatory question types.

**Why** The second question reads as a product decision and turns out to be a consequence of the
first. Resolving it in the pack rather than in code means no branch anywhere asks which pack is
loaded, which is doc 12 principle 4: doctrine is data.

### BN-026 Background intake is skipped when the profile states a role

**Spec** 14 question 2, proposal yes. Resolved as proposed.

### BN-027 A local model for the sensitive folder support check stays a measurement

**Spec** 05 question 1 and 07 question B3, listed as still open in `HANDOFF.md` section 5.

**Decision** Unchanged from the approved plan: decided at M8 against an Ollama alias, on the
false positive numbers rather than in advance.

### BN-028 The memory schema lands as migration 0002, not as an edit to 0001

**Spec** 01 v0.2 adds `Card.builds_on`, source class `own_card`, ConceptLink relation
`builds_on`, and `Profile.memory_enabled`.

**Decision** A second migration file.

**Why** No database exists outside this machine, so editing `0001` would have worked and cost
less. It would also mean the migration runner had still never run a second migration on the day
M13 ships an installer that upgrades a real user's profile. Running it now costs nothing when it
fails.

It found something immediately. SQLite cannot widen a `CHECK` constraint in place, so adding
`own_card` means rebuilding `source`, which means dropping it, and `passage.source_id` cascades
on delete. With foreign keys enabled the migration would have deleted every passage in the
profile, and every citation points at a passage, so the audit trail would have gone with them.
`PRAGMA defer_foreign_keys` does not help: it delays the violation check, not the cascade. So
`Store::migrate` turns foreign keys off for the duration and `pragma_foreign_key_check` inside
the transaction earns the right to turn them back on.

The test that would have caught it in production is
`crates/tessera-store/tests/migration.rs::a_version_one_profile_upgrades_without_losing_a_row`.

### BN-029 The source class enum widens without a schema version bump

**Spec** 01 section 4.8 and `schemas/entity/common.v1.json`.

**Decision** `own_card` joins the enum, and the schema stays at v1.

**Why** Adding an enum value widens what validates, so every document valid under the old enum
is still valid. No bundle has ever been exported, so no reader exists anywhere that could reject
the new value. The version bumps when a change first narrows something, which is the case a
version number exists to signal.

### BN-030 The house style is a test, not a document

**Spec** `HANDOFF.md` section 7: "no dashes of any kind, sentence case, verbs name actions, no
apologies. The owner's preference: no em dashes anywhere and no 'it is not X, it is Y'
constructions. Run these as a lint on UI strings."

**Decision** `crates/tessera-style`, run by `cargo test`. Four rules are checked: dashes, a
hyphen used as sentence punctuation, title case in a heading, and the construction the owner
named. Apologies are checked too. "Verbs name actions" is left to review, because a checker that
guessed at it would flag every noun label a product legitimately has.

**Why scope is narrow** The first version guessed at which strings a user reads and reported
four violations in TypeScript, where `=>` reads as a closing HTML tag, and two in a doctrine
pack, where a synthetic regulator is called "Central Authority for Prudential Oversight" because
that is its name. A lint with a six in six false positive rate gets switched off in a week. So
the extractor is told what surface it is reading: HTML text nodes and label attributes, Rust
double quoted literals, the doctrine pack fields that reach the screen, and TypeScript only in a
file named `strings.ts`. M9 step 1 moved the card's copy into it, so the lint reads the strings on
the busiest screen; the copy the rail and the four pages write joins it in step 3.

U+2212 MINUS SIGN is deliberately not in the dash set. The zoom control uses it as the
counterpart to a plus, and it is a mathematical symbol rather than punctuation.

### BN-031 Memory emits no new events

**Spec** 15 and 05 v0.2 section 8.5 name no event of their own.

**Decision** The boards retriever uses the existing retrieval vocabulary, and `builds_on` rides
on `card.answered.v1`.

**Why** Recording it here so the absence reads as a decision rather than an oversight. Doc 05
section 8.5 says the card records `builds_on` for every `own_card` passage cited or used, which
is only known once the Synthesizer has finished, and `card.answered.v1` is the event emitted at
that moment. A projection field that no event carries cannot survive a replay, so it had to ride
on something.

### BN-032 The v1 regulation never stated its own v1 values

**Spec** 02 section 5.4: "A card written before this that cites CAR3 v1 for a changed value is
stale." Doc 02 section 10.2 scores staleness detection at 0.95.

**Found** while building doc 15 section 5's stale propagation case, which needs exactly the
card doc 02 section 5.4 describes. There was none, and there could not be one.

`corpus.build_layer_one` gave each regulation the facts matching `truth == "true" and
supersedes is None`. A superseded fact carries `truth == "superseded"`, so the v1 text got the
values that never changed and none of the thirty that did. `edge_cases.superseded_regulation`
then built the v2 text from the unchanged facts plus the v2 values. So the v2 values were in
the corpus, the v1 values were in no document at all, and nothing anywhere ever stated the old
number.

The consequence is quiet. Nothing failed, no test broke, and `staleness_detection` reported a
number. It reported it over an empty set, because a card can only be scored stale for citing a
value that some document stated.

**Decision** `reg-car3-v1` carries the superseded facts. It is the v1 text, and the v1 values
are what a v1 text says.

`test_the_v1_regulation_states_its_own_superseded_values` is the check that it stays that way.

### BN-033 One fact is held out of its regulation so the own_card case can exist

**Spec** 15 section 5: `own_card` as sole support after verification, target 0.

**Problem** The target is only meaningful if a case exists where a prior card is the only
support available. Regulations carry every true fact of their domains, so every fact a prior
card could state is also in a consolidated text that nothing removes. Removing a regulation to
make room was not an option: doc 02 section 5.4 deletes internal documents specifically so that
"a regulation never quietly disappears".

**Decision** One fact is held out of both the v1 and v2 texts and given to a single internal
memo, `int-memory-sole`, which the timeline removes at T2. After T2 the value appears nowhere
except a prior card, which is exactly the moment doc 15 section 2's rule has to hold.

Two further consequences, both deliberate:

The question set pins it. Two hundred root questions are drawn from six hundred facts, so
leaving the trap fact to the shuffle would have put the case in the corpus and left it out of
the question set about two times in three.

The prior cards are planted, not found. Board cards are seeded from root questions, and no
question requires a superseded fact or the held out one, so no board card stated either.
Searching for a card that cannot exist is what the first version of this module did, and it
returned nothing twice while reporting success.

### BN-034 A memory metric reports n/a until a card has been tempted

**Spec** 15 section 5's four measurements, and BN-019.

**Decision** The denominator of `own_card_sole_support_rate` is the number of cards that cited
a prior card at all, not the number of cards.

**Why** The target is zero, and a rate of zero over zero cards would report `pass` from the day
it was written until long after it stopped being true. What would actually have happened is
that no card has ever been offered a prior card to lean on. With this denominator the metric
says n/a while the temptation does not exist and becomes real the moment it does. The other
three report n/a on `memory_enabled` in the run manifest, the same way fact recall reports n/a
on `retrievers_enabled`.

### BN-049 A snapshot is a tree as well as a manifest

**Spec** 02 section 5.4, which describes the corpus as "a sequence of snapshots at T0, T1, T2, T3"
and section 8, which lists `snapshots/T0.json ...` as the only snapshot output.

**Decision** `gen build --seed 42 --snapshot T3` writes the whole corpus as it stands at that
label to `<out>/<seed>-<label>`, beside the default tree rather than replacing it. The manifests
are computed by hashing what the materialiser returns, so a file and its snapshot entry cannot
disagree. A build with no flag writes exactly what it wrote before, byte for byte.

**Reason** The manifests named what changes at T2 and T3 but no code ever wrote those bytes, so
the single tree on disk was the union of every time and nothing could read the corpus as it stood
at T3. Doc 02 section 5.4's own acceptance, a board written at T1 and reopened at T3, needs both
trees present at once, which is why the snapshot tree sits beside the default one rather than
overwriting it. Computing the manifest from the materialised documents rather than beside them
removes the way the two could drift: the earlier code hashed `body + "\n\nRevised at T2."` in one
place and would have written the file in another.

### BN-050 A snapshot records the questions it strands

**Spec** silent. 02 section 5.4 deletes documents between snapshots and section 6 fixes the
question set at 400 for every snapshot.

**Decision** A snapshot tree counts the questions whose required sources it no longer holds,
writes one `stranded_question` row per question into `ledger.jsonl`, and reports the count in the
build row and the README. Seven questions are stranded at T3.

**Reason** BN-019. A sweep against the T3 tree will answer those questions worse, and the reason
is that the documents were deleted on purpose rather than that retrieval got worse. Without the
record the next person to run that sweep reads a recall drop and starts fixing the retriever. The
first verify only run made the point: `fact_recall_research` read 0.000 off exactly one question,
whose only source is the memo doc 15 section 5 removes at T2.

### BN-051 Staleness is computed from the files, never from the corpus manifests

**Spec** 05 section 3, "re-verification of cited locators (content hash comparison)", and section
7's three reasons: `content_changed`, `locator_gone`, `superseded_version`.

**Decision** The re-verification pass reads the corpus the way a retriever does: the file at the
locator, its bytes against the baseline tree's, and the other files in its folder. It never reads
`snapshots/*.json` or `facts.jsonl`. All three reasons come out of that: a locator that resolves
in neither tree is gone, bytes that moved are changed, and a document with a later version beside
it is superseded.

**Reason** The generator's manifests are the answer the metric is scored against. A pass that read
them would report what the generator already knew, and `staleness_detection` would measure whether
the copy succeeded rather than whether the product noticed. The cost is that a locator resolvable
in neither tree is counted unresolvable rather than assumed gone, which is the honest reading.

### BN-052 The version in force is read from the folder, not filtered on

**Spec** 07 section B8.4, "version_ref equals the version in force".

**Decision** A regulatory document whose stem carries a version, `reg-car3-v1`, is superseded when
a sibling in the same folder carries a later one. This is a freshness check at re-verification
time. It is not the version_ref retrieval filter, which stays struck.

**Reason** Both ends of doc 15's stale chain cite `reg-car3-v1.md`, whose bytes are identical at
T1 and T3, so neither a hash comparison nor an existence check can find them. Supersession is the
only signal that reaches them, and without it two of the three gates cannot be measured at all.
The struck filter was a different thing for a different purpose: filtering retrieval results by
version, measured useless because only 2 of 104 documents carry a version and a filter over two
documents cannot move a recall gate. Those same two documents are exactly what the freshness check
exists for.

### BN-053 An imported card records the locator a retriever would

**Spec** 01 section 4.9's dedupe key, and 05 section 12's zero duplicates gate.

**Decision** The eval's board import writes each citation's locator in the form the retriever that
reaches the same file records, relative to the folder it indexes, rather than the form the corpus
files it under. `regulatory/reg-car3-v1.md` becomes `reg-car3-v1.md`.

**Reason** Found by splitting a number. The first re-verification marked 51 cards stale and the
follow up leg found none, because the two spellings produced two Source rows for one document. A
card answered today deduped against the retriever's row and inherited nothing from the one the
imported card cited. Doc 05 section 12's gate would have counted the same duplication.

### BN-054 A card read back is not an answer, and a fixture question is not a sample

**Spec** 02 section 10.2, whose metrics are all defined per question asked.

**Decision** `gen score` splits three populations. A `verify_only` row re-verifies an existing card
and is read only by the staleness metrics. A question the verify leg asked to build a stale
ancestor is read only by the Planner metrics. Everything about answering, recall, precision,
flags, cost and latency, reads the questions that were actually sampled, and reports n/a when
there are none.

**Reason** BN-019, twice over. A re-verification carries the card's own answer and the card's own
facts, so counting it scored `fact_recall_deep` at 1.000 for restating text nobody was asked to
find. The verify leg picks its questions because their sources went stale, several of them deleted
outright, so scoring their recall measures the timeline. Both numbers looked like results.

### BN-055 A verify_only packet carries the card id the store holds

**Spec** 01 section 3, "identifiers are ULIDs", and 07 section B3's re-verification batch.

**Decision** The Verifier packet requires a ulid `card_id` for every kind except `verify_only`,
which accepts whatever id the store holds. Every other packet, and every other kind, keeps the
rule unchanged.

**Reason** A re-verification reads a card that already exists, and on the eval corpus that is one
the generator named `B-01-C03`. Those ids are kept deliberately, because doc 15's ground truth
names prior cards as `board_id/card_id` and the boards retriever's document reference is exactly
that; minting ulids for them would need a translation table on the path the memory gates are
measured through. Relaxing the rule for one packet kind was the smaller change, and it points at
a real case: doc 01 section 7's bundle merge re-verifies cards that arrived from another machine.
The follow up leg runs through the ordinary pipeline on cards the product minted, so nothing else
needed the exception.

### BN-057 The grounded mock has to produce structure, or half the product is unmeasured

**Spec** 02 section 10.1, where the mock exists so the pipeline can be measured for nothing.

**Decision** The grounded mock emits one `structured_summary.value` per quoted passage and its
findings as strings carrying their own `[n]` markers, which is the shape `draft_schema` declares.

**Reason** It emitted an empty summary and findings as objects. The Synthesizer's
`filter_map(Value::as_str)` dropped the objects silently, so every grounded run ever recorded
produced 400 cards with no findings; and `select_type` saw an empty summary, declined, and returned
before composing, so `index_blocks`, the doctrine limits and every other part of doc 06 part B were
unreachable on the eval. `visual_fidelity` and `visual_type_match` had no thresholds, so the report
said nothing was wrong. A mock that cannot exercise a stage measures the mock.

### BN-058 `visual_type_match` is measured and not gated

**Spec** 06 section B12, which gates type match at 0.85.

**Decision** The metric computes and reports; the threshold is not applied.

**Reason** The type follows the shape of the summary the model wrote, so on a scripted provider it
scores the script. The grounded mock emits values and nothing else, which selects a table every
time and lands at 0.24 against a corpus expecting five types. A gate that fails every free run is a
gate people learn to ignore, which is the failure doc 07 section B12 names for flag rules. It
becomes a gate on a live sweep, where the summary is the model's own. `visual_fidelity` is gated at
1.00 as B12 asks, because it is a deterministic property of the Visualizer rather than of the
summary it was handed.

### BN-059 A safety gate needs a denominator, so the hostile document gets its questions

**Spec** 02 section 10.3, injection resistance 100 percent.

**Decision** The facts the hostile document states are hoisted to the front of the root question
pool, the same way doc 15's sole source fact already was. Nine questions now carry the
`hostile_document` case; the hostile copy stays out of `required_sources`, because citing it is the
failure the metric measures.

**Reason** The corpus planted the hostile document from the first build and no question ever
reached it, so `injection_resistance` reported n/a against a threshold of 1.0 for every run there
has been. The three facts it carries are stated in three or four real documents each, so a question
about one is answerable honestly and pulls the hostile copy alongside. This is the same defect the
`held_out` hoist already fixed for memory, in the same place, found only because the report was read
for what it was not measuring rather than for what it was.

The metric's note now carries how many of those questions actually cited the hostile document,
because retrieval decides that and not the question set. On the first run it was 3 of 9: the gate
reads 1.000 and the exposure behind it is three answers.

### BN-060 The ledger check has to ask the Verifier's question, not a neighbouring one

**Spec** 02 section 10.2: "Citations whose passage supports the claim span, per Verifier verdict and
per ledger check. Both are reported so the Verifier's own accuracy can be measured."

**Decision** `citation_accuracy_ledger` is the share of citations whose passage states a value the
question required, decided by the same matchers the corpus was verified with.
`verifier_agreement` is how often the Verifier reached that same answer. The run record carries the
cited passage text so the scorer can ask the question at all.

**Reason** The first implementation counted verdicts equal to `supported` and called it the ledger
check, which reported the Verifier's own opinion twice and could never measure its accuracy. The
second asked whether the cited *document* states a required fact, which is a different question from
the one the Verifier answers, and scored the difference as disagreement: 0.609 where most of the gap
was definitional. A gate at 0.90 that measures the wrong comparison is worse than one that reports
n/a.

### BN-061 The support check runs, and its two gates stay advisory on a mock

**Spec** 07 section B8.2, and 02 section 10.3's 0.90 agreement gate.

**Decision** The support check is live: one batched call on the verify stage, then the deterministic
override, with unsupported and weak-numeric raising the flags B8.2 names and a provider failure
falling to all weak plus a card flag. `support_check_enabled` is now true, so the verdicts in a run
record are real. `citation_accuracy_ledger` and `verifier_agreement` join `route_accuracy` and
`flag_false_positive_rate` in `MOCKED`.

**Reason** The grounded mock cites every passage it was handed and quotes them verbatim. The support
check therefore calls almost everything supported, while the ledger asks whether the passage carries
a required value, and the two disagree on every passage that was retrieved but not needed. Both
readings are correct about different passages; the disagreement is the fixture's. On a real provider
the answer cites what it used and the two questions converge, which is the run where doc 02 section
10.3's automation gate means what it says. Until that run happens
`verifier_below_threshold` keeps firing on every deep card, which is doc 07 section B9's own
fallback and the honest state.

The exemption is an exemption and not a retirement: a guard test already refuses any name in
`MOCKED` that has no threshold to be exempted from, which is how `visual_type_match` got its doc 06
section B12 threshold back after being briefly left ungated.

### BN-062 The Synthesizer and Visualizer packets are validated at their boundary

**Spec** 06 sections A4 and B4, which declare both packets, and doc 12's operating principle 1,
validate at every boundary.

**Decision** `schemas/packet/synthesizer.v1.json` and `schemas/packet/visualizer.v1.json` now exist
and both agents name them.

**Reason** Neither file had ever been written. Both agents returned `tessera:entity/common.v1` as
their packet schema, which validates the shared primitives and nothing packet shaped, so a packet
missing its passages, its request or its summary reached a model and was answered. Four packets were
guarded and two were not, and the two unguarded ones are the pair that spend the most tokens.

### BN-063 What the packets carried and the agents never saw

**Spec** 06 sections A2, A4 and A7.

**Decision** Four fields that existed on one side of the boundary and nowhere on the other are now
joined up: `ancestors` on the Synthesizer packet (hardcoded empty, while the prompt loop that reads
them was already written), `request.kind` and `request.anchor_text` (hardcoded `root` and null),
`plan.constraints.must_include` (produced by the Planner, read by nobody), and `writing_rules` (taken
from the pack into the packet and never put in a prompt). `card.synthesized.v1` gains the
`audience_id` doc 06 section A7 lists.

**Reason** Each one reads as working. A follow-up was written as though nothing preceded it, a branch
spawned from a highlighted phrase looked identical to a question typed from nothing, and the
doctrine's units and spelling governed nothing. None of them fails a test, because what they produce
is a plausible answer to a smaller question.

### BN-064 Two of the three conflict resolutions were unreachable

**Spec** 06 section A8.3: "higher trust rank wins; equal rank, later `published_at` wins; otherwise
both are presented and the conflict is recorded."

**Decision** The readings are sorted by trust rank and then by date, and the resolution names which
rule decided. The winning value rides on the conflict so a later fix-up call can use it.

**Reason** The code took the best trust rank and then reported `higher_trust` whenever a best
existed, which is whenever there were any readings at all. Two passages of equal rank resolved as
though one outranked the other, and `later_date` and `presented_both` could not be produced by any
input. The output schema had allowed all three since it was written.

### BN-065 The doctrine model rules run, and the rule's own words are the check

**Spec** 07 section B8.5.

**Decision** A rule whose detector starts with `model:` is collected during the deterministic pass
and asked in one batched call at the `doctrine_model_checks` stage. Each rule's own `description` is
the question, so the pack decides what is looked for and the code decides only when to ask. Matches
are capped at warn. A call that fails lists every rule as unchecked and flags the card, never as
passed.

**Reason** Doctrine is data, never code. The alternative was a detector function per rule id, which
would have put `jurisdiction_drift` in Rust and made the pack a list of names for behaviour it did
not contain. The finance pack has shipped three of these rules since it was written and every one was
listed as skipped: "This build runs deterministic detectors only."

### BN-066 The support check flags cards, and a flagged card is not remembered

**Spec** 15 section 3's eligibility rule, and 07 section B10's fail closed posture.

**Decision** Kept. A card the support check could not verify is flagged, and doc 15 section 3 only
remembers a card whose status is `done`.

**Reason** Found by a test rather than by a metric. An end to end memory test scripted the mock for
route, plan, synthesize and visualize but not verify, so the support check failed, the card was
flagged, and it was no longer eligible to be recalled on another board. That is the fail closed rule
working: a card nobody could verify should not become the evidence for the next one. The fixture now
answers both verify stage calls, which is what a fixture wanting an admitted card has to do.

Worth naming because the chain is not obvious: eligibility reads `status = 'done'` and separately
excludes open `block` flags, and the Verifier sets the status to `flagged` on any warn. The second
clause is therefore unreachable, and any warn flag anywhere keeps a card out of memory. That is
defensible and it is stricter than doc 15 section 3 reads on its own.

### BN-067 What M7 leaves for a run that spends money

**Spec** 06 sections A8.4, B8.1, B8.4, B8.5 and B3.

**Decision** Five pieces of doc 06 are built no further, each because a mock cannot exercise them and
a fixture that pretended to would measure itself.

**The audience rewrite** (A8.4). The second call that rewrites an answer for its audience, with the
deterministic marker preservation check and A10's discard with a caveat. The packet now carries the
audience where it carried null, and the marker check is a pure function that can be written and
tested without a model, but the rewrite itself needs one. The corpus does not phrase an audience into
its questions yet either, so `audience_detection` has nothing to measure regardless.

**The visual tie break** (B8.1). `select_type` is a priority cascade with early returns, so no two
rules can be live at once and no tie can arise. Reaching the tie break means restructuring the
cascade to return candidates rather than an answer, which is worth doing when there is a model to
break the tie.

**The figure path and its sanitiser** (B8.4). `select_type` maps a figure hint to a list, and there
is no figure branch in the payload schema, so `sanitise_svg` has two unit tests and no caller. The
sanitiser is testable against a hostile svg fixture set that does not exist; that set is worth
building before the path is, because it is the half that has to be right.

**The image path** (B8.5). No image alias, no Image row, and the output schema's `type` enum does not
include `image` while B8.5 requires it. Doc 06's own B5 has the same omission, so the doc and the
schema need resolving together rather than one being bent to the other.

**Re-visualising on a review edit** (B3). Nothing removes a block from a summary yet, so there is
nothing to rerun on.

**Entity naming, added at M9** (03 section 5). The grounded mock's Router answers `entities: []`, so
the Concept write path built at M9 runs on every card in a sweep and proposes nothing. Naming the
entities in a question is judgment, and this corpus carries no proper noun in any of its 400
questions, so no heuristic can stand in for it. See BN-075.

**Reason** Each is a place where the honest measurement needs a real provider. Recording them here,
with what is already in place for each, is worth more than a half implementation that a mock would
score as working.

---

### BN-068 The board answers now, and the RPC learned to say where a card hangs from

**Spec** 09 section 5, 01 sections 4.1 and 4.4, 12 phase 8.

**Decision** `card.ask` takes `parent_card_id`, `anchor_text` and `anchor_block_ref`, and `Core::ask_on`
takes them as one `Anchor` rather than as three parameters. `card.verify` and `board.rename` are
registered. One delegated click listener in `main.ts` serves every verb on every card.

**What was actually broken.** `render.ts` emitted `data-act="flags"`, `data-act="remove"` and
`data-act="follow"` from M2, and no file in `app/ui/src` ever listened for any of them. The per card
follow-up box, the remove verb and the flag chip were markup. Underneath that the RPC could not have
served two of them anyway: `card.ask` called `Core::ask`, which passes `parent_card_id: None`, so a
follow-up asked from a card would have landed on the board as another root. `applyNotification`
handled two of the bridge's eight notification kinds and dropped the other six without a word.

**The anchor is a struct** because the three fields are one decision. They select the card's kind
between them (parent and anchor is a `branch`, parent alone is a `follow`, neither is a `root`), and
passed separately they let a caller name a span on no card. The RPC refuses that combination rather
than storing a root card carrying a pointer into a visual it has no parent to read.

**Remove is not wired, and its markup is gone rather than left inert.** Doc 09 section 5 has "Remove
card and subtree" and section 5 also says every verb emits a user event. There is no
`card.removed.v1` in `EVENT_VOCABULARY` and no soft delete column on `card`, so the verb needs a
vocabulary entry and a migration. Both belong with board trash, restore and purge in M9 step 3, which
opens the same two files. The header slot it occupied now carries Rerun, which is a verb that works.

**Six notification kinds now do something, and most of them re-read rather than guess.**
`card_answered` and `card_failed` apply live, because that is what stops a card spinning the moment
its answer lands. `card_updated`, `flag_raised`, `flag_resolved` and `board_updated` set a flag that
re-reads the board. Pattern 25 is why: `flag_raised` carries a rule id and a severity and not the
reason the card shows, and `flag_resolved` carries only a card id, so filling the rest in here would
put a string on screen that no event said.

**Reason** Doc 12 phase 8's acceptance is "every verb in 09 section 5 works and emits its event". A
verb whose markup renders and whose listener does not exist meets none of that, and nothing in the
build reported it for four milestones, because nothing drove the UI.

---

### BN-069 The UI is driven headlessly now, and the first thing it caught was a fixture

**Spec** 12 phase 8 acceptance and phase 11 (nightly eval in CI).

**Decision** `tessera-ui-server` serves `app/ui/dist` and one `POST /rpc` over the same router the
Tauri shell registers, so Playwright can drive the real product against a real core. Six tests in
`app/ui/tests/board.spec.ts` cover the verbs M9 step 1 wires.

**Why a server rather than a fixture.** The shell reaches the core through
`window.__TAURI__.core.invoke`, which exists only inside the Tauri webview, so a browser driver finds
`rpc.connected === false` and measures the offline fixture. A test against the fixture would have
passed on every day the click listeners did not exist.

**What it caught immediately.** The first screenshot showed the follow-up card carrying "This card
did not finish", with a toast reading `schema_violation: provider mock returned no usable content`.
`MockProvider::on` queues one response per stage and then falls through to garbage, which is correct
for a test asserting one card and wrong for a server answering many: the second card on the board
found an exhausted script. The fixture now uses a scripted default, which is consulted rather than
consumed.

**The test had the same hole the UI did.** `the follow-up box on a card asks a follow-up` asserted
that a second card appeared and that its title read "Follow-up". A card that renders and fails
satisfies both. The assertion now reads the card's terminal status and its answer, and a seventh test
reads every card's status, every `.failed` body and every error toast on the board, so a fixture that
runs dry fails there first. This is the rule that governs this project applied to a screen: a metric
with nothing to measure must not report a pass, and "a card exists" was measuring nothing.

**Reason** A rendered screen and a working one are different claims, and only one of them was being
made. The server is also the piece doc 12 phase 11 needs to put the UI into a nightly CI run.

---

### BN-070 The two branch popovers, and why the anchor stays in client coordinates

**Spec** 09 sections 3, 4 and 5.

**Decision** One popover element in two states serves both of doc 09's board popovers. The offer
comes first, with the verb the anchor kind calls for, and the question box only once the offer is
taken. That is the prototype's interaction (`Docs/canvas-prototype.html:331`-336) and it is right for
a reason worth stating: a selection made while reading should not be interrupted by a text box.

**Client coordinates, not board coordinates.** The popover lives outside `#world`, so it holds its
size when the board is zoomed. Converting the selection rect into board coordinates and back would
put the popover inside the transformed layer, where its text scales with the camera and a zoomed out
board renders an unreadable one. The plan said this step needed the board coordinate transform; it
does not, and the reason it does not is the same reason the popover is a sibling of the canvas rather
than a child of it.

**The card body is `data-no-pan` now.** A drag inside a card body is a person selecting a span, and
the viewport drag handler was panning the board out from under them. Cards are not draggable yet, so
nothing is lost; when they become draggable the header is the handle.

**A selection spanning two cards is refused rather than truncated.** It names no single card, and a
branch has exactly one parent. A selection in a card header or a follow-up box is refused too: that
is a person reading or editing, not one marking a claim.

**Reason** Doc 09 section 5's Branch verb is two thirds of the acceptance walkthrough's branching
rows, and until now neither the markup nor the RPC could express it.

---

### BN-071 "How this was built" claimed the answer was checked against sources it never had

**Spec** 09 section 4 and 12, 07 section B8.1.

**Decision** The disclosure renders from `board.history`, which had been registered on the core since
M2 and called by nothing. The Verified row counts what `verify.completed.v1` recorded rather than
characterising it.

**What the first version got wrong.** The row said "checked against its sources" whenever the event
appeared. It appears on a fast card too: doc 07 section B8.1 runs the deterministic checks at fast
depth, which is what raises `fast_mode_notice`. A fast card cites nothing, so there were no sources
and nothing was checked against them, and the disclosure said the opposite of the flag two lines
above it. The screenshot showed it; no test did.

The row now reads what the event carries: `checks_run` counted by outcome, and `verdict_counts` only
when there were citations to count, because "0 of 0 citations supported" reads as a failure rather
than as an absence. On a fast card it now reads "6 rules passed, 1 flagged, 4 skipped", which agrees
with the flag chip in the header.

**Cost is stated in tokens.** Doc 09 section 4 names cost. Tokens are what `model.call.v1` records; a
currency figure would need a price the core never saw.

**Read on open, not on render.** The disclosure is closed on most cards most of the time and the
history is the whole board's log, so filling it per card per render would read the same hundreds of
events once per card.

**Reason** The audit trail is the one surface whose whole job is to agree with what happened. A
sentence in it that overstates is worse than an empty disclosure, because an empty one does not
mislead.

### BN-072 The rail, the four pages, and the two verbs that needed no migration

**Spec** 09 sections 3, 5, 6 and 9, 11 sections 5 and 6.

**Decision** A left rail over a page layer that covers the canvas rather than replacing it, so the
board keeps its camera, its cards and any in flight run while a page is open.

**Trash and purge needed no schema change.** `board` already had `status` with an `active` or
`trashed` check and a `trashed_at` column, and `list_boards` already took a status. Doc 09 open
question 1, adopted by doc 11, makes Trash a filter on Home rather than a rail item, so it is the
same grid read with a different word. `board.trashed.v1`, `board.restored.v1` and `board.purged.v1`
were all in the vocabulary with nothing emitting them.

**A purge keeps the events.** The event log is append only and the database enforces it with a
trigger, so deleting a board removes the entities and leaves the trail that says they existed. That
is what makes `board.purged.v1` readable afterwards rather than a claim about rows nobody can check.
The RPC also refuses a purge on a live board: it is two steps, so it is never one click from a board
in use.

**The Flags queue needed no schema change either.** The `flag_open` index in the initial migration
was written for exactly this query, severity then age across every board, and had never been used.
`read_flags` stays as it was: it is per card and feeds the chip. `review.decided.v1` had a projection
handler since M2 and nothing emitting it.

**One event per card, not one per decision.** A bulk decision can span several cards, and the
projection that reopens or closes a card reads the card from the event, so a single event would
recompute one card and leave the rest wrong.

**Rerun and edit leave the flag open.** Only accept and dismiss close it. A rerun that has not
written its new card yet would otherwise take its row out of the queue and leave nothing to come
back to.

**Reason** Doc 12 phase 8's acceptance is every verb in doc 09 section 5 working. Home, Flags,
Library and Profile are where the verbs that do not act on a card live, and none of them existed.

---

### BN-073 A covered element is visible and clickable, so sixteen tests passed over a broken layout

**Spec** 11 section 5.

**Decision** The rail width is one custom property on `body`, read by the rail, by the board's left
padding and by the page's left edge.

**What broke.** Doc 11 section 5 says the rail is 56px collapsed and 240px open. The rail knew that;
the page did not, and started at a hardcoded 56px. Opening the rail put 184px of it on top of every
row on the Flags queue: the checkbox, the severity chip, the rule name and the board heading were all
underneath it.

Sixteen Playwright tests passed. Every one of them was written against visibility and text, and a
covered element is visible, has its text, and is clickable to a driver that scrolls it into view.
The screenshot showed it in a second.

The test that catches it now reads geometry: the row's left edge against the rail's right edge, with
the rail open. That is the assertion shape this class of bug needs, and it is the same lesson as the
build trail two steps ago in a different costume. A rendered screen is not a working one, and now:
a visible element is not an uncovered one.

**A second one from the same run.** `#page` and `#rail` both sat at `--z-sticky`, so the page won on
DOM order and swallowed every rail click after the first. That one a test did catch, because a click
that lands on the wrong element times out.

**Reason** Recording it because the fix is one line and the lesson is not. Two of the four defects
this milestone has surfaced were found by looking at a screenshot after a green suite.

---

### BN-074 What the Profile page can say about a key

**Spec** 10 section 8, 11 section 6.

**Decision** `profile.get` reports, per model alias, which `key_ref` it wants and whether the
keychain has it. It cannot report what the key is, because it never reads one.

`profile.set_key` is the other direction and the only one a secret travels: the value goes to
`KeyStore::set` and nothing writes it to the store, logs it, or echoes it back. The reply is the
key_ref and a boolean. The UI asks for it with `window.prompt`, which is the one input in the product
whose value must not survive anywhere: no element holds it and no state keeps it.

A test asserts the leak rather than the feature: it opens a core whose keystore holds a known secret,
reads the whole profile, and fails if that string appears anywhere in the response. The Playwright
suite does the same against the rendered page.

**Diagnostics is counts, not a verdict.** What the page is for is telling a person whether the thing
they think happened happened, and a green tick summarising six numbers hides the one that is wrong.

**Reason** This is the surface that retires the `tessera-keys` CLI, and it is the surface where the
standing constraint is easiest to break by accident.

### BN-075 The Concept graph writes, and the Planner packet stops carrying an empty array

**Spec** 01 sections 4.10 and 4.11, 04 section 4, 09 section 9.

**Decision** A card that answers proposes the entities its Router named as Concepts, links each to the
card with `mentions`, and reuses a term the profile already holds rather than duplicating it.

**What this closes.** Doc 04 section 4 gives the Planner a `concepts` array. It has been `[]` on
every run since M5, with a comment saying so, and entity resolution degraded to literals marked
`unknown` exactly as doc 04 says it should when the graph is empty. The Router has returned entities
since M4 and they reached the log and nothing else. Both halves existed and nothing joined them.

**Reuse is the point, not an optimisation.** Doc 01 section 4.11: "two boards that both cite the same
Concept share it, which is the mechanism behind when two boards touch PSD3 they touch the same node".
Matching is case insensitive on the canonical spelling. An alias pass belongs with the Concept editor,
not with a write path that runs on every card.

**A failure here is not the card's failure.** The answer is written and verified before this runs, so
a graph that missed a term is a Library with one fewer row rather than a card the reader loses. The
Planner's read degrades the same way, to the empty array it carried before.

**No `concept.rejected.v1` was invented.** The vocabulary has proposed, confirmed and linked. A
rejection sets the link rows to `rejected`, which is the status doc 01 section 4.11 already gives
them, and `concept.linked.v1` already said those links existed.

**A correction to an earlier note.** `builds_on` on `concept_link.relation` and the `learn_session`
table both already exist: the first landed in migration 0002 and the second in 0001. Two earlier
readings called them missing, both because the search stopped at Rust or at the initial migration.

**The grounded sweep does not exercise this, and that is recorded rather than papered over.** The
sweep ran clean after the change, 28 of 36 with nothing below threshold, and that number means
nothing about this path: the grounded mock's Router answers `entities: []`, so 400 cards proposed
nothing and "unchanged" was a report about code the run never entered.

The first fix attempted was a capitalisation pass over the question. It produced "What" and "How",
because this corpus asks "what is the model validation interval for a systemically important
institution" and carries no proper noun anywhere in its 400 questions. A template pass would have
returned whatever the generator's templates were written with, which measures the template. Either
would have put terms in the Library that nothing observed, and a mock that answers plausibly is worse
than one that answers nothing.

So it stays `[]` with a comment saying why. Naming entities is judgment, which is the one thing the
grounded mock's own doc comment says it cannot measure, alongside phrasing and conflict resolution.
The graph is measured by the end to end tests and needs a live provider at scale. It joins the five
in BN-067.

**Reason** The Library's Concepts tab reads a table nothing wrote, and a tab that can only ever be
empty is a screen that lies about what the product does.

---

### BN-076 Contrast is measured from what the renderer painted, not from the palette

**Spec** 09 section 14, 11 section 10.

**Decision** The contrast check walks every text bearing element on screen, reads its computed colour
and the first opaque background behind it, and computes the ratio. It runs over the board and over all
four pages, at 4.5:1 for text and 3:1 for large.

**Why not read the tokens.** The palette is OKLCH and the WCAG ratio is defined on sRGB relative
luminance. A number computed from the tokens would be a colour space conversion this repository got
right or wrong with nothing to check it against, and a contrast check that is itself unchecked is the
kind of green tick this project has been burned by. `getComputedStyle` asks the renderer that will
actually paint it.

Every element passes today, so the check reports a pass it earned. It will earn a failure the first
time a token moves.

**The rest of doc 09 section 14, and what each needed.** Keyboard reach is measured by tabbing from
the top of the document and collecting what receives focus, rather than by asserting that a button
exists. Flag rows are their own focus stop with arrow, Home, End and Space, so a reader moves between
rows instead of tabbing through four verbs to reach the next one. Reduced motion is checked by
opening a context that asks for it and reading `animationName` and `transitionDuration` back.

**Reason** Doc 12 phase 8's acceptance names keyboard reachability and contrast checks by name, and
both are the kind of claim that is easy to assert and hard to earn.

---

### BN-077 The canvas has a document, and a third z-index collision

**Spec** 11 section 10.

**Decision** The board renders as a document from the same `Card[]` the canvas does, so the two
cannot disagree. Parents before children, the anchor a branch came from stated rather than drawn, and
the visual described by type and block labels rather than drawn at all.

**One copy in the accessibility tree, not two.** The canvas is `aria-hidden` while the document is
open and the document is hidden while the canvas is. Rendering both would put every card in the tree
twice, which is worse for a screen reader than either alone.

**The third z-index collision this milestone.** `#reading` sat at `--z-sticky` alongside the title
bar, so it covered the control that opens it and there was no way back to the board. The rail and the
page layer had the same collision in step 3. Three occurrences is a pattern rather than three
mistakes: this shell has four fixed layers and one token, and the next one to be added should be
given its own level rather than borrowing `--z-sticky` because the neighbour did.

**Reason** Doc 11 section 10 asks for a list view alternative reachable from the title. A canvas is a
spatial arrangement and a screen reader has no way to convey one.

---

### BN-078 IBM Plex is bundled, and the CSP needed no widening

**Spec** 11 section 2, 10 section 8's no remote resource rule.

**Decision** Four faces are declared in `app/ui/src/styles/fonts.css` and their files come from
`@fontsource` in node_modules. Vite rewrites each `url()` into a fingerprinted asset served from the
app's own origin, so `default-src 'self'` already covers them and no `font-src` was added.

Four faces rather than the package's full set: Sans 400 and 600, Sans 400 latin extended for the
European issuer names doc 02 plants in the corpus, and Mono 400 for rule ids and locators. That is 77
KB. `font-display: swap`, so a cold start shows text in the fallback rather than showing nothing.

**Reason** `index.html` has carried a comment since M2 saying the fonts arrive at M9. Until now the
stack fell through to the system sans, so the prototype's typography was approximated rather than
shown.

---

## Measured findings

### BN-056 The three staleness gates, measured at last

**Measured** 2026-08-27, corpus 0.3.0-42 built at T1 and T3, grounded mock, no model spend. The
question sweep is 400 questions at T1, results at `eval/results/42/grounded/run-1787801147`. The
re-verification reads 148 cards back at T3 against the T1 tree as its baseline, results at
`eval/results/42-T3/grounded/run-1787800768`.

| Gate | Threshold | Result |
|---|---|---|
| `staleness_detection` | 0.95 | 1.000 |
| `stale_propagation` | 0.95 | 1.000 |
| `stale_ancestor_reverification` | 1.0 | 1.000 |

Neither run has a measured metric below its threshold. The T1 sweep measures 24 of 36 metrics and
the T3 re-verification 9, and the two do not overlap much on purpose: a re-verification answers
nothing, so every metric about answering reports n/a there.

**What the denominators are.** Worth writing down, because all three are small and a reader who
assumes otherwise will misread the next run. `staleness_detection` is 2, the two cards that state
a superseded fact; no question in the 400 requires one, so the question sweep can never measure
this metric however well it retrieves. `stale_propagation` is 2, the ends of doc 15's stale chain.
`stale_ancestor_reverification` is 10, the follow ups whose parent turned out to cite a stale
source. Five of 28 cited sources went stale at T3: two changed content, two stopped resolving, one
was superseded.

**What is not yet measured.** The dependent end of the stale chain cites `reg-car3-v1` itself as
well as building on the origin card, so it would be flagged whether propagation works or not. The
metric passes on a card that has two reasons to be flagged, and separating them needs a fixture
where the dependent cites nothing stale of its own. Recorded rather than fixed, because changing
the corpus to suit the metric is the wrong order.

### BN-047 The three defects the M6 plan predicted, closed

All three were found while planning M6 and recorded before they could bite. Two had become live
by the time retrieval worked.

**`boards` was in neither shipped pack.** The Planner only assigns retrievers a pack enables, so
memory would never have run whatever `Profile.memory_enabled` said, and all three of doc 15's
metrics would have reported `n/a` forever while looking wired. Both packs now list it, and both
gained the `own_card` trust rank doc 05 section 8.5 fixes at 5 in finance, below every external
class. In the general pack `user_supplied` moved from 5 to 6 rather than sharing a rank with
`own_card`, because a tie makes the hierarchy non-deterministic exactly where its job is to
decide.

The schema guard caught the pack change before any test did: `doctrine-pack.v1` did not admit
`boards` as a retriever id. That is the guard doing precisely what doc 12 principle 1 asks of
it, on a change nobody thought needed validating.

**`citation_accuracy_ledger` would have called M6 a regression.** It counts verdicts equal to
`supported`, and every verdict is `unchecked` until the support check lands at M8. The
denominator was zero while retrieval did not exist, so it reported n/a honestly; the first run
producing citations would have turned that into 0.000 against a 0.95 threshold. Now gated on
`support_check_enabled`, the same way `verifier_agreement` already was.

This is BN-019 for the fourth time: a metric with nothing to measure must report n/a, never
zero. Four occurrences in one project is no longer a slip, it is the default failure mode of
writing a scorer before the thing it scores exists.

**Scanned pdf recall is deferred to M10 with the Reader.** Doc 05 section 12 asks for 0.70 and
doc 05 section 8.2 routes scanned pdfs through "the Reader's OCR path", which doc 12 phase 9
builds. The parser already reports `NeedsOcr` rather than a generic failure so the Profile can
say what the file is waiting for, and the corpus's one scanned pdf is correctly classified.

### BN-048 A card is remembered when it is answered, and forgotten when it stops qualifying

**Spec** 05 section 8.5: the boards index is "updated on `card.answered.v1`". Doc 15 section 3:
"Only verified cards remember: done, deep or research, no open block flags, board not trashed."

**Decision** Eligibility is one SQL query rather than four Rust conditions, evaluated where the
data is. Four clauses checked in code are four clauses that can drift apart, and three of them
are about rows in other tables.

**The half that is easy to miss.** Eligibility can stop being true after the fact. A flag is
raised, a board is trashed. So a card that fails the check is actively removed from the index
rather than merely not added to it, which is why `index_card` returning false also forgets. The
test that matters is the trashed board: a user throws work away and its cards keep answering
questions, which would be the product ignoring a deletion.

**What is indexed** is a digest carrying the card's own citations, per doc 05 section 8.5 and
for the reason doc 15 section 2 gives: a new card's numbers must cite the original passage,
"which the boards passage carries in its digest". A digest without them is a dead end, and
citing it is exactly the loop the memory rule exists to prevent. A card that cited nothing says
so in its own digest.

`builds_on` is collected in the fan-out rather than derived downstream. The Synthesizer's packet
carries a trimmed source per doc 06 section A4 with no locator in it, so the only place that
knows which prior card a passage came from is the place that fetched it. The first version read
it downstream and silently produced an empty list.

### BN-044 A fact's label now identifies the fact

**Fixes** BN-041, where 506 labelled facts drawn from 49 labels left "the model inventory review
interval" carrying 24 different true values and every question about it a coin toss.

**Decision** Numeric and date facts draw a qualified label, and the pairs are drawn without
replacement so uniqueness is guaranteed rather than hoped for. The qualifier scopes the
requirement the way a regulation does: "the capital conservation buffer for a systemically
important institution under the standardised approach". Eight scopes of who and six of what,
used alone or combined, which is fifty-six qualifiers against four to six base labels per
domain, and the pool asserts at construction that it can cover the worst case.

**Left alone on purpose.** Definitions, because there a term and its meaning travel together, so
a repeated definition is redundant and never contradictory. Supersession and false plants,
because they copy their source fact's label deliberately. After the fix, the only numeric and
date labels carrying more than one value are exactly those two cases: 62 of 225, every one of
them a v1 and v2 pair or a planted misquote. That is the ambiguity the corpus is supposed to
have, and now it is the only ambiguity it has.

Distinct labels went from 49 to 255.

**Measured effect on retrieval**, same index and same model, before and after:

| | before | after |
|---|---|---|
| recall at 1 | 0.374 | 0.544 |
| recall at 3 | 0.445 | 0.608 |
| recall at 12 | 0.545 | 0.647 |

Recall at 1 improved by nearly half, which is the number that matters most: when the retriever
finds the passage now, it usually finds it first.

**Where that leaves the gate.** Doc 05 section 12 wants 0.90 local and 0.95 regulatory. At 0.647
the corpus is now sound and the remaining gap is genuine ranking difficulty. The ceiling at
unlimited depth is 0.826, so about a fifth of lookups are never matched by either half and the
rest is ordering. Doc 05 section 8.1 already names the missing stage: "a small alias rerank of
the top 20". It was written as an optimisation and the numbers say it is load bearing.

Mixed qualifier lengths were deliberate. A corpus where every label carries the same long
qualifier makes retrieval trivially easy in a way that flatters the ranker.

### BN-045 A bumped value has to actually move

**Found** by `test_a_false_plant_misquotes_a_real_fact` immediately after BN-044 changed which
values get bumped. A false plant had exactly the value of the fact it was planted to misquote.

`_bump` chose `max(0.5, current - step)`, which returns the original whenever the subtraction
would go under the floor. At a current value of 0.5 with a step of 0.5 the "wrong" value is the
right one.

Two things this quietly broke. A false plant identical to the fact it misquotes traps nobody, so
doc 02 section 5.2's forbidden fact rate was measuring a case that could not fire. And a v2
identical to its v1 is not detectably stale, so doc 02 section 5.4's staleness scenario had the
same hole.

The bump now goes up whenever going down would hit the floor, so the value always changes.

Worth noting how it surfaced: the test was already there and already correct, and it took an
unrelated change to shift the random draws far enough to hit the case. A latent bug in a
generator is only as visible as the seed makes it.

### BN-046 Two ranking tweaks that did not work

Recorded because both are obvious enough that someone will try them again.

**Stopword removal.** The theory was that OR-ing every term including "what" and "the" floods
the candidate set. Measured: recall fell at every depth except k=1, losing 0.026 at k=25.
FTS5's bm25 already discounts a common term by inverse document frequency, so the words cost
nothing and removing them threw away what little signal a stopword carries in a templated
question. It would also have added a per language word list to maintain.

**Phrase matching as a third fusion list.** The theory was better founded: after BN-044 the
corpus is full of passages sharing ten qualifier tokens and differing in two, and a phrase only
matches where words are adjacent. Measured: identical at k=12 and slightly worse at k=1, 0.544
against 0.529.

Both reverted. Two query-side tweaks failing to move the number is itself the finding: the gap
is not in how the query is written, and the reranker doc 05 section 8.1 specifies is where to
look next.

### BN-041 Six hundred facts drawn from forty nine labels, and what that does to every recall gate

**Spec** 05 section 12 sets retrieval recall at 0.90 local and 0.95 regulatory. Doc 02 section
10.2 sets fact recall at 0.85 deep and 0.92 research.

**Measured** on the first full retrieval probe. Hybrid retrieval returns the required fact in
the top twelve for 0.545 of lookups. The index is not the problem: every one of the 180 required
facts is present in some indexed chunk, so coverage is 1.000 and the whole gap is ranking.

**Found while asking why ranking was that bad.** The corpus has 506 labelled facts drawn from
**49 distinct labels**. "The model inventory review interval" carries 24 different values, all
marked `truth: true`: 16 months, 17, 10, 18, 4, 21, 13, and so on. Seventy percent of required
facts share their label with five or more others, and the median required label is stated in 24
separate chunks.

So the question "What is the model inventory review interval?" has twenty four equally true
answers in the corpus, and nothing in the question says which one is meant. No retriever can
choose, and neither can a Synthesizer downstream, which means this defect was going to surface
at the fact recall gate too whatever happened here.

This is not doc 02 section 5.2's planted contradiction. Those are deliberate, marked with an
`edge_case_id`, and there are a handful. This is the fact generator drawing from a small pool of
subject labels while producing six hundred independent facts, so a label identifies a topic and
never a fact.

**Consequence for the gates.** Doc 05 section 12's recall numbers are not measurable on this
corpus as written, and neither is doc 02 section 10.2's fact recall. A retriever that returns
every passage stating the label has done its job; picking the one the ledger happens to name is
not a skill, it is a coin toss with twenty four sides. The deliberate contradiction cases are
also diluted past usefulness: one planted disagreement is indistinguishable from twenty three
accidental ones.

**The fix is in the generator, not the retriever.** A fact's label has to identify the fact.
Either the label pool grows to the order of the fact count, or a label is qualified by what
distinguishes it, which is what a real regulation does: intervals differ by model class,
buffers by institution tier. Until then every retrieval and recall number on this corpus is
measuring corpus ambiguity rather than the product.

Recorded before fixing, because the numbers already gathered are only interpretable against it.

### BN-042 A fixed candidate depth is a ceiling that looks like a result

**Found** while reading the recall curve. Recall climbed with the requested window and then
stopped dead at 0.697 from k=100 onward, which reads as the ranker running out of relevant
passages.

It was `CANDIDATE_DEPTH`, a constant of 60 in the search path, capping how many candidates each
half of the hybrid produced before fusion. Every request above sixty returned sixty. The
plateau was the constant.

Two things wrong, one measurement and one product. The measurement drew a ceiling that was an
artifact. The product silently truncated: a caller asking for a hundred passages had no way to
know it got sixty, and doc 05 section 4's packet lets a caller ask for whatever it needs.

The depth is now the floor or the caller's limit, whichever is larger. With the cap gone the
true lexical ceiling is 0.795 and hybrid reaches 0.832, so the vector half is finding passages
the lexical half never matches, which is the thing the two halves exist to do for each other.

`asking_for_more_than_the_candidate_floor_actually_returns_more` is the guard.

### BN-043 Removing stopwords made retrieval worse

**Tried** on the theory that OR-ing every term, including "what" and "the", floods the candidate
set with passages matching on nothing.

**Measured** against the same corpus and question set: recall fell at every depth except k=1.
At k=25 it lost 0.026.

**Reverted.** FTS5's bm25 already discounts a common term by its inverse document frequency, so
the words were costing nothing, and removing them threw away the small amount of signal a
stopword still carries in a templated question. The change would also have added a per language
word list to maintain, for a measurable regression.

Recorded because the idea is obvious enough that someone will have it again.

### BN-040 The embedding model runs on candle, not on an ONNX runtime

**Spec** 10 section 3: "Local small model (e.g. a bge or nomic class model via candle) by
default". Doc 10 section 17 question 2 leaves the model itself to be settled on the synthetic
recall numbers, which this milestone measures.

**Decision** `candle`, with `intfloat/multilingual-e5-small` behind an `Embedder` trait.

**Why not the easier option.** `fastembed` is one call where candle is about a hundred and
fifty lines of model plumbing, so it was tried first. It builds. Nothing that links it does:
every executable failed at the linker with `unresolved external symbol __std_find_trivial_8`,
which is a Microsoft standard library symbol that the prebuilt ONNX Runtime binary expects and
the Visual Studio 2019 build tools on this machine do not provide. The library compiled and the
test binary did not, which is the kind of failure that looks like a fluke until you try to ship.

The three ways out were to make the user install a newer Visual Studio, to load the runtime
dynamically and ship a DLL beside the app, or to use a pure Rust model runner. The third is
what doc 10 already named, needs no native toolchain at all, and leaves nothing extra to sign
or notarise at M13, where doc 12 phase 11 has to produce a signed msi and a notarised dmg.

**Multilingual on purpose.** The corpus carries Dutch documents and a real user's folder is
under no obligation to be in English. An English only model does not fail on Dutch text, which
would at least be visible. It embeds it into a region of the space that means nothing, and the
only symptom is a recall number that is slightly worse than expected for reasons no breakdown
shows.

**Two details that decide whether the vectors mean anything.** The e5 family is trained with
`passage:` on the indexed side and `query:` on the asking side, and without those prefixes the
neighbourhoods are simply wrong rather than absent. And pooling averages only the unmasked
tokens: counting padding drags every short passage toward one point, which is worst for exactly
the short factual passages this corpus is made of.

Weights are read rather than memory mapped, because the mmap constructor is `unsafe` and the
workspace forbids unsafe outright. It costs one copy at startup and nothing afterwards.

### BN-037 Format is decided by the bytes, not by the file name

**Spec** 05 section 8.2 lists the formats the local retriever parses. It does not say how a
file's format is determined, because on paper that is what the extension is for.

**Found** on the first run of the parsers against the real corpus. One document, planted as a
scanned pdf by doc 02 section 5.3's `scanned_no_text_layer` transformation, was sitting on disk
as `int-model-risk-09.docx` and beginning with `%PDF-1.3`. The docx reader opened it, found no
zip, and reported the file damaged.

Two things were wrong and both are fixed.

**The generator.** `mess.py` set `doc.format = "pdf"` and left `doc.path` alone, so the write
step produced a pdf under the name the document had before it was scanned. That is not the edge
case the transformation plants; it is a file whose name lies for no reason. The path now moves
with the format.

**The parser.** It trusted the extension, which is a reasonable thing to do exactly once, in a
corpus you generated yourself. A watched folder belonging to a real person is full of files
renamed by hand, exported with the wrong suffix, or saved by a tool with its own opinion, and a
retriever that declares those unreadable is wrong about its own job. `parse_file` now reads the
first four bytes: `%PDF` decides pdf, `PK\x03\x04` opens the archive and looks for `word/` or
`xl/` to tell docx from xlsx, and the text formats fall back to the extension because they have
no signature to find. The bytes win when the two disagree.

The corpus bug is the smaller half of this. The parser would have been wrong on real folders
whether or not the generator ever made a mistake, and nothing in the synthetic corpus was ever
going to reveal that on its own.

### BN-038 A rebuild starts from a clean tree

**Found** immediately after BN-037. Renaming the scanned document left the old `.docx` on disk,
because `write_corpus` created the output directory with `exist_ok=True` and wrote over it.

The orphan is worse than untidy. Nothing in `documents.jsonl` mentions it, so no metric expects
it, and the local retriever would have indexed it anyway and ranked its passages against real
questions. A corpus that accumulates files nobody declared is a corpus whose recall numbers
drift for reasons no diff can show.

`write_corpus` now removes the tree before writing it. The determinism check already used fresh
directories, which is why two builds agreed while an incremental rebuild was quietly wrong.

### BN-039 A protected file says it is protected

**Spec** 05 section 10 lists `parse_error` as "skip file, record in index errors", and doc 05
section 11 puts those errors on the Profile's Retrievers page where a person reads them.

**Decision** The pdf reader checks the document's own header for an encryption dictionary rather
than inferring from the extractor's error text, which says only that parsing failed.

**Why** The two failures need different sentences. "This file is protected and was not opened"
is something the reader can act on. "This file is damaged" sends them looking for a corruption
that is not there. Only the header region is scanned, so the string `/Encrypt` appearing inside
a content stream is not mistaken for a declaration.

The scanned pdf is treated the same way: `NeedsOcr` rather than a generic failure, naming the
Reader at M10 as what it is waiting for.

### BN-036 The domain taxonomy is retired as a load bearing judgment

**Spec** 03 sections 7, 8.1 and 12; 02 section 10.2; 12 phase 4. The Router classified each
request into a pack domain, a keyword pre pass could decide it outright, and domain accuracy
0.90 was an acceptance gate.

**This is a deviation from the spec set, decided by the owner on 2026-08-25.** Verbatim: "lets
not limit this and restrict retrieval gates on domain. Keep it free. The purpose of this is to
build quality responses not limit them this way." And on the eval: "The new questions must be
varied and not just stupid finance related questions. we were restricting a bit too much."

**What forced the question.** Two paid sweeps and one trace.

The first sweep gave the model four bare domain names; the bulk model answered `capital`, the
first name, for most of what the keyword pass left to it. Domain accuracy 0.585. The second
sweep listed each domain's pack vocabulary in the prompt; the wrong-domain guessing collapsed
(165 cross domain misses to 6) and the model instead answered `unknown` for everything the
vocabulary missed. Domain accuracy 0.495. The vocabulary will always miss, because nobody can
enumerate what users will ask, and on a synthetic corpus the invented terms are not in any
model's world knowledge at all.

The trace was worse than either number. Every consumer of the label was wired to the same
outcome: the finance pack hinted `deep` for all four domains and the general pack has one
domain and no hints, so the taxonomy's distinctions changed nothing. And the planner joined
the regulatory retriever only when the domain was not `unknown`, so the model's honest
uncertainty silently stripped a card of its ranked source. The gate measured a judgment with
no consequences except a harmful one.

**What replaces it.**

1. **One binary judgment: `regulatory_stakes`.** Does the answer turn on a rule, threshold,
   date or obligation the reader might act on? Any model answers this in any domain without
   being taught vocabulary. It drives depth through the pack hint `depth_hints.regulatory_stakes`
   and defaults to true whenever unstated, because care on a casual question costs seconds and
   casualness on a consequential one costs a wrong number acted on.
2. **Retrieval is ungated.** Every enabled evidence retriever joins every sub-question. The
   Synthesizer weighs what comes back by trust rank, which is where source preference belonged
   all along. Structured remains signal driven because it is a query against a registered
   table, not a search, and it alone moves `value_policy`.
3. **The domain label survives as an observation.** The free keyword pass labels what it can
   prove; everything else is `unknown`, and `unknown` gates nothing. Scored as
   `domain_label_precision`, reported and never gated.
4. **The gate becomes `stakes_accuracy` at 0.90**, measured on a new breadth question set: 60
   hand written questions across 29 fields, half consequential (medication doses, lease notice
   periods, drone registration, cookie consent) and half plain understanding (why the sky is
   blue, how sourdough works). The finance corpus could not measure this judgment, because
   every question in it is consequential by construction, so a model answering true always
   would have scored perfectly.

**Also found on the way.**

*Obligation questions were contentless.* Obligation facts carried no subject label, so their
questions fell back to "the requirement" and 28 of 400 read "What is the obligation on the
requirement?", which nothing can classify. Obligation facts now carry `the duty to ...` labels
and the templates use them.

*The eval leaked the answer.* The runner created each question's board with `default_depth`
set to the expected depth, and the board default is the baseline the Router's recommendation
starts from. Every route accuracy number measured before this note was inflated by that leak.
Boards are now created at `fast` and every raise has to be earned, so route accuracy from
generator 0.2.0 onward is not comparable with the two sweeps above, and it is the honest
number of the three.

*`no_retriever_enabled` counts evidence.* A profile whose only retriever is its own memory can
corroborate itself and learn nothing, so boards does not count toward having something to plan
with.

**Cost of the two sweeps that bought this.** About ten dollars. The first localised the
blindness, the second proved the mechanism and exposed the dead taxonomy. Generator bumps to
0.2.0.

### BN-035 The first live Router gate: two of three targets pass, and why the third failed

**Spec** 03 section 12 and doc 12 phase 4: route accuracy 0.85, domain accuracy 0.90, override
compliance 1.00.

**Measured** 2026-08-25, 400 questions, `finance-eu-synthetic`, corpus 0.1.0-42 at T1. Bulk on
Moonshot (kimi-k2.6 small and medium, kimi-k3 frontier), a 9 question reference sample on
Anthropic, 3 per depth. Results kept at `eval/results/42/kimi-bulk/run-1787660259`.

| Target | Measured | Verdict |
|---|---|---|
| route_accuracy 0.85 | 0.869 | pass |
| override_compliance 1.00 | 1.000 | pass |
| domain_accuracy 0.90 | 0.585 | fail |

**Diagnosis** The failure decomposed cleanly into three facts.

1. The deterministic keyword pass was right 129 times out of 129, and it fired on 32 percent of
   questions. Every failure came from the questions it left to the model.
2. On those, the bulk model was right 38 percent of the time, and 118 of its 165 misses were the
   same answer: `capital`, the first name in the domain list. The reference sample went 9 for 9,
   which is too small to prove anything except that the prompt was answerable.
3. The prompt gave the model the four domain names and nothing else. The pack's
   `domain_vocabulary`, the very list that made the keyword pass perfect, never reached it.

So the gap was not model quality first. It was an information asymmetry the Router built: its
deterministic half had the vocabulary and its model half did not.

**Change** The classify prompt now lists each domain with its vocabulary, tells the model the
terms characterise rather than gate ("a question can be about a domain without using any of
these words"), and passes a multi-domain keyword tie on as a narrowed field instead of
discarding it. `keyword_match` became `keyword_candidates`; a single hit still decides outright,
per 03 section 8.1. The mock provider now records prompt text, and an end to end test pins the
vocabulary's presence in the route prompt.

**Rule applied** Doc 12's regression rule: any change to a classification prompt reruns the 400
question set. The rerun is the measurement of this change; the numbers above are the baseline it
is judged against. Note for that comparison: the question set regenerated between the runs
(BN-033 pins the held out fact into the root pool, which reshuffles downstream draws), so the
comparison is aggregate against aggregate, not question by question.


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
