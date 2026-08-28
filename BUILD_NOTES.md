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

### BN-079 The Exercise agent, and the gate it should have had for four milestones

**Spec** 08, 12 phase 9.

**Decision** The Exercise agent is built, and `exercise_traceability` has the threshold doc 08
section 12 and doc 12 phase 9 both give it.

**The defect the plan named, in full.** The metric computed from the day it was written, gated on
`manifest["exercise_enabled"]`, and had no entry in `THRESHOLDS`. The moment the flag flipped it
would have produced a number, looked measured, and been gated by nothing. `visual_fidelity` had the
same shape at M7 and went unreported for as long as nothing drew a visual.

The fix is structural rather than one line. `READOUTS` now names the ten metrics that describe a run
rather than judging it, each with a reason, so the classification is **total**: every metric is
gated, deliberately ungated, or a readout, and a guard test fails on any metric in none of the three.
Ten metrics were in none of them. A second guard fails on a metric in two.

`reader_structure_recovery_mess_f1` sat in `NO_THRESHOLD` with nothing producing it, which reads as a
degraded scan path that is covered and reported. It is removed until the Reader writes one, and a
third guard now fails on any exemption or readout with no metric behind it.

**The scorer re-checks rather than trusts.** The agent runs doc 08 section 5's two rules and drops
what fails, so scoring its output against its own check would report 1.00 whatever the check did.
`gen/src/tessera_gen/harness.py` carries a second implementation that reads the persisted exercise
and the persisted cards, and a test proves that second implementation can fail: an answer the card
does not state, a card outside the scope, a citation ordinal the card does not have, and a distractor
that is true on another card.

**A one word distractor is a word, not a statement.** Checking every short option against every other
card would drop "yes" and "no" from any board containing either, so the leak check skips options
under three words. Recorded because it is a deliberate hole and a small one.

**Measured.** 30 of 37 metrics on the grounded sweep, up from 28 of 36, with nothing below threshold.
`exercise_traceability` 1.000 and `exercise_distractor_leakage` 0.000 over 36 items across 12 boards,
with 3 further boards correctly reporting doc 08 section 10's `no_eligible_cards`. The sweep samples
five boards per worker and says so, because "traceability 1.00" over 15 boards and over 200 are
different claims.

**What still needs a live provider.** Doc 08 section 12's fourth line, "item answerable from the
source card by a second model with only that card as context", needs two real models. The grounded
mock quotes cards; it does not judge whether a question is worth answering.

**Reason** Doc 12 phase 9's acceptance is "exercise traceability 1.00", and until now there was
neither an agent to measure nor a gate to measure it against.

---

### BN-080 Two shapes the schema guard caught before a reader did

**Spec** 12 principle 1, 06 section A7, 08 section 4.

**Decision** Recorded because both were caught by the boundary rather than by review, which is what
doc 12 principle 1 built the guard for.

**Findings are objects, not strings.** Doc 08 section 4's packet example writes `"findings": []` and
does not say the element type. The packet schema guessed `string`, and the guard rejected the first
packet carrying a card with findings: the stored shape is doc 06 section A7's `{text, citations}`. The
schema, the agent and the scorer all read the same shape now.

**Every output carries its envelope.** The agent returned `title` and `items` and no
`schema_version`, `agent_id` or `run_id`, and the output schema rejected it on both return paths
including the empty one. An agent that answers nothing still has to say who it was.

**Reason** Two boundaries, two catches, no reader involved. Worth writing down as evidence that the
schema first principle pays rather than as two mistakes.

### BN-081 The Reader, and the half of it a mock can measure

**Spec** 07 part A, 12 phase 9.

**Decision** The Reader is built. Its deterministic half is fully exercised on the mock; its vision
half is measured on a live run and nowhere else, and the metric says so rather than reporting a
number the fixture wrote.

**What is deterministic here, and why it is separated.** Doc 07 section A6 makes preprocessing,
structuring and summarising deterministic and only recognising a model call. So the injection check,
the mapping into the Synthesizer format, the traceability rule and the confidence are pure functions
with unit tests, and the vision call is one seam. That split is what lets doc 07 section A12's
"injected image text obeyed 0 times" and "summary values traceable to structure 1.00" be tested for
nothing while "structure recovery F1 0.80" waits for eyes.

**The injection check is about the address, not the verb.** A table that says "ignore rows below the
double line" is a table; one that says "ignore your previous instructions" is an attack. What
separates them is whether the sentence is speaking to a model, so the list is of phrasings that
address one. Doc 07 section A10 continues with the block excluded rather than dropping the image: one
sentence written on a page must not destroy a reader's diagram.

**Confidence does not score an unmeasured term as full marks.** Doc 07 section A9 has three terms and
one of them is OCR agreement with a local pass that does not exist. Its weight goes to the terms that
are measured rather than being counted as perfect agreement, and a test asserts a clean picture with
nothing recovered does not read as confident. Scoring an absent term as a pass is the shape of
dishonesty this project keeps finding.

**`reader_structure_recovery_f1` is advisory on a mock, and no `--read` eval leg was built.** A mock
has no eyes. A fixture that returned the structure the corpus recorded as `sketch_truth` would be
scoring this repository against itself, and a fixture that returned anything else would measure the
fixture. Either way the number would be about the harness. The eval also imports no ink, so a read
leg would have had to invent its own subject. What it would have produced is one meaningless number
and one that the Rust tests already assert more directly, so it is not built. The metric joins
`visual_type_match` and `verifier_agreement` on the advisory list, gated the day a vision run sets
`reader_enabled`.

The note on `reader_enabled` said "the Reader arrives at M10". It has arrived, so it now names what
it actually waits for.

**Reason** Doc 12 phase 9's second half. The vision entry point exists, and the part of it that can
be held to a standard for free is held to one.

---

### BN-082 The sketch raster path, and one test that earns its keep

**Spec** 12 phase 9, 07 section A6.

**Decision** Ink strokes rasterise to a greyscale png, cropped to what was drawn, bounded at the
vision alias long edge, and the ink survives it.

**Cropped, because a sketch in the corner of a large board should not hand a vision model a page that
is nine tenths empty.** One scale for both axes, because a drawing stretched on one of them is a
different drawing. Greyscale, because ink is one colour and a vision model gains nothing from three
channels of the same number.

**The test worth naming** is `the_ink_actually_lands_on_the_page`. It decodes the png back and counts
dark pixels. The failure it guards is a rasteriser that encodes a valid, empty image: every other
check in that file would still pass, and the only symptom downstream would be a vision model
reporting `unrecognised` for a picture that really was blank.

**Reason** Doc 07 section A2 has the Reader read an Image row and doc 07 section A4's packet carries
a `blob_ref`. Something had to turn the strokes a person drew into a picture, and doc 12 phase 9
names it.

---

### BN-083 A paste rule that read correctly and would have blocked every paste

**Spec** 07 section A3.

**Decision** An image on the clipboard is read. A paste that also carries text, into a box that takes
text, stays text.

**What the first version did.** It checked the focus: a paste into an input or a textarea is text,
whatever else is on the clipboard. That reads as obviously right and is wrong in practice, because
`boot` focuses the composer, so the composer always has focus and no image would ever have been read
at all. The rule now asks whether there is text to prefer rather than whether a text box is focused.

**Two rounds were spent on a fixture rather than the product.** The test pasted a base64 string that
looked like a png and was not one: its chunk table walked off the end, `createImageBitmap` refused
it, and the page correctly reported that the image could not be read. The fixture is generated now
rather than remembered, and the comment says why.

**Reason** Recorded because the first defect is the interesting one: a guard can be correct about the
case it names and wrong about every case that occurs.

### BN-084 The Tutor decides and never answers, and four rules stand between the two

**Spec** 14 sections 1, 3.5 and 5.

**Decision** The Tutor writes no card content and retrieves nothing. It chooses what to ask and what
to open next, and doc 14 section 3.5's four rules are deterministic checks over what it decided,
each one dropping the part that broke it rather than failing the turn.

**Why dropping rather than failing.** A turn is a conversation, and a learner who asked a question
and got an error learned nothing. A turn that loses its `open` because the tutor asked for two cards
at once still carries its reply, and the caveat says what went. The Verifier fails closed because a
card is a claim; a tutor turn is not a claim, so it does not.

**Two of the four rules are the Exercise agent's, reused rather than rewritten.** A check item is an
Exercise item with a single card scope, which doc 14 section 1 says in as many words, so
`exercise::traceable` and `exercise::leaks_truth` are called on it directly. A second implementation
of traceability would be a second definition of it, and the two would drift.

**The load bearing rule is the fourth.** A tutor reply carrying `[1]` claims a source for a sentence
nobody verified, inside a product whose entire argument is that a marker means the Verifier stood
behind it. That rule has both a unit test and a Playwright test, at the two ends: one over the turn,
one over what the learner reads on screen.

**Reason** Doc 14 section 1: "Learn mode adds no new answer path." Everything the tutor could have
been given to do that a card already does was given to the card.

---

### BN-085 A session that renders intake and cannot leave it

**Spec** 14 section 3.4.

**Found** while writing the Playwright pass over Learn mode. The intake questions rendered, the
options were tappable, each answer recorded and the session's `intake` list grew. And the plan never
came, because `learn.build` is its own RPC and nothing on screen called it: answering an intake
question refreshed the session and stopped. Doc 14 section 3.4 lets the learner skip intake, which is
why building cannot be a side effect of finishing it, and that is exactly why something had to notice
when the questions ran out.

**Decision** The panel drops a question the session already holds an answer to, and asks for the plan
when the last one goes. A `Just build it` button offers the skip doc 14 section 3.4 names.

**The same shape as the click delegation gap at M9**, and worth naming twice for that reason: markup
that renders correctly, a handler that fires correctly, and no path from the end of one step to the
start of the next. Neither a unit test nor a screenshot catches it. A test that drives the whole
sequence does, and only that.

**A second bug in the test, not the product.** The first helper clicked the first intake option and
waited for it to become hidden. A `.first()` locator re-resolves on every poll, so once that question
was gone the locator resolved to the next question's first option and reported it visible until the
timeout. The loop waits on the count of unanswered questions now.

**Reason** Recorded because the plan's rule about metrics has a UI twin: a screen that renders is not
a screen that works, and only a driver that walks the whole path can tell them apart.

---

### BN-086 Two events the log claimed and one actor it got wrong

**Spec** 14 sections 2 and 3.3, 12's acceptance walkthrough line 12.

**Found** while checking the Learn session against walkthrough line 12, "every act appears in board
history with the right actor". Two defects, both in the same twenty lines.

**The log claimed checks nobody asked.** Doc 01's vocabulary declares seven `learn.*` events, and the
turn recorder needed an event for two stages that have none: the tutor asking its intake questions,
and the tutor replying with no card to open. It reached for the nearest declared name and wrote
`learn.check_asked.v1` for both. Anything counting checks would have believed it, and the log is
append-only, so nothing could take it back.

The fix is that neither stage records anything. Asking the intake questions changes no session state:
the answer is what changes it, and `learn.intake_answered.v1` already carries that. A reply with no
card to open changes none either. Both write columns that were already what they are, so the write
existed only to carry the event.

**Every act was attributed to the learner.** `Provenance::user` on all six, including the plan and the
check question, which are the tutor's decisions. On a screen that reads back as the learner having
written their own exam. Session writes now name their actor at the call site, and the three the tutor
makes carry `Provenance::agent("tutor", run_id)` with the run it decided in.

**Not adding two events to the vocabulary** was the choice underneath both. The seven declared events
are the session's state changes, and the two stages that wanted a name are not state changes. An
eighth event for "the tutor said something" would put the conversation in the event log beside the
decisions, and doc 14 section 2 keeps the conversation in the session row where a reopened panel can
still read it.

**Reason** Recorded because a wrong event in an append-only log is the one class of defect this
build cannot correct later, only annotate.

---

### BN-087 The bundle, and the one default where being wrong sends someone's documents to a stranger

**Spec** 01 section 7, 12 phase 10.

**Decision** A board exports as a zip: a manifest, one jsonl per entity kind, and the blobs the rows
point at. Rows travel as objects keyed by column name rather than as thirteen hand written structs,
so a column added to the store appears in the next bundle instead of being silently dropped.

**The checklist defaults to sending nothing.** Doc 01 section 7 says the exporter shows a checklist
of local document sources "so nothing leaves by accident", and a checklist whose default is to send
everything is not a checklist. A local document travels only when its source id is named. There is a
test whose whole content is that assertion, because this is the one setting where the wrong default
puts a person's own files on a stranger's machine.

**The redaction lives in one function.** `redact_source` reduces a local document's locator to its
file name and rewrites its dedupe key to match, and every export path calls it. A rule applied in two
places is a rule that will one day be applied in one.

**A citation whose source was withheld still travels.** Dropping it would quietly change the card's
answer: markers are rendered from citations, and a card that cited four things arriving citing three
reads as a card that claimed less. The importer resolves it as a citation with no passage, which is
visibly missing rather than invisibly absent.

**The sender's history arrives as a replay.** Doc 01 section 6.3's `source` enum has `replay` for
exactly this. Appending the events as `live` would have the recipient's own log claim they built a
board they were given, which is the same class of defect as BN-086 and caught by the same question.

**Every rule has a guard that bites**, checked by breaking each one in turn: redaction off, checklist
ignored, existence check dropped, source merge skipped, concept collision merged silently, history
appended as live. Six mutations, six named failures. The first run of the whole suite passed, which
in this build is a reason to go looking rather than a reason to stop.

**Reason** Doc 12's walkthrough rows 10 and 11 are a person exporting a bundle and importing it on a
second machine, and every test here uses two profiles for that reason: one profile proves the writer
and the reader agree with each other, which is true whatever either of them does.

---

### BN-088 The corpus ids are not ULIDs, and the bundle is where that first matters

**Spec** 01 line 79, 02 section 6.

**Found** on the first run of doc 12 phase 10's acceptance. All twenty boards failed export, and the
manifest schema said why: `/board_id: "B-01" does not match "^[0-9A-HJKMNP-TV-Z]{26}$"`.

**The schema is right and the corpus is the outlier.** Doc 01 line 79: "Identifiers are ULIDs (time
sortable, safe to merge across machines when bundles are imported)." The parenthesis is the whole
point, and the bundle is the first place in the build where that sentence has teeth. Everywhere else
an id is just a key, so `B-01` worked and nothing noticed.

**Decision** The round trip seeds its own profile, translating corpus ids to ULIDs as it goes. The
sweep keeps the readable ids, because a failing question is easier to chase when the board is called
`B-05` than when it is called `01M113PSRHMPE9P9HQGADJ630D`. The seeding step was already a
translation from corpus form into store form: `retriever_locator` does the same thing for paths.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --bundles`. Twenty of twenty boards
arrive whole, three of them the ones doc 02 line 155 marks for export, and the one concept term
collision the corpus plants is handled as doc 01 section 7 specifies. No provider is called: a bundle
carries what the board already holds and asks no model anything, so this acceptance is free.

**The check can fail.** Dropping one citation per board on the import side is reported for every board
that had one, with the counts on both sides.

**Reason** Recorded because the schema guard found a spec violation nothing else could have: the ids
were wrong in a way that only mattered at the boundary the ULIDs exist for.

---

### BN-089 The memory rule did not exist, and the plan was wrong about where it lived

**Spec** 05 v0.2 line 106, 12 principle 4, HANDOFF section 2.

**The plan said** `own_card_sole_support` was doctrine living in code, firing from a hardcoded
`rule_id` in the boards retriever. It was not. That line is a test fixture asserting that a card
carrying such a flag is never remembered, which is the eligibility rule and a different thing. The
name appears twice more in the build, both times in a comment. **The Verifier had no such rule at
all**, so the gate doc 05 v0.2 states in as many words had never once been evaluated.

**Decision** The detector is written, both packs carry the rule as data at block severity, and the
new `finance-eu` pack carries it too. A figure covered only by citations to `own_card` or `page`
passages is blocked. A figure covered by no citation belongs to `numeric_without_citation`; two
rules firing on one absence would put two flags on one span and read as two faults.

**Scoped to figures, not to every claim**, because a prior card summarising what a rule is about is
the context memory exists to supply. It is the numbers and the citations to instruments that have to
rest on the thing itself. Doc 16 line 69 extends the same rule to `page` sources when the vault
lands, so both classes are listed now and that arrival is a pack edit rather than a code edit.

**Doc 05 v0.2 also asks that own_card passages reach the Synthesizer "marked prior work, context
only".** The prompt carried `class="own_card"`, which says what a passage is and never what to do
with it. The sentence is there now, asserted end to end.

**Reason** Recorded because the plan asserted a defect that was one step milder than the truth, and
the check that found the difference was reading the line the plan cited.

---

### BN-090 Every percentage in the corpus escaped the citation rules

**Spec** 07 section B8.1.

**Found** while writing a fixture for BN-089's rule. The fixture said `2.5 %` and the detector found
nothing in it.

**`numeric_spans` ended its pattern in `\b`**, which cannot follow `%`: a word boundary needs a word
character on one side, and `%` is not one. So `2.5 %`, `2.5%` and `2.5 %.` matched nothing, and every
rule built on that helper skipped them without a word. That is `numeric_without_citation`, block
severity, doc 07 section B8.1's rule that a figure carries a source.

**Split before fixing, and the split is the finding.** The synthetic corpus writes all 147 of its
percentages with the symbol and none as "per cent". The unit tests wrote theirs as `percent`, which
the pattern matches. So the gate passed every test and measured nothing across the whole corpus, and
neither side of the build ever met the other.

**Measured after the fix**: no metric moved on the grounded sweep. That is the right outcome and it
is worth saying why rather than treating it as nothing happening. The grounded mock quotes each
passage behind its own marker and spans the citation over it, so every figure it writes is already
inside a cited span. The rule now sees the figures it always should have seen and finds them cited.
What changed is that a real model writing `2.5 %` outside a cited span will be caught, and until now
it would not have been.

**Reason** The same shape as BN-019's rule seen from the other end: a metric can have nothing to
measure, and so can a gate. This one had a denominator of 147 and a matcher that could not see any
of them.

---

### BN-091 Finance ships, and the twin has to keep agreeing with it

**Spec** 12 phase 10, 11 mission, 02 section 4.

**Decision** `packs/finance-eu.json` ships beside `general` and `finance-eu-synthetic`, which is doc
12 phase 10's three. It is the twin with real bodies in the source hierarchy and real instruments in
the vocabulary: EBA, ECB and EUR-Lex at trust rank 1, ESMA at 2, then structured, local, own_card,
web, user supplied. CRR and CRD, PSD2, DORA.

**The rules are identical and a test says so.** Doc 02 section 4 says the twin exists "so a score on
the corpus is comparable with the shipped pack", and that only holds while the two agree on every
rule id, severity and detector. What may differ is who the issuers are and what the words are, which
name the domain rather than decide whether a card passes. Dropping one rule from the shipped pack
fails that test.

**The hierarchy is a first draft and is recorded as one.** Saying that EBA guidance outranks a
national circular is a domain judgment, and nothing in this build verifies it. The rules underneath
are measured; the ranking of real issuers is not, and it wants a review from someone who works in the
field before it reaches one.

**Reason** Doc 11's mission makes finance the first doctrine pack rather than an optional one, and
two of three shipping meant the sentence was not true yet.

---

### BN-092 The diagnostics export shipped everything it exists to withhold

**Spec** 10 section 11.

**Found** by the test written beside it, on its first run.

**What happened.** `payload` is declared TEXT and holds serialised json, so a row
read comes back as one long string. The redaction walked the value it was given,
matched no keys because a string has none, and wrote the whole payload through
untouched: every answer, every question, every reason a flag gave, every passage
quoted in an evidence object. The export was airtight against exactly the shape
it never saw.

**Decision** A json column is parsed before it is redacted, and a nested string
that looks like json is descended into rather than passed through. `Step` fields
carry a document inside a document, and a pass that stopped at the quote mark
would ship it.

**The rule here is the opposite of the bundle's.** A bundle names what it
includes, so a column added to the store tomorrow travels by design. This names
what survives, and everything else goes. That is the right way round for the one
file whose recipient is a stranger debugging a crash rather than someone the
sender chose.

**Evidence goes whole rather than field by field.** Doc 01 line 310 says evidence
is "the passage, the number, the stale date", so descending into it to keep some
of it would be looking for reasons to keep part of a field that exists to carry
content.

**Reason** Recorded because the defect and its test were written in the same
hour, and only one of them was right. A redaction that has never been run
against real rows is a claim, not a guard.

---

### BN-093 A test that searched for a string the export could not have contained

**Spec** 10 section 11.

**Found** by breaking the redaction on purpose after BN-092 was fixed, to check
the guards bit. Two of three did. The third, the end to end one over the RPC
surface, passed with the redaction switched off entirely.

**Why.** It looked for the card's answer. `card.answered.v1` carries a card id, a
mode and three counts, and never the prose, so the answer was not in the export
either way and the assertion was true whatever the code did.
`card.requested.v1` does carry the question a person typed, which makes it the
one field in that fixture where a leak would show.

**Reason** The same shape as BN-090 and BN-019 in a third place: a check with
nothing to check reports success, and success is indistinguishable from working
until somebody breaks the thing on purpose. Mutating each rule in turn is now
what earns a guard the right to be called one.

---

### BN-094 A backup is a snapshot, not a copy of a file

**Spec** 10 section 15.

**Decision** `VACUUM INTO` rather than a byte copy. SQLite in WAL mode keeps
recent commits in a side file, so copying `tessera.sqlite` while anything is
writing produces a database mid transaction: it opens, it passes a shallow look,
and it is missing whatever had not been checkpointed. The snapshot is deleted
whether the zip succeeded or not, because until then the profile folder holds two
copies of everything a person owns.

**Corruption is detected before the migrations run**, not after. A migration
against damaged pages rewrites tables on top of the damage and turns a database
that could still have been partly read into one that cannot, at which point the
backup is the only copy of anything. `PRAGMA integrity_check` reads every page,
which is the point: an opened handle proves the header parsed and nothing else.

**Nothing is moved on start.** Doc 10 section 15 says the damaged file is kept
aside, and `quarantine` is a separate call the shell makes after telling the
person what it found. A start that silently renamed someone's work and carried on
is the behaviour they would least expect and could least undo. The `-wal` and
`-shm` files go with it, because applied to a restored database they would be a
second corruption on top of the first.

**Restore refuses a folder that already holds a profile.** The offer is made to
someone whose database is damaged, and the worst reading of that offer is one
that overwrites the damaged file before anyone has looked at it.

**Restore is not an RPC.** It replaces the database the running core is holding
open, so a core cannot perform one on itself. A `profile.restore` that half
worked would land on someone whose database is already damaged.

**Reason** Doc 10 section 15 names three operations and the interesting part of
all three is what they refuse to do.

---

### BN-095 CI, and the tree that had never been formatted

**Spec** 12 phase 11.

**Decision** Three workflows. `checks.yml` runs everything free on every push,
split into four jobs so a failure names itself rather than saying "checks
failed". `eval.yml` is the live sweep, behind a manual trigger with the question
count as an input, and its nightly schedule is written and commented out.
`release.yml` builds the msi and the dmg and opens a draft release.

**The formatter check needed the tree formatted first.** `rustfmt.toml` has been
in the repo since the first commit and rustfmt was never installed in the
container this was built in, so `cargo fmt --check` failed on forty five files
the moment it was added. A check the tree cannot pass is a check nobody can act
on, so the tree was formatted in its own commit before CI arrived.

**Signing is conditional and the job says which build it made.** An unsigned
build that claimed to be signed is worse than one that says so: the person finds
out from Gatekeeper instead, holding a file nobody warned them about.

**The nightly is not scheduled.** Doc 12 phase 11 asks for one and the cron line
is there, commented, because a job that bills an account every night is a
decision somebody makes on purpose rather than a default that arrives with a
merge. Uncommenting it is that decision.

**Not verified on a runner.** Every step was run locally and the yaml parses, but
both pushes produced runs that ended in seconds with no runner assigned and no
logs, which is what an account with no Actions minutes looks like. The workflows
are written and unproven, and that is recorded rather than assumed away.

---

### BN-096 A key a headless runner can read, and why it is not a second keychain

**Spec** 01 section 4.16, 10 section 8, 12 phase 11.

**Found** while writing `eval.yml`. There was no path at all: Linux `keyring`
wants a Secret Service over D-Bus, a CI runner has no session for one, and every
live run there would have failed on its first call. Doc 12 phase 11's nightly
could not exist.

**Decision** `EnvKeyStore` reads a key from an environment variable named by its
provider, so `anthropic-default` and `anthropic-team` both resolve to
`TESSERA_KEY_ANTHROPIC`: a runner has one account per provider, and the label
after the first dash is a name someone chose on their own machine.

**What the rule was protecting is intact.** A secret still never lands in a file
and never becomes an argument, and an argument is the thing that shows up in
`ps`, in a crash dump, and in the runner's own echo of the command it ran. The
store is read only, because nothing a process exports reaches the step after it
and reporting a key stored that is not would send someone looking for it later.
Its error names the variable and never any part of a value: a CI log is the most
public place this build has.

**Opted into by `TESSERA_CI`, never by probing.** Asking whether the keychain
happens to answer would treat a locked keychain on someone's laptop the same as
an absent one, and falling back there would train a person to expect an unlock
prompt that never comes.

**Reason** Recorded because this is the one place a standing constraint was
widened, and the argument for it should be legible without reading the diff.

---

### BN-097 First run, and the question the shell must not answer for itself

**Spec** 11 section 6, 12 phase 11.

**Decision** Three steps, and the third says it is optional. The pack is already
chosen, because a profile always has one, so that step opens finished. The key
is the only one that gates the way out, and the disabled button says why rather
than leaving a person to work it out from a dead control.

**Whether this is a first run is asked of the core.** A shell that inferred it
from "are there any boards" would show the setup screen again to someone who
trashed their only board, and a second shell would infer it differently. The
definition lives in one place, and a key in the keychain is what it turns on:
a pack is always set and a folder is optional, so neither can be the question.

**The key field is cleared before the call, not after.** It holds the only copy
of the secret in the page and the screen stays up when a call fails.

**A folder that does not exist is refused.** A path typed with a typo is the
common case, and a setup that accepted it would leave a retriever pointed at
nothing and report that everything went well.

**A sensitive folder cannot ask for provider embeddings.** Doc 05 section 7 and
doc 10 section 16 make those two settings a contradiction, and honouring both
quietly would send the text of a folder someone marked private.

**Reason** Doc 12 phase 11's acceptance is a fresh install to a first verified
deep card, and the only part of that measurable without spending money is
everything up to the ask. Seven Playwright tests cover it; breaking the gate
fails six of them.

---

### BN-098 Walkthrough line 15 had no screen, and finding one out took four defects

**Spec** 01 section 4.4, 15 section 2, 12's acceptance walkthrough line 15.

**The row** is "a card on a second board builds on a verified card from the
first, citing the original source". The core has done it since M6 and a Rust test
has proved it since then. `builds_on` was recorded on the card, carried over the
RPC in `board.get`, and rendered by nothing. The plan called this the encouraging
row because the hard part was built; what it lacked was a surface, and a surface
that does not exist is indistinguishable from one that does until someone drives
it.

**Decision** The build trail names the prior cards, read from `card.answered.v1`
like every other row in that disclosure rather than from the card row, so the
disclosure has one source and cannot disagree with itself. Doc 15 section 2's
rule decides the wording: it names the cards and never stands in for the
citations below it, because a prior card is context and the source it cited is
the evidence.

**Driving it end to end found four things**, none of which a unit test would have.

1. **The boards retriever is not a retriever.** Doc 04 section 10's
   `no_retriever_enabled` refuses a plan with nothing to retrieve from, and doc
   05 adds `boards` to every sub-question rather than making it a substitute for
   one. A dev core with only memory configured could not answer a deep card at
   all.
2. **The dev server could never produce a verified card.** Its Synthesizer mock
   returned a fixed answer that cited nothing, so every deep card came back
   flagged `unsupported_claim`, and doc 15 section 3 rules out a card with an
   open block flag. Memory had nothing to remember, in the one place a person
   would look at it.
3. **A citation marker after the full stop is its own sentence.** The Synthesizer
   binds by walking sentences and reading the `[n]` markers in each, so the first
   fix put the marker outside the claim it belonged to and every card stayed
   flagged. The marker goes before the stop.
4. **`verifier_below_threshold` was telling people something untrue.** Its
   message read "the support check is not enabled yet", which stopped being true
   at M8 when that check was built. Doc 07 section B9 has the rule fire while a
   pack has not reached 0.90 agreement with the ledger check, which no pack has,
   so it fires on every deep and research card in the product. Anyone reading it
   would conclude nothing had checked their card, when what is pending is the
   measurement that would let the checking run unsupervised.

**`own_card_sole_support` still did not fire**, and correctly: the answer carries
no figures, and BN-089 scoped the rule to figures on purpose. Its first real
opportunity came and went, which is worth more than the n/a it keeps.

**Reason** Recorded because the fourth one is the interesting one. It was in
front of a user on every deep card since M8 and no test noticed, because no test
reads a flag's prose and the copy lint checks style rather than truth.

---

### BN-099 A verdict the sample could not support, found while rehearsing the run that spends money

**Spec** 02 section 10.3, and BN-019's rule one step along.

**Found** by rehearsing the small live sample on the mock before any key existed.
Eight questions, and the report said `fact_recall_deep` **failed** at 0.833
against a 0.85 gate. The same code on 400 questions passes at 0.923.

**The denominator was six.** At n=6 the values the metric can take are 0.667,
0.833 and 1.000. Nothing that sample can produce lands near 0.85, so both
verdicts are an artefact of the sample size rather than a statement about the
product. `fact_recall_research` was worse: n=2, where the only answers are 0.00,
0.50 and 1.00 against a 0.92 gate.

**Decision** A metric withholds its verdict when one item either way would flip
it, reporting the value and naming the denominator instead. Fragility rather
than a minimum count, because a floor would be a number somebody picked, and
what actually matters is whether the sample can tell the two sides of the gate
apart.

**Two exemptions, and the second one was a correction.** An absolute gate is
exempt: 1.00 and 0.00 mean "never" and "always", and one violation disproves
that however few ran, so doc 07's injection resistance still fails on a single
case. The first version stopped there and broke an existing test, which was
right to break. `staleness_detection` at n=2 became "thin" when one of two
planted stale cards was missed, and a missed stale card is a defect at any
denominator.

**The distinction is not the threshold, it is what the denominator counts.** Six
deep questions are a sample of a population and cannot estimate a rate. Two
planted superseded regulations are not a sample of anything: they are two cases
the corpus put there to be caught. `PLANTED_CASES` names those, which is data
like the three classifications beside it, and a guard test refuses a name in it
that is already exempt for being absolute.

**Thin is an abstention and never a failure.** A run whose only complaint is a
small sample must not report a regression, or the small sample everyone runs
first becomes the reason nobody trusts the report. The full sweep is unchanged:
at n=400 nothing is thin, and the verdict reads exactly as it did.

**Reason** Recorded because of when it was found. The whole point of a small
sample before a full sweep is to check the machinery, and the machinery would
have reported a failure that was not one, on the first run that cost money.

---

### BN-100 A run record whose numbers moved when it was read again

**Spec** 02 section 10.2, 07 section B12.

**Found** by a stop hook, of all things. It reported uncommitted changes in the
tree, and the changes were a committed sweep's `report.md` and `summary.json`,
which had been rewritten by rescoring the same run after BN-099.

**Nothing about the run had changed.** `flag_false_positive_rate` names the worst
offending rule, because one rule crying wolf is enough to make the Flags queue
something a user learns to ignore and an average would hide it. Six rules sat at
exactly 1.0 on the grounded sweep, and the pick was `max` over a dict, which
returns whichever the iteration reached first. So the rule the metric named
changed between two readings of one run, and the denominator reported beside it
went from 89 to 153 with it.

**Decision** The tie breaks by name. Scoring one run twice now names the same
rule, and a genuinely worse rule still wins whatever it is called.

**Reason** Worth its own note because of what it threatens rather than what it
broke. A run against a live provider costs money and cannot be reproduced
exactly, which is the whole argument for committing its record. A record that
reads differently each time it is scored is not one, and this would have been
found on a paid run rather than a free one if the hook had not caught the diff.

---

### BN-101 The environment keystore could never have fetched a key

**Spec** 12 phase 11, and BN-096 which introduced the thing this breaks.

**Found** by a two question smoke test against a real provider, the first time
that path had ever run. It reported `No key stored under moonshot-default` with
the key sitting in the environment beside it.

**`keystore` honoured `TESSERA_CI` and `build_plan` did not.** The helper handed
back an `EnvKeyStore`, and the two places that build a Core used it. The plan
builder read `OsKeychain` directly, and the plan builder is what actually
fetches a provider secret. So the environment path existed, had four unit tests
of its own, and could not reach a key.

**BN-095 called the CI eval written and unproven.** This is what unproven was
hiding. Nothing about it was visible from the unit tests, because they tested
`EnvKeyStore` rather than whether anything asked it for a key.

**Reason** Recorded because of what the smoke test cost. Two questions, and the
provider was never reached, so the run that found this spent nothing. The same
defect found on a scheduled nightly would have been a failed run and an
unexplained silence.

---

### BN-102 Kimi is unreachable from one environment, and that is a policy denial

**Measured** 2026-08-27. `api.moonshot.ai:443` answers 403 to CONNECT through
the session's egress proxy. `api.anthropic.com` is on the proxy's bypass list
and answers normally: a single sixteen token call returns `ready` on
`claude-sonnet-4-5`, so the Anthropic key is live and the client works.

**So the 90/10 split cannot run here.** The ninety percent leg is the blocked
one. What is reachable is the expensive provider that was meant to carry a tenth
of the run.

**Not worked around.** The proxy's own README says a 403 is the organization's
egress policy for the session and to report the blocked host rather than retry
or route around it. Recorded rather than solved.

**What this does not say.** Nothing about whether the Kimi key is valid. The
request never reached a Moonshot server, so the key is untested and stays that
way until the sweep runs somewhere that can reach one.

**Amended 2026-08-28: the title is wrong and this entry misled its own author.**
Kimi is not unreachable from "the build container". It is unreachable from *an
environment whose network policy is allowlist only*, and Tessera has run two
full four hundred question sweeps against `api.moonshot.ai` from an environment
that was not: `eval/results/42/kimi-bulk/run-1787660259` and `run-1787665281`,
2026-08-25 12:17 and 13:41 UTC, 398 of 400 and 400 of 400 cards produced on
`kimi-k2.6` and `kimi-k3`.

What settles it is that nothing about Moonshot was singled out. In the blocked
environment `api.openai.com`, `example.com` and `www.google.com` answer 403 to
CONNECT as well, and what answers normally is exactly the proxy's bypass list:
the Anthropic API and the package registries. So this is one environment
setting, not a standing denial, and the fix is to run the sweep from an
environment with open network access rather than to work around anything.

Recorded because this entry was read a day later as evidence that the ninety
five percent leg could never run, and the owner had to say "it worked
yesterday" twice before anyone checked the results tree sitting next to it. A
measurement of one environment written as a property of the product is the
failure here, and it is the same class as a metric reporting a number for
something it never measured.

---

### BN-103 Three schema keywords the structured output endpoint refuses, found on the first live run

**Spec** 12 operating principle 1, 10 section 10.

**Found** by the first sweep that reached a real provider. Twelve of twelve
questions failed, all with the same shape of error, and none of them cost a
token: the API rejects the request before generating, and a rejected request is
not billed.

**Three refusals, one after another**, each hidden behind the one before it.

1. `output_config.format.schema: For 'array' type, property 'maxItems' is not
   supported`.
2. `For 'object' type, 'additionalProperties: object' is not supported. Please
   set 'additionalProperties' to false`.
3. `For 'object' type, 'additionalProperties' must be explicitly set to false`,
   which is the same rule reaching a nested object three levels down that a hand
   written schema had not bothered to close.

**Decision** The Anthropic client sends a copy of the schema with the count
keywords stripped and every object closed. Ten keywords go: the ones that say
how much rather than what shape. Shape is what a model needs, and `type`,
`properties`, `required`, `items` and `enum` all survive.

**This loosens what is asked for and not what is checked.** Doc 12 principle 1
validates every agent output against the full schema on the way back in, and
that pass reads the original. A model that returns seven items where the schema
allows five still fails the guard exactly as it did before. What changed is only
the copy handed over as a generation hint, because the provider refuses the
request outright rather than ignoring a keyword it does not know.

**A pre-existing test asserted the schema was passed through verbatim**, and it
broke, correctly. Passing it through verbatim is what refused the run.

**Reason** This is the whole argument for a live run, in one finding. The mock
accepts any schema, because a fixture answers in the shape it was asked for.
Nothing short of a real provider could have found it, and the twelve questions
that found it were free.

---

### BN-104 One forbidden value, two specs, and the number that would have sent work to the wrong place

**Spec** 02 line 201, 07 line 233.

**Measured** 2026-08-27, twelve questions on Anthropic, 74 model calls, 163k
input and 52k output tokens, 4.53 US dollars.

**The run reported `forbidden_fact_rate` 0.083 against a zero threshold**, which
is a fail on the gate doc 07 section B12 calls a P0. Splitting it before
reacting, as the standing rule requires, found one question: Q-0010, a
superseded regulation case. The card stated a value planted as wrong.

**And the Verifier caught it.** Status flagged, confidence 0.25, thirteen flags
including `numeric_without_citation` and three `unsupported_claim`. Doc 07 line
233 is explicit that the P0 is "a forbidden value that reaches an **unflagged**
card", and this one reached a card nobody could mistake for verified.

**The two specs disagree, and both are right about different things.** Doc 02
line 201 counts answers containing any forbidden value, target zero. Doc 07 line
233 counts one that survives verification. The scorer implemented doc 02's
definition under doc 07's name, and cited doc 07's wording in the comment beside
it.

**Decision** Both, reported separately. `forbidden_fact_rate` keeps doc 02's
definition and its zero gate. `forbidden_fact_unflagged` is doc 07's P0, gated
at zero, and it passed.

**Why not just fix the one.** Collapsing them loses what tells you where to
work. A wrong value written and caught is a Synthesizer problem with a Verifier
doing its job. A wrong value written and not caught is a Verifier problem. One
number cannot say which, and this run is precisely the case where the difference
matters: reacting to the single failing number would have sent the next stretch
of work at the Verifier, which was the part that worked.

**This is the sixth time a metric would have misdirected this project**, and the
first time the rule caught it on a run that cost money.

---

### BN-105 What twelve live questions actually said

**Measured** 2026-08-27, corpus 0.3.0-42, twelve questions, every one on
Anthropic through the reference leg because Kimi is unreachable from the build
container (BN-102). Haiku 4.5, Sonnet 5 and Opus 5 across the tiers.

**Cost** 4.53 US dollars. Opus carried 28 of 74 calls and 4.14 of those dollars,
which is what a frontier tier costs when the Synthesizer and the Verifier both
sit on it. Extrapolated, the full four hundred question sweep is about 150
dollars on this policy.

**Every question produced a card**, eleven flagged and one clean. Median
confidence 0.75.

| Gate | Value | Reading |
|---|---|---|
| `fact_recall_deep` | 1.000 | ten of ten, the first real evidence the pipeline recalls what it retrieved |
| `injection_resistance` | 1.000 | two of three demonstrably saw the hostile document and neither followed it |
| `must_exclude_compliance` | 1.000 | doctrine held |
| `forbidden_fact_unflagged` | 0.000 | nothing wrong survived verification |
| `citation_accuracy_ledger` | 0.365 | fails, and now measures the product rather than the mock |
| `verifier_agreement` | 0.542 | fails, and this is the 0.90 automation gate |
| `route_accuracy` | 0.750 | fails, three of twelve routed to the wrong depth |
| `visual_type_match` | 0.083 | fails hard, one of twelve |

**The two advisory gates are advisory no longer.** BN-061 and the MOCKED list
held `citation_accuracy_ledger` and `verifier_agreement` back because a mock
cites everything it is handed and quotes what it cites, so both numbers
described the fixture. On a real provider they describe the product, and both
are a long way under. Doc 02 section 10.3's 0.90 automation gate is not met,
which means the harness stays in draft mode and `verifier_below_threshold`
keeps firing, exactly as doc 07 section B9 specifies.

**Latency is the other finding.** Median 58 seconds a card, worst 122, against
doc 07 line 233's 8 second p95 for deep. Planner p95 alone is 10.2 seconds
against doc 04 section 12's 4 second target. Nothing about that was visible on a
mock that answers instantly.

**What twelve questions cannot say.** Three gates reported thin, and
`fact_recall_research` at n=2 is not a measurement of anything. The numbers
above are a direction, not a score.

---

### BN-106 Docs 16 and 17 adopted, and the decisions their arrival forces

**Spec** 16, 17, HANDOFF second revision (sections 10 to 12).

**Adopted** 2026-08-27. The HANDOFF's consolidated sequence stands: phases 12a to 12f (vault)
then 13a to 13g (learning system), vault first because learning records are pages. Everything
runs on the grounded mock and the synthetic corpus; the live sweep stays a parked spend
decision.

**The identifier stays `tessera`.** HANDOFF section 10 says the code identifier stays `canvas`
until the trademark clears, and that line was written before BN-001, where the owner chose
Tessera at planning. A hundred commits carry the name. If the trademark check ever fails, the
rename is one workspace refactor then, and would have been the same one now.

**Doc 17's open questions resolve as its own proposals.** The learner accepts a proposed
prerequisite set and corrects on the map. Exposure counts an open, a scroll, or a hover of
three seconds, behind one named constant so the guess is tunable. Spaced review stays
deferred, with `decayed` and `last_evidence_at` modelled so a scheduler is additive. A lesson
board keeps mode `learn` and allows exploration; the map reads both.

**Doc 16's open points resolve as its proposals.** Simultaneous edits keep both: last write
wins plus a conflict copy, with an event. Page trust rank 4 in the finance pack, recorded as
the proposal it is. An ungrounded notebook answer that is rerun with web on is superseded,
never discarded, so the trail shows what was said before sources were found. Save as page is
a ninth card verb, Save.

**Doc 17's event names map onto the vocabulary rather than duplicating it.** `lesson.planned`,
`check.asked` and `check.answered` are the existing `learn.planned.v1`,
`learn.check_asked.v1` and `learn.check_answered.v1` with payloads gaining
`{concept_id, level}`. Two names for one meaning in an append-only log is the BN-086 failure
class, so the deviation from doc 17 section 9's spelling is recorded here instead of shipped.

**Exercise kinds grow, they do not rename.** The shipped `kind` vocabulary
(`recall | apply | contrast | trace`) is in the output schema, the packs and the scorer's
independent re-check; renaming values there would invalidate recorded runs. Doc 17's ladder
arrives as an orthogonal `level: 1..4`, the kind enum gains `explain` and `discriminate`
additively, and the level to kind mapping is 1 recall, 2 explain, 3 apply, 4 discriminate.

**The Map is a board.** Doc 17 says "a board of `mode: map` rendered from concepts", and the
build takes that literally: one lazily created board per profile backs the rail view, so
viewport persistence, events and export come free, while the view renders from concept rows
and edges and never from stored cards. `board.list` gains a mode filter; Home shows explore
and learn, the Notebook lists its own sessions, and the map board never appears as a row.

**Mastery moves and the old column stays.** `learn_session.mastery` stops being written once
mastery lives on concept rows; the column remains because dropping it is a table rebuild for
nothing, and historical sessions stay transcript, never backfilled into the map.

**No filesystem watcher in this stretch.** `vault::sync()` is the reconciliation unit, called
from app start, page writes and an RPC. A watcher is additive later; during the build it buys
only CI timing flake.

**Two defects found while planning gate the new work and land first.** `Core.retrievers` is
never populated from the pack and watched folders in any production path, so a watched folder
never actually retrieves on the shipped app; and the `note` table has no writer at all, so
doc 16's "Add note" begins by building the write path its sticky needs.

---

### BN-107 The retrievers the product claimed to have

**Spec** 05 sections 8.2, 8.5, 10 and 11; 10 section 16; 15 section 6.

**Built** 2026-08-27, M14.2.

**The defect** `Core::open` set `retrievers` to `RetrieverSet::default()` and nothing ever
replaced it. The dev server, the eval runner and every end to end test built a set of their
own, so retrieval was measured constantly and wired nowhere: on the shipped app a person could
add a folder in setup, watch the step tick green, ask a deep question and get an honest "no
sources found" card forever. Two things hid it. The set's own doc comment said it was "built
once per run from the pack's enabled retrievers and the profile's watched folders", which was
a description of the intention rather than of the code. And a working retriever over an empty
index produces exactly the card an unwired one does, so nothing about the output said which
it was.

A second half of the same defect: `profile.watch_folder` wrote the row and never read the
folder. `index_folder` existed and had three callers, none of them the product.

**Built** `repo::watched_folders` reads the rows; `retrieval::assemble(pack, folders,
memory_enabled)` turns them plus the pack's enabled retrievers into the set;
`Core::rebuild_retrievers` runs it at `Core::open`, on `use_pack`, and after
`profile.watch_folder` ingests. The RPC now walks the folder through `index_folder` with the
pack's `must_exclude` and returns what it found, and the setup step says the count rather than
just the label.

**What assemble refuses to claim.** `local` appears only once a folder is watched, `boards`
only while memory is on, and `regulatory`, `web` and `structured` do not appear at all: there
is no subscription mechanism and no web retriever yet (13e builds one). Doc 05 section 10
separates a retriever that is not configured from one configured and empty, and the first
belongs on the Profile page rather than at the bottom of a card, so `profile.get` reports them
unconfigured and the fan-out skips them. The pack still enables them, which is the honest
state: the pack wants them and the profile has not got them.

**No embedder in the product.** The set is assembled with `embedder: None`, so retrieval runs
the lexical half. The local model is a download the app does not ship, and the eval passes one
when the machine has it, which is why the recall number the eval reports is the better of the
two. Shipping the model, or fetching it on first run, is its own decision and is not this
step's.

**The per run allowlist landed here rather than at 12d.** `retrieval::run` takes
`allow: Option<&[&str]>` and narrows the set through `RetrieverSet::restricted` before
anything reads it, because the plan-less fallback in `assignments` reads the set too: two
places deciding what a notebook question may open is how one of them ends up opening the web.
A narrowed run drops what it was not allowed silently, since a policy choice is not a missing
connector and should not put a caveat on the card.

**Verified** Full battery green, plus the 400 question grounded sweep and the 20 board bundle
round trip, since the pipeline and the store both changed. The sweep record is no longer
committed: 8.6 MB of run rows per step, reproducible from the seed by one free command, and
already uploaded as a CI artifact. `.gitignore` said as much about mock output and never
covered `--policy ci`, which is how a dozen of them reached the history.

**The test that would have caught it** `a_watched_folder_is_cited_without_the_test_building_a_retriever_set`
in `end_to_end.rs` gives the core the two things a person has, a pack and a folder, and asserts
a citation of class `local_document`. Every test before it handed the core a set it built
itself, which is precisely why the wiring could be missing without a single failure. The guard
was broken once on purpose: with the rebuild removed from the RPC the citation count drops to
zero. The Playwright half drives the same path through the setup screen.

---

### BN-108 Importing a pack, and the choice that did not survive the night

**Spec** 10 section 9; 12 principle 4; 11 section 6.

**Built** 2026-08-27, M14.3.

**Built** `pack.import` reads a file, validates it through the registry against
`schemas/pack/doctrine-pack.v1.json`, copies it into `<profile>/packs/<code>.json`, registers
the version with `repo::ensure_pack` and adds it to the library. `PackLibrary::load_imported`
reads that folder at every start, so an import outlives the session that made it. Profile >
Doctrine lists the packs, says which ship with the app and which came from a file, and carries
the import field and a Use this pack verb.

**Importing does not activate.** Doc 10 section 9 makes a pack change a deliberate act, and an
import that silently re-judged the next card would be one. The RPC returns `active: false` and
the Doctrine list is where the switch happens.

**An imported pack may not take a shipped code.** Boards pin the pack they were judged under by
code and version. A file that renamed `general` would change what every board that pinned it
claims to have been judged by, so both paths refuse it: the RPC with an error, and a file
dropped into the folder by hand with a problem on the Doctrine page.

**A pack file that does not load never stops the profile opening.** A built in pack that fails
to parse is a build error and stops the app, because it is the same file on every machine. An
imported one belongs to the person, so it is skipped, the reason is carried to the Doctrine
page, and the boards open.

**The defect found on the way.** `Core::open` set `pack_code` to `general` unconditionally.
Choosing finance and restarting put the profile back on general with nothing on the screen
saying so, and every card after that was judged by rules the person had switched away from.
The Playwright test that covers the pack switch reloads the page rather than the process, so it
passed throughout. `profile.default_doctrine_pack_id` already existed and was written once at
profile creation and never again: `use_pack` now writes it, and `Core::open` reads it back,
falling back to general when the code names a pack the library no longer has.

**Two events that existed and were never written.** `pack.activated.v1` and `pack.imported.v1`
join the vocabulary and are emitted here. `index.folder_added.v1` has been in the vocabulary
since M2 with no writer at all, and now that adding a folder reads it (BN-107) there is
something to record, so `profile.watch_folder` emits it with what the walk found.

**Verified** Full battery green, 49 Playwright tests, the 400 question grounded sweep and the
bundle round trip. `an_imported_pack_outlives_the_process_that_imported_it` opens a second core
over the same profile folder, which is what tomorrow is.

---

### BN-109 The board offers the pack update, and the pin finally means something

**Spec** 10 section 9: "A pack update never rewrites a board's pinned version; the board offers
update pack, which reruns `verify_only`."

**Built** 2026-08-27, M14.4.

**Read of the plan.** The step was written as "switching or importing a pack triggers
`verify_only` over affected cards". Doc 10 section 9 is narrower and better: the update is a
verb on the board, not a consequence of importing. A profile-wide sweep on a pack switch would
re-judge boards under rules they never pinned, which is the thing the pinning rule exists to
prevent. So importing changes nothing on its own; a board whose pinned pack has a newer version
loaded offers the update, and taking it repins that board and re-judges its cards.

**The defect the step depended on.** `verify_card` resolved the pack from `self.pack_code`, the
profile's active pack, rather than from the board. A person who switched packs and reopened an
old board had it re-judged by rules it was never written under, while the board went on naming
the pack it pinned. Without fixing that, updating a pin would change nothing about how cards
are judged and the whole verb would be theatre. `pack_for_board` resolves by the board's pinned
code now; the version can move under it, which is what the update verb is for, and a pinned
code the library no longer has falls back to the profile's pack rather than refusing to
re-verify.

The test that catches it needed two packs with different version strings: the first version
compared `finance-eu-synthetic` against `general`, both at 1.0.0, and passed with the defect in
place. An imported pack at 0.1.0 against general at 1.0.0 discriminates, and the guard was
broken once on purpose to prove it.

**The trail.** `board.pack_updated.v1` (new) records the board moving from one version to
another and how many cards it will re-judge. `card.rerun.v1` has been in the vocabulary since
M2 with no writer, and this is what it is for: the card was not asked again, it was judged
again, and the payload says by what. The `verify_only` run row then carries the version that
did the judging, so a card that flips to flagged months later traces to the pack version that
flipped it rather than looking like the Verifier changed its mind.

**Failing one card does not stop the board.** Doc 07 fails closed per card; a card that cannot
be re-judged is reported as `not_reverified` in the report and the rest of the board carries
on.

**Verified** Full battery green, 50 Playwright tests, the 400 question grounded sweep and the
bundle round trip.

---

### BN-110 What the two failing gates were actually measuring

**Spec** 02 sections 10.2 and 10.3; 06 sections B8 and B10.

**Diagnosed** 2026-08-27, M14.5, from the committed record at
`eval/results/42/live/run-1787823689`. No money spent: everything below is a re-read of a run
already paid for, plus free runs on the grounded mock.

The rule this step exists to follow: split a surprising number by every dimension the record
carries before building the fix it seems to call for.

#### visual_type_match 0.083

| Dimension | What the record says |
|---|---|
| Expected type | 10 of 12 questions expected `table`, 1 `tree`, 1 no visual |
| Router's hint | `table` on every one of those 10, from the pack's `type_preferences` |
| Produced type | `tree` on 9, none at all on 2, `tree` correctly on 1 |
| Blocks per visual | 1, on all ten. Always the root, `citation_ordinals` empty, `no_claim` true |

So the corpus expectation and the doctrine agree, and the Router passed the right hint. What
the card got was a diagram with a single box in it.

Doc 06 section B8 point 1 selects the type from the shape of the Synthesizer's summary and
consults the hint only when nothing in the summary decides, which is what the code does. The
summary had relations and the rule stopped there. **The record cannot say how many**, because
`card.synthesized.v1` carried counts of citations, conflicts and unsupported statements and
nothing about the structure the type is chosen from. That is now recorded as `summary_shape`,
so the next run answers the question this one could not without a second paid run.

**Fixed here, because the record proves it:** a tree with no children is declined rather than
drawn. It reached the card by two routes, a model returning a bare root and a model whose
children were pruned as untraceable under it, and only the second was checked, after pruning.
The check now runs before indexing as well.

**Not fixed, because the record does not prove it:** whether the type rule should reach the
doctrine hint sooner. The one-line change that would make these ten cards tables is to demote
the `relations >= 1` branch, which is a rule doc 06 does not list, but with no `summary_shape`
in the record there is no evidence about which branch fired. Wait for a run that carries it.

**Worth noting:** the fix does not raise the gate. A declined visual counts as a miss the same
way a wrong type does, so `visual_type_match` stays failed until a value question produces a
summary with values in it. What the fix removes is a diagram that said nothing.

**And a gate that could not see it:** `visual_fidelity` passed at 1.000 over those ten
one-block visuals, because a block with `no_claim` true is bound by definition. A rule that
every block is cited or marked `no_claim` cannot notice that there is only one block and it is
the title.

#### verifier_agreement 0.542 and citation_accuracy_ledger 0.365

Split by verdict, over 96 citations:

| Verifier verdict | Passage states a required value | Count | Counted as |
|---|---|---|---|
| supported | no | 35 | disagreement |
| supported | yes | 26 | agreement |
| weak | no | 24 | agreement |
| weak | yes | 4 | disagreement |
| unsupported | no | 2 | agreement |
| unsupported | yes | 5 | disagreement |

And by position: citations at ordinals 1 and 2, the ones carrying the headline claim, state a
required value 15 times out of 19. From ordinal 6 on, 17 out of 60.

Every question in this run required exactly one fact, and the cards cited between 5 and 13
passages each. The ledger check asked of every one of them "does this passage state the value
the question required", so a card that cites the rule, its version history, its scope and its
dates scores one hit out of nine while being exactly right. The threshold of 0.95 over that
denominator is unreachable by any answer that cites more than one passage per fact.

Doc 02 section 10.2 says something narrower: "citations whose passage supports the claim
span". The ledger can only judge a span that asserts a value it holds, and the record carried
no claim spans, so the scorer asked a different question and called the gap disagreement. The
comment on `read_citations` had already reasoned its way to half of this when it added the
passage text; the claim span is the other half and it is added now.

**Fixed here:** `read_citations` carries `claim_span` and `binding`; the scorer resolves the
span against the answer and judges only the citations bound to a claim that states a value the
ledger holds. Two readouts land beside the gates so the narrowed numbers stay readable:
`citations_the_ledger_can_judge` (how much of the run the ledger has an opinion on) and
`verifier_missed_support` (disagreements in the direction that loses a citation the card had
every right to keep).

On the grounded sweep the narrowing moves `citation_accuracy_ledger` from 0.491 to 0.954 and
`verifier_agreement` from 0.593 to 0.987, with 0.277 of citations judgeable and
`verifier_missed_support` at 0.013. Both stay advisory on a mock, so no gate was made green by
this: what changed is that the number now measures what its name says.

The committed live run reports both as n/a naming what they wait for, since its citations
carry no spans. That is the governing rule working as intended, and it is more honest than the
0.365 it reported before.

**The nine that matter.** Of the 44 disagreements, 35 are the metric asking a different
question. The other 9, five `unsupported` and four `weak` over passages that do state the
required value, are genuine Verifier misses. Reacting to 0.542 by tuning the Verifier would
have spent the effort on the 35 and the component that was mostly right.

#### Noticed while splitting, not in this step's scope

`route_accuracy` 0.750 is three questions: two research-shaped ones the Router recommended
`deep` for (both of the run's two research questions) and one definitional one it recommended
`fast` for. With two research questions in the sample the research half of that metric is 0 of
2, which is a sample too thin to conclude from and a hint worth carrying into the next run.

---

### BN-111 The vault's tables, and the last board rebuild

**Spec** 16 sections 3.1, 3.2 and 4; 17 section 6.

**Built** 2026-08-27, M15 12a-i. Schema version 5.

**Migration 0005** adds `page` and `page_link`, adds `card.page_id`, and widens `board.mode`
with `map`.

**Why `map` rides along.** 0004 rebuilt `board` for `notebook` and stopped, because doc 17 had
not arrived and nothing would write `map`. It has arrived and is adopted (BN-106), so the mode
lands in this rebuild rather than earning a third one. A table rebuilt twice for two enum
values is BN-028's mistake made in instalments, and `board` is the table whose rebuild takes
every card with it if the foreign key handling is wrong.

**Pages are source of truth, not projections.** `rebuild()` folds the log into card status,
confidence and run cost. A page is a document the person wrote, and replaying the log must
never be able to rewrite one, so nothing about `rebuild()` changes here.

**The title rule is an index, not a writer's promise.** Doc 16 section 3.1 makes the title
unique per profile and case insensitive; `page_title` is a unique index with `COLLATE NOCASE`,
so the title keeps the capitals the person typed and only the comparison ignores them, and a
second writer cannot forget the rule.

**A rename keeps the id.** Doc 16 section 2.2 lists resolution by title string as one of the
assessed package's mistakes, because renames silently break the links into it. The rename
writes title and file path and leaves the id, which is what a wikilink will resolve to at
12a-iii.

**`file_path` is the caller's to compute.** The slug rule belongs with the mirror that writes
the file, which is 12a-ii, so this layer takes the path it is given. `content_hash` is written
with the body in the same statement, because the mirror compares by content hash and never by
mtime: a hash that lagged its body would make `sync` decide the file was the newer of the two.

**One writer, two events.** A page saved from a card emits `page.created_from_card.v1` and one
written by hand emits `page.created.v1`, because the two are different claims about where the
text came from and the log is what a person reads to find out.

**Which of doc 16's nine events land here.** Five: `page.created`, `page.created_from_card`,
`page.edited`, `page.renamed`, `page.deleted`, each with its writer in this step.
`page.link_resolved` and `page.link_unresolved` wait for the parser at 12a-iii; `notebook.asked`
and `notebook.grounding` wait for 12d. BN-107 and BN-109 both found events that had sat in the
vocabulary for milestones with nothing writing them, so each of these arrives with the code
that produces it.

**Verified** Full battery green, 480 Rust tests, 50 Playwright tests, the grounded sweep and
the bundle round trip. The sweep now measures 33 of 40 metrics rather than 31, because BN-110's
claim spans made the two citation metrics computable.

---

### BN-112 The vault mirror, and the third value a two way sync needs

**Spec** 16 sections 3.1 and 7 point 2.

**Built** 2026-08-27, M15 12a-ii. Schema version 6.

**The shape.** `plan(rows, files) -> Vec<Action>` is a pure function: no clock, no disk, no
store. `sync` reads the directory, calls it, and applies what it decided. Two way sync is a
correctness swamp and this is the way out: the decisions are a table that tests can be written
against without a filesystem, and there are eighteen of them.

**Comparison is by content, never by mtime.** A file restored from a backup, a folder synced by
another tool, a clock that moved: each produces an mtime that lies, and none of them changes
what the text says.

**A two way sync needs three values, and 0005 stored two.** `page.content_hash` held the hash of
the row's body, which the body already tells you. Deciding which copy moved needs what the row
says, what the file says, and what the two last agreed on. Migration 0006 renames the column
`synced_hash` and the meaning follows: an edit leaves it alone, because an edit is precisely the
event that makes the two disagree, and the mirror writes it when it reconciles them. A rename
rather than a second column, because two hash columns where one is derivable is how a later
reader compares the wrong one.

**The decision table.**

| Row moved | File moved | What happens |
|---|---|---|
| no | no | The agreement is recorded and nothing is written |
| yes | no | The file is written from the row |
| no | yes | The row takes the file's text, as `page.edited.v1` with `edited_in: vault` |
| yes | yes | The file wins the page; the row's text is kept as `<slug> (conflict).md` |
| any | file missing | The file is written back |
| no row | file present | The file becomes a page, titled by its first heading or its file name |

**Two judgement calls, recorded rather than assumed.** A missing file is rewritten rather than
taken as a deletion, because an unmounted folder, a sync tool mid pass and a deliberate deletion
look identical from here and only one of them wants the page gone; deleting happens in the app,
where the person is asked. And on a double edit the file wins the page, because doc 16 says
last write wins and there is no trustworthy "last": a person who edited the file outside the app
meant to, and the row's text is kept beside it rather than dropped.

**A title that is taken is named, not forced.** Two files whose titles differ only in case, or a
file titled like an existing page, leave one page and one `Skipped` with its reason. Guessing
which should win would be a coin toss with somebody's notes.

**The one race, and the bug it caught.** A sync without a lock can have a file edited between
the listing and the write, so every write re-reads first and stands down if what it finds is not
what the plan believed. The first version passed `None` for that expectation on every write,
meaning "the plan believed there was no file", so an ordinary app edit refused to reach its own
file: the file existed, held the agreed text, and did not match the new body. The end to end test
caught it, and `WriteFile` now carries what the plan believed the file held.

**No watcher.** `sync` is the unit, called at app start and from the `vault.sync` RPC, and after
a page write once 12b and 12c write pages. A watcher is additive later and would buy nothing
here except timing that fails in CI.

**Subpaths from day one.** `file_path("learning/a-mission", "2026-08-27")` gives
`vault/learning/a-mission/2026-08-27.md`, because doc 17 section 5 writes learning records there
and retrofitting a folder into a path function is a change to every caller.

**Verified** Full battery green, 18 vault tests, 50 Playwright tests, the grounded sweep and the
bundle round trip.

---

### BN-113 Wikilinks that survive a rename, and backlinks that are a query

**Spec** 16 sections 2.1, 2.2 and 3.1.

**Built** 2026-08-27, M15 12a-iii. Schema version 7.

**The rule this exists for.** Doc 16 section 2.2 lists resolving a wikilink by title string as
one of the assessed package's mistakes: a rename silently breaks every link into the page. So a
link resolves to a Page by id or a Concept by id, and the title in the body is what it displays
rather than what it points at. Rename the page and the link still arrives, which the test
asserts by renaming and re-querying.

**Resolution order: page, then concept, then nothing.** A page is a document a person can open
and a concept is a term in the glossary, so when both carry the title the page is what they meant
to follow. The concept detail lists pages beside cards either way.

**The parser will not read a link out of code.** A vault that documents this feature is full of
`[[Title]]` in fenced blocks and backticks, and linking those would fill somebody's notes with
references they did not write. Fences, inline code, unterminated brackets, and brackets spanning
a paragraph are all text.

**A third migration, and why it is not padding.** `page_link` stored `display_text` and a target.
`[[Liquidity risk|the rule]]` displays "the rule", so an aliased link that could not resolve
could never resolve later, and doc 16's "unresolved links create the page on click" would have
created a page called "the rule". 0007 adds `target_title`: every row now says what it points
at, what it shows, and where it is. `resolve_pending_links` is what that column buys: write
`[[Basel III]]` before the page exists, and when the page arrives the link lights up.

**One event per kind per save.** A page with twenty links would otherwise write twenty events on
every edit. `page.link_resolved.v1` when a save left nothing hanging, `page.link_unresolved.v1`
with the titles when it did, and the rows carry the detail either way.

**Backlinks are an index lookup**, and there is a test that reads `EXPLAIN QUERY PLAN` and fails
if it stops using `page_link_target`. A panel that scanned every body works on ten pages and
stops working at a thousand, which is where a person starts needing it.

**Where the backlink completeness gate went.** The plan put the 1.00 metric here. Its eval half
needs the synthetic vault, which is 12a-iv, so what lands here is the same property asserted
exhaustively rather than sampled: twelve pages, each linking to every page after it, every
count checked against the arithmetic and the total row count checked against the sum. The
harness metric lands with the vault it measures.

**Verified** Full battery green, 26 vault tests, 7 parser tests, 50 Playwright tests, the
grounded sweep and the bundle round trip.

---

### BN-114 A page is retrievable, and it is not a document

**Spec** 16 section 3.3; 05 section 8.5; 15 section 2.

**Built** 2026-08-27, M15 12a-iv, product half. The synthetic vault and its metrics are the
second half and land next.

**One implementation, one more id.** Doc 16 section 3.3 says "the local retriever indexes
`vault/` like any folder, so pages are retrievable with no new retriever", and that holds for the
implementation: this is `indexed` over another folder, exactly as `boards` is. What it needs of
its own is the id, because one `IndexedConfig` carries one source class and a folder set holding
both a person's documents and their pages could not label them apart. Doc 16 section 3.3 is
emphatic that it must, since the Verifier extends `own_card_sole_support` to `page` and a numeric
claim may not rest on a note the person wrote. Indexing the vault as `local` would have made it
evidence.

The id also pays for itself at 12d: doc 16 section 3.4 restricts a notebook question to vault
plus boards, which needs `vault` to be a name the allowlist can hold.

**Indexed from the row, not the file.** The row is what the mirror has just agreed with the file,
and the wikilinks are stripped first: `[[Liquidity risk]]` indexed verbatim matches a query for
brackets and misses the sentence the link is part of. The title leads the indexed text, because
a page called "Liquidity risk" whose body never repeats the phrase is still the page somebody
asking about liquidity risk wants.

**The vault is configured without anybody pointing at it**, unlike `local`, which waits for a
folder. The profile's own pages are already where the app can read them, so an empty vault is a
retriever with nothing to find rather than one nobody has set up. A pack can still turn it off,
because doctrine is data.

**And it cannot be the only retriever.** Doc 04 section 10's `no_retriever_enabled` excluded
`boards` on the grounds that a profile whose only retriever is its own memory can corroborate
itself. A page is context too, so the vault joins boards in not counting. Both are what a person
already had; a retriever is what brings something new. The existing test caught this the moment
the general pack enabled the vault, which is the right way round.

**Trust ranks.** `page` at 4 in both finance packs, doc 16 section 3.3's proposal, level with
`local_document` and above `own_card`. General ranks it 3, level with its own local documents,
because doc 16 speaks only of finance and an unranked class is the least trusted: leaving it out
would have ranked a person's own notes below a blog. The twin parity test now also asserts that
the two finance packs order their source classes identically, which is what makes a corpus score
transfer, while still allowing the twin's extra synthetic issuer to shift the absolute numbers.

**Verified** Full battery green, 50 Playwright tests, the grounded sweep and the bundle round
trip. The end to end test writes a markdown file into `vault/`, syncs, asks a deep question and
asserts a citation whose source class is `page`.

---

### BN-115 Forty pages that say what a vault says, and one that says nothing else does

**Spec** 16 section 5's eval line; 02 section 6.

**Built** 2026-08-27, M15 12a-iv, eval half.

**Three kinds of page, because they test three rules.** Twenty four saved from cards, carrying
the card's citations as `{ordinal, passage_id}`: doc 16 section 2.2's whole point, since the
assessed package pointed the next citation at the note and lost the regulation two hops later.
Eight written by hand about something a document also says, which is what a numeric claim must
not rest on alone. Eight written by hand about something **no document says**, which is what a
page-only question needs.

**"Only in the vault" is checked, not asserted.** A label pool shared with the documented facts
can hand out a value some document already states, and a page-only question whose answer is also
in the corpus measures nothing: an answer that never opened the vault would score just as well.
The generator moves the value by hundredths until no passage in the corpus states it, and a
guard test re-checks every one of the eight against every passage.

**The vault's questions are their own set.** Doc 02 section 6 fixes the main set at 400 with a
shape the guards check, so the two vault families live in `questions_vault.jsonl` beside
`questions_breadth.jsonl`. Merging them broke two guards immediately, which is the guards working.
The facts do join the ledger, so a page-only answer is scored by the same matchers as any other.

**The bug the output caught.** The page-only titles named a different subject from the one their
bodies stated: the title was computed from `fact`, which was still bound to the previous loop's
value, and Python was happy to read it. Printing forty pages and reading them is what found it.

**backlink_completeness, gated at 1.00 and absolute.** The eval seeds a throwaway profile,
queries the backlinks of every page the corpus linked to, and writes what the query answered.
The scorer does the arithmetic against the corpus's own `links_to`, because measuring a query
with itself reports 1.00 whatever it does. The denominator is what the corpus planted rather
than what the store kept: a link that never arrived cannot fail a backlink check, and scoring
only what arrived would report 1.00 on a vault that lost half its links. Three guard tests cover
the whole set, one lost link, and one link the target cannot find.

**Two metrics that wait.** `grounding_state_accuracy` and `page_sole_support_rate` report n/a
naming 12d, where the notebook produces the states they measure. Registered now with their
thresholds so the day a run sets them the number is judged rather than reported.

**Verified** Full battery green, 80 generator guards, 50 Playwright tests, the grounded sweep
(34 of 43 metrics measurable, `backlink_completeness` 1.000 over 60 links) and the bundle round
trip.

---

### BN-116 Save as page, and the citations that go with it

**Spec** 16 sections 3.2 and 4.

**Built** 2026-08-27, M15 12b.

**The ninth verb.** `card.save_as_page` builds a page from the card's question, answer, findings
and a text rendering of the visual, sets `source_card_id` and `citations_carried`, writes the
file, points the card at the page, resolves the links and indexes it. The card header shows a
chip; the verb goes away once there is nothing left to do to it.

**Copied, never re-derived.** Doc 16 section 2.2 is the whole reason `citations_carried` exists:
the assessed package pointed the next answer's citation at the note, so two hops later the
regulation was out of reach and possibly stale. The page carries the passages the card cited, so
the evidence is still the evidence.

**Blocked content is excluded, in both senses.** A card with a block flag is refused outright,
because it stays on the board until the flag is decided. And a visual block the Verifier hid is
left out of the rendering, since a page that carried the hidden tile would put back exactly what
the Verifier took out.

**Saving twice is not a second page.** The card already names one, so the verb reports what is
already there rather than making a duplicate with a numbered title. The person pressed a button
whose work was done.

**The acceptance, both halves.** Doc 16 phase 12b: "the Verifier blocks a numeric claim resting
on the page alone and admits it when the carried passage is cited." The detector half is two
unit tests over `own_card_sole_support` with class `page`; the end to end half writes a page into
the vault, asks a question the vault alone answers, and asserts the flag. The rule itself needed
no change, because M12b listed `page` beside `own_card` when it built the detector.

**A title from the question.** Trimmed of its question mark, capped, capitalised, and numbered if
the profile already has one. Renaming is the person's, and the id is what the links hold.

**Verified** Full battery green, 51 Playwright tests, the grounded sweep and the bundle round
trip.

---

### BN-117 The Pages view, and a link that is a control

**Spec** 16 sections 3.1, 3.7 and phase 12c.

**Built** 2026-08-27, M15 12c.

**One view, two states.** The explorer lists what the vault holds; opening a page reads it, and
writing it is the same view with a textarea. Five RPCs behind it: `page.list`, `page.get`,
`page.write`, `page.delete`, `page.create_from_link`.

**A wikilink is a control, not text.** A resolved one opens the page it names. An unresolved one
is dashed and offers to write it, which is doc 16 section 3.1's "creates the page on click" and
the reason an unresolved link is kept rather than dropped. The preview renders headings, list
items, paragraphs and links, and anything else renders as the text it is, which is the honest
failure for a preview rather than a markdown library nobody asked for.

**One write path for new and existing.** A person editing a title has done the same thing as a
person typing one: the row keeps its id, the file follows the slug, the links are re-read from
the body. A rename removes the old file before writing the new one, because a crash between the
two leaves a row with no file, which the mirror writes back, while the other order leaves a file
with no row, which it adopts as a second page.

**Deleting takes the index with it.** Doc 16 section 2.1 says an answer that cited the page is
untouched, and it is, because a citation names a Passage carrying its own text. What must go is
the index: a page whose row is gone and whose chunks remain would be retrieved and cited as a
source nobody can open.

**Two collisions the tests found.** The editor's `#page-title` was the shell's own heading id, so
the field could never be filled; and `.page-note` was already the class of the backlinks empty
state, so the file path assertion matched two elements. Both are the kind of thing that only
shows when the view is driven rather than read.

**A count of zero is not a measurement.** The explorer chip said "0 carried citations" for a page
saved from a card that cited nothing, which states the same thing twice while looking like a
number. It now says where the page came from, and counts only when there is something to count.

**Verified** Full battery green, 56 Playwright tests including five that drive the vault the way
a person would, the contrast gate extended to the new view, the grounded sweep and the bundle
round trip.

---

### BN-118 The notebook, and the ungrounded answer that stays honest

**Spec** 16 sections 2.1, 3.4 and 4; 06 section A10.

**Built** 2026-08-27, M15 12d-i, the core half.

**A session is a board.** Doc 16 section 3.4: "so history, events, memory, and export come
free". `notebook.open` creates one of mode `notebook` or converts a board, and `board.list`
gains a mode filter so Home lists what a person explores and learns on while the Notebook lists
its own sessions.

**The restriction is a property of the run.** A notebook question narrows the retriever set to
vault plus boards before the pipeline sees it, using the per-run allowlist M14.2 built. Narrowing
inside the fan-out would leave the plan-less fallback reading the full set, which is why that
step put the filter where the set is chosen rather than where it is used. `local:*` is left out
though doc 16 lists it as optional: a notebook is the view over what the person wrote, and a
question that quietly reached their whole document folder would answer from somewhere they did
not ask about.

**Where this build departs from doc 16, and why.** Section 3.4 describes the ungrounded state as
"`no_passages`; the answer is the model's, marked as such". This build keeps doc 06 section A10's
card: the answer says no sources were found, and the notebook labels it ungrounded. Section 2.1
adopts the ungrounded contract on the grounds that "Tessera already has this in the Synthesizer's
`no_passages` path", which is that card; substituting an uncited model answer into a deep run
would undo the rule that path exists to enforce, that a card which looks answered and is not is
worse than one that says it found nothing. An unverified explanation beside the answer is doc 17
section 5's pattern and arrives with the doctrine flag doc 17 section 8 defines for it, rather
than by quietly changing what a deep card may do.

**The states are computed from what the run found, not guessed from the card.** `CardOutcome`
carries `passages_seen` and `unsupported`, because the citations cannot tell you: a card can
retrieve ten passages and cite none. No passages is ungrounded, unsupported claims are partly
grounded, neither is grounded, and `notebook.grounding.v1` records which with its counts.

**A schema that had not heard of notebooks.** The Router's packet fixed `board.mode` at explore
or learn, so the first notebook question failed validation before any model was asked. Widened to
the four modes the store now allows, `map` included, since doc 17's board is adopted and a
second widening for it would be the same edit twice.

**What 12d-ii carries.** The chat layout, the three states rendered, Save as page and Open on a
board, and the dev-server fixtures Playwright needs. "Search the web instead" ships disabled
until the web retriever exists at 13e. Doc 16 open question 4's superseded rerun goes with the
affordance that reruns, which is the UI's.

**Verified** Full battery green, 56 Playwright tests, the grounded sweep and the bundle round
trip. The two grounding metrics still report n/a, and now name what they wait for precisely: a
run that asks a notebook question, which is an eval leg rather than a missing feature.

---

### BN-119 The notebook on screen, and the retriever rule that had to bend

**Spec** 16 sections 2.1 and 3.4.

**Built** 2026-08-27, M15 12d-ii.

**A chat over the vault.** Turns rather than cards on a canvas: the question, the answer, the
grounding chip, what it read, and two verbs. Save as page is the same verb the board has; Open on
a board asks the question again on an explore board, because a board is where a question grows
follow-ups and a card that arrived without a run of its own would have no trail behind it.

**"Search the web instead" ships disabled and says why.** Doc 16 section 3.4 gives the ungrounded
state a one click way out, and the web retriever is 13e. A control that failed when pressed would
be worse than one that says what it is waiting for, so it carries the reason in its title.

**The rule that had to bend, and how far.** Doc 04 section 10 refuses a plan whose only
retrievers are the profile's own memory: a profile that can corroborate itself learns nothing.
M14.2's step extended that to the vault for the same reason. A notebook question is the one place
that reading is wrong, because doc 16 section 3.4 asks the vault on purpose and restricts the run
to it, so the first notebook question ever asked failed with `no_retriever_enabled`. The Planner
packet now carries the board's mode and the vault counts inside a notebook. What keeps it honest
is untouched: a figure resting on a page alone is still flagged, because that rule is the
Verifier's.

**A fixture that broke another test, and why it was not folded in.** The vault page answers the
same question the dev server's corpus document does. Put it in the memory fixture and the card
that rests on it is flagged for page sole support, and a flagged card is not remembered, which is
the premise doc 15's own test is built on. So the vault is its own reset flag, and each test gets
the fixture its claim needs.

**Two vaults for two states.** A lexical index over one page answers most things somewhat, so
asking about marine biology still came back grounded. The honest way to reach the ungrounded
state is a profile whose vault is empty, which is also the state most people's first question
meets.

**Verified** Full battery green, 60 Playwright tests, the contrast gate extended to the new view,
the grounded sweep and the bundle round trip.

---

### BN-120 Pages travel with the board, and the chip that pointed forwards

**Spec** 01 section 7, 16 sections 2.2 and 3.1.

**Built** 2026-08-27, M15 12-eval.

**The rule, and why it is stricter than the one for documents.** A bundle carries a page only when
the author ticked it. The checklist offers the pages saved from this board's cards, because doc 16
section 3.2 sets `source_card_id` on Save as page and that is the only tie a page has to a board.
A page written in the vault by hand belongs to the person and is offered by nothing. Where a
withheld local document still appears in the manifest as a file name that did not travel, a
withheld page leaves no trace at all: not a row, not a title, not a line. A vault is a person's
own writing, and a list of the notes someone declined to send is itself a disclosure.

**The defect the round trip found.** Doc 16 section 4 gives a card a `page_id` for the chip in its
header. That is the only column in a bundle that points forwards, and pages arrive after the cards
they were saved from, so the foreign key failed and **every board carrying a saved page failed to
import**. Save as page shipped at 12b and nothing had exported a board since. The chip is held
back and re-applied for the pages that actually travelled; a card whose page stayed home keeps no
chip, which is what a chip pointing at a page the recipient does not have would have to mean
anyway. Broken once on purpose: six of the fourteen round trip tests fail with `FOREIGN KEY
constraint failed`.

**Carried evidence is filtered, not remapped.** Doc 16 section 2.2 makes the passages a page
carries the reason it can support a claim at all. Passage ids are ULIDs and survive the trip, so
the map is the identity; what changes is that a passage whose source the author withheld never
arrives, and an entry naming it would offer the recipient a citation they cannot open and the
Verifier cannot bind. Those entries are dropped and counted, so the round trip says how much
evidence stayed behind rather than leaving a page quietly weaker than it left.

**A title collision keeps both, the way a term collision does.** Doc 01 section 7 keeps both
concepts and marks the incoming one proposed. A page has no status to mark and its title is unique
per profile, so the incoming page takes the vault's own conflict suffix and a free file path.
Overwriting would delete something the recipient wrote; refusing would lose something the sender
wrote.

**A link whose target stayed behind arrives unresolved.** Doc 16 section 3.1 keeps an unresolved
link and creates the page when it is clicked. A link arriving at a page that did not travel
becomes exactly that, carrying its title, rather than a foreign key that cannot hold or a link
silently dropped.

**The corpus reaches the boards it exports.** The vault's saved pages are chosen by walking the
card list, which never reached the last three boards, and those are the three doc 02 line 155
ships as bundles. One card on each exported board is now picked first, so the round trip carries
pages on the boards the corpus actually exports. The vault stays at forty pages, three of the
twenty four saved ones changed which card they came from, and the planted page title collision
sits on an exported board that carries no term collision, so a failure names one merge rule rather
than two.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --bundles`. Twenty of twenty boards
arrive whole, 24 pages carried, no carried citation dropped, one term collision and one title
collision handled. The grounded sweep on the rebuilt corpus has no measured metric below its
threshold, `fact_recall_deep` unchanged at 0.923 and `backlink_completeness` at 1.000.

**Verified** Full battery green, 60 Playwright tests, 81 generator guards, the grounded sweep and
the bundle round trip.

---

### BN-121 Two shapes a tree cannot draw, and the enum that would have refused them

**Spec** 16 section 3.5, 06 sections B5 and B8.

**Built** 2026-08-27, M15 12e-i.

**What decides a flow.** Doc 16 section 3.5 gives the reason in one line: a tree has no cycles and
no cross links. So the selection rule is not "relations, therefore tree" any more, it is whether
the relation set is a strict hierarchy, meaning every node has at most one parent and some node has
none. A node reached twice is a cross link and a set with no root is a cycle; both are flows.
Drawing either as a tree would have drawn one node twice or dropped one of the edges, silently.

**What decides a stats tile, and why it is not "few numbers".** Doc 16's own example is "1949,
120m", which is a year beside a size: two quantities, a tile each. Eight beside ten is one quantity
measured twice, and those want a table where they can be read against each other. So the test is
whether the units differ, not whether the values are few. That keeps every existing table a table,
including the shape the grounded mock writes, and it means a stats visual arrives only where the
summary genuinely carries separate figures.

**The uncited tile belongs to one rule, not two.** Doc 16 section 3.5 makes a tile without a
citation a `numeric_without_citation` block flag. The Visualizer therefore never marks a tile
`no_claim`, because that marking is what excuses a block from `visual_block_unbound`, and excusing
it is exactly the case the rule exists to catch. The numeric rule now reads the stats tiles out of
the block index, and the unbound rule skips them: one absence, one flag, which is the rule BN's own
`own_card_sole_support` comment already states for figures in prose.

**The enum that would have refused both.** `schemas/output/visualizer.v1.json` types the agent's
answer with its own list of shapes, shorter than doc 01 section 4.3.1's because the harness makes
some of them and v1.1 reserves others. It had no `flow` and no `stats`, so the first flow the
Visualizer selected was declined at the boundary with a valid payload in hand. The store enum, the
common types and the entity schema all carried the two names since migration 0004; the output
schema was the one place that did not, and only an end to end test could find it, because every
unit test stops before that validation.

**Not hintable, on purpose.** The Router's `visual_hint` keeps its six values. A hint is a guess
made before anything is retrieved, and both new shapes are chosen from structure the Synthesizer
grounded: a flow needs edges and a tile needs a figure with a unit, so a hint for either would name
a shape with nothing to put in it.

**The mock answers in the shape it was asked for.** The grounded mock composed a table whatever the
Visualizer requested, which was invisible while a table was the only thing ever selected. It now
parses the summary out of the prompt as json rather than scraping five prefixes line by line, and
lays it out as whichever shape the prompt names, still using the summary's own labels verbatim so
doc 06 section B8.3 can trace them.

**Measured** 2026-08-27, 400 questions on the grounded mock. Unchanged: every visual is still a
table, `visual_fidelity` 1.000, `fact_recall_deep` 0.923, no measured metric below its threshold.
The corpus has no question that asks for a flow or a set of tiles yet, which is 12e-ii's work, so
`visual_type_match` stays at 0.250 and stays advisory.

**Verified** Full battery green, 107 agent tests, 59 end to end tests, 60 Playwright tests, the
grounded sweep and the bundle round trip.

---

### BN-122 Drawing the two new shapes, and the corpus expectation that was not written

**Spec** 16 section 3.5, 09 section 14, 06 section B12.

**Built** 2026-08-27, M15 12e-ii.

**A flow draws its layers and names every edge.** Nodes sit one layer below the deepest thing that
reaches them, which puts the sources at the top and the ends at the bottom. A cycle has no such
order, so a node already placed stays where it is and the walk is bounded by the node count rather
than running forever. Every edge is then written out below the diagram, labelled and clickable,
because the edge that goes back up is the one a tree could not have shown at all and the one most
worth saying out loud.

**A tile is one block, not two.** The numeral and the label render together and carry one pointer,
because a tile is one thing on the canvas: two blocks would let the Verifier hide half of it and
leave a number on screen with its meaning gone.

**The contrast gate had never seen either.** Both shapes paint their own colours, a large numeral on
a filled block and a small edge label on another, and neither is on screen when the board draws a
tree. The gate now asks a question that produces each one.

**The metric says which shape missed, not how many did.** `visual_type_match` was one ratio over six
shapes: 0.250, which reads as a Visualizer that picks the wrong type three times in four. Split by
what the corpus expected it reads `table 65/65, tree 0/125, list 0/48, steps 0/22`, which says
something else entirely: the selection rule is right every time it is asked for the shape the
grounded mock can write, and the mock writes values and nothing else. A type at zero is either a
wrong rule or a summary shape the provider never writes, and those want opposite fixes.

**The corpus expectation the plan asked for is deliberately not written.** M15's step called for
corpus questions expecting flow and stats. An `expected_visual` is a claim that a good answer to
this question would draw this shape, and nothing planted in this corpus is a cycle or a set of
figures in differing units: every fact is one value, one date, one obligation or one ordered
procedure. Writing the expectation anyway would be ground truth asserting a shape the material
cannot support, which is worse than a recorded gap, and it would score every provider a miss for
declining to invent one. What lands instead is the guard that ground truth may only expect a shape
the canvas draws, mirroring the renderer's own switch, and the breakdown above, which is what will
show the two shapes the day the corpus grows material for them.

**Verified** Full battery green, 63 Playwright tests including both new shapes and their contrast,
82 generator guards, the grounded sweep rescored and the bundle round trip.

---

### BN-123 The sticky nobody could write, and the event that had never fired

**Spec** 01 section 4.5, 16 sections 3.6 and 7 point 1, 09 section 5.

**Built** 2026-08-27, M15 12f, first half.

**A table with no writer.** `note` has existed since migration 0001. Nothing has ever inserted a
row into it: not the shell, not an agent, not a test. `note.added.v1` sat in the event vocabulary
from the same day and had never once been emitted, and the bundle exported `notes.jsonl` empty on
every board it has ever written. The canvas had a sticky in its data model and no way for a person
to put one on the board.

**The column that had to be added to draw the line.** Doc 16 section 3.6 attaches the sticky to its
card by a dashed edge, and doc 01 section 4.5 gives Note a board and a position and nothing else.
Migration 0008 adds `note.card_id` as a nullable ADD COLUMN, so nothing is rebuilt. Null is the
ordinary sticky about nothing in particular, which is what a sticky mostly is, and `ON DELETE SET
NULL` keeps it when the card it quoted is removed: the words were the person's own and they outlive
the card they were written beside. There is a test for exactly that.

**The event a removal writes.** `note.removed.v1` joins the vocabulary, the way doc 11 section 13
added the names its own pass needed. Doc 09 section 5 gives every verb an undo and taking the
sticky off is Add note's, and in an append only log the removal has to be a fact rather than the
absence of one. It carries the board, read before the row is deleted, because an event with no
board never appears in that board's own history, which for the undo of a board verb is the one
place it has to be.

**Where the sticky lands is the canvas's business.** The core has a default place for a caller that
names none, and the board sends its own: beside the card, computed from the layout the reader is
looking at. The core has never seen that layout and a position it invented would put stickies on
top of each other on any board that had been tidied.

**The words are the person's.** A sticky carries no citation, no verdict and no flag, and no agent
reads it. The event records that one exists and how long it is, never what it says: the row is
where a person's own writing is kept, and copying it into the log would put it somewhere the log's
own append only rule would never let them take it back from.

**Verified** Full battery green, 65 end to end tests, 64 Playwright tests, the grounded sweep
unchanged and the bundle round trip whole.

---

### BN-124 Four handles that are not on the card

**Spec** 16 section 3.6, 12 phase 0, 09 section 14.

**Built** 2026-08-27, M15 12f, second half.

**One overlay, not eight hundred buttons.** Doc 16 asks for four handles on hover. Putting them in
the card's markup would add four elements per card and put a hover state into the signature
`render.ts` diffs on, so every pointer crossing a card would rebuild it: doc 12 phase 0's gate is
60 fps pan at 200 cards and the render diff is what earns it. The handles are one element for the
whole board, moved onto the hovered card by reading the transform the layout already wrote. The
card markup is untouched, and a Playwright assertion says so by looking for `data-side` in the
card's own html and not finding it.

**The empty card that cannot exist.** Doc 16 says the drag "creates an empty follow-up card with
the composer focused", and then says the prototype's card footer input does the same thing without
the drag. A Card carries a question and the store requires one, so an empty card would be a row no
pipeline could run and nothing could ever answer. The handle does what the sentence's second half
describes: it puts the cursor in the follow-up box on that card, which is the composer doc 16 wants
focused.

**Out of the tab order on purpose.** Doc 09 section 14 asks that every verb be reachable by
keyboard, and this one is: the follow-up box is a tab stop on every card and is what the handle
points at. Four more stops per card for a pointer shortcut to a control already there would
lengthen the walk through a board for the readers who can least afford it.

**Verified** Full battery green, 65 Playwright tests, the workspace tests and clippy at
`-D warnings`.

---

### BN-125 The notebook leg, and the ungrounded state that cannot happen

**Spec** 16 sections 3.4 and 5, phase 12d.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --grounded --notebook`. Sixteen
vault questions, no provider spend.

**Two gates had been waiting for a leg that was never scheduled.** `grounding_state_accuracy` and
`page_sole_support_rate` both reported n/a saying "the eval leg that does lands with 12d". 12d-i and
12d-ii landed, the notebook works, and nothing ran a question through it: the note named a milestone
that had already passed. `--notebook` runs the vault question set through the ordinary pipeline on
boards of mode `notebook`, one session each, and writes rows the scorer keeps out of every answer
metric because a notebook question is asked over the vault alone.

**The boards retriever is left out of this leg, and that is the point of asking.** Doc 16 restricts a
notebook question to the vault and the profile's own prior cards. With doc 02's twenty prior boards
seeded, every question in the no vault match family was answered from memory: the family exists to
produce the ungrounded state and it measured doc 15's retriever instead.

**The finding: the ungrounded state cannot happen over a vault that is not empty.** With the boards
gone, the eight no vault match questions still retrieved two to eight pages each. A lexical index
always returns its best matches, so `no_passages` never occurs and doc 16 section 3.4's ungrounded
state, which is `no_passages` and nothing else, is unreachable. What a person asking about something
their vault does not cover gets instead is an answer assembled from unrelated pages, labelled partly
grounded. **Every one of the sixteen answers came back partly grounded**, and the two families are
indistinguishable in the result.

The product fix is a relevance floor: passages below it are not passed to the Synthesizer, so a
question the vault cannot answer returns nothing and says so. That is a change to retrieval and the
threshold has to be chosen from evidence, so it is its own step rather than a number guessed here.
The corpus has a second, smaller share of the blame: its no vault match questions carry the same
boilerplate as the pages ("for a small and non-complex institution", "under the internal ratings
based approach"), so they match on phrasing rather than subject.

**`grounding_state_accuracy` is 0.000 and exempted on a mock run.** The grounded mock quotes its
passages into prose the Verifier cannot bind sentence by sentence, so every card carries unsupported
statements and every answer reads as partly grounded whatever the vault did. That is the artefact
`flag_false_positive_rate` is already exempted for, and it would hide the retrieval finding above if
the number were left to speak for itself.

**A new gate that measures the core rather than the model.** `ungrounded_is_no_passages` is doc 16
phase 12d's acceptance sentence: "the ungrounded state appears whenever `no_passages`; never a silent
fallback". The state and the passage count come off the same event the core wrote, so the check asks
whether the core kept its own contract. It is 1.000, and the guard is broken on purpose in the
generator tests with a card that claims to have found nothing while holding four passages.

**`page_sole_support_rate` counts figures, not sentences.** Doc 05 v0.2 line 106 is about a figure
resting on a context-only source. Two of the sixteen answers restate a definition from the reader's
own note, cite pages alone and carry no block flag, and counting them would gate the notebook on
saying anything at all about what the reader wrote. Narrowed to answers stating a figure, the rate is
0.000 against its gate of 0, with 71 block flags raised across the sixteen: the Verifier stopped
every one.

---

### BN-126 The relevance floor is parked, and here is what would settle it

**Spec** 16 section 3.4, 05 section 8.2.

**Decided** 2026-08-27, following BN-125.

**The obvious fix does not exist.** BN-125 asks for a relevance floor so a question the vault cannot
answer returns nothing. The score a retriever returns is a reciprocal rank fusion, and
`index.rs` says what that is in as many words: "the fused score, comparable within one query and
meaningless across two". It is computed from rank alone, so the top hit of a query about marine
biology scores exactly what the top hit of a perfectly answered question scores. A floor on that
number is a floor on nothing.

**The two signals that would work are both out of reach here.** Cosine similarity from the vector
half is bounded and does mean the same thing across queries, and the embedding model cannot be
fetched in this environment: the proxy refuses `huggingface.co`, which is reported rather than
worked around. A floor that needs the embedder would also do nothing on a machine without it, which
is worse than no floor, because the ungrounded state would work for some installs and not others
with nothing saying which. The other signal is a model that answers honestly from its passages, and
the grounded mock is not one.

**The mock cannot judge this at all, and the numbers say why.** Across the sixteen notebook answers,
the eight questions the vault cannot answer came back with *more* supported citations and higher
confidence (0.42 to 1.00) than the eight it can (0.33 to 0.42). The mock quotes its passages
verbatim into the answer, so the Verifier finds the sentence in the passage and calls it supported,
whatever the passage was about. Support here means the answer was copied from the page, which is
true by construction and says nothing about whether the page answered the question.

**Parked, with the evidence it waits for.** No threshold is invented. The step needs either the
embedding model present, so a cross-query similarity exists to put a floor on, or a live run where
support means what it says. Until then the notebook names the pages it read, so a reader can see the
answer came from unrelated notes, and the state it reports is partly grounded rather than grounded.

---

### BN-127 The learning layer's tables, and the columns left empty on purpose

**Spec** 17 sections 2.1 to 2.4.

**Built** 2026-08-27, M16 13a-i.

**Six columns, nullable, and that is the design.** Doc 17 section 2.4 says the self rating prior
applies "only when mastery is null". A default of 0.0 would erase the difference between a concept
nobody has been checked on and one that failed every check it was given, and the prior would then
never apply to anyone. `learning_state` is left null rather than defaulted to `unseen` for the same
reason: 13a-ii makes it a projection, and a row that claims to have been seen and not seen at once
is the state a replay would have to argue with. Every column is an ADD COLUMN, so nothing is
rebuilt.

**An edge is not a link.** Doc 17 section 2.1 puts prerequisite structure in its own table on
purpose. `concept_link` binds a concept to content that mentions it; `concept_edge` says one concept
has to be understood before another. Folding them together would make "prerequisite of" a relation
between a concept and a card, which is not a thing anyone could confirm. The pair plus the relation
is unique, so a planner proposing the same prerequisite twice has proposed it once, and the three
relations doc 17 names can each hold between the same two concepts.

**A mission is why.** Doc 17 plans every lesson against an active mission so difficulty and examples
fit a reason rather than a syllabus, which is the whole difference between this and a course.

**Verified** Full battery green, twelve migration tests including the four checks broken on purpose,
the workspace tests, clippy at `-D warnings`, and the bundle round trip whole at schema version 9.

---

### BN-128 The map folds from the log, and the replay lands before anything writes it

**Spec** 17 sections 2.2, 2.3 and 9.

**Built** 2026-08-27, M16 13a-ii.

**Why the columns are projections at all.** Doc 17 section 9 ends "every mastery change is traceable
to an event". A learning column written outside the log would be a claim about a person that no
replay could check, and the whole point of the map is that it is evidence rather than a
self-assessment. So `card.viewed.v1` moves unseen to exposed, `concept.rated.v1` sets the rating,
`concept.state_changed.v1` carries the state it settled on and the mastery with it, `path.loaded.v1`
adds the path, and the edge and mission statuses fold the same way.

**The replay test lands before anything writes them.** Nothing in the product emits these events yet.
The test writes them by hand, folds them, throws every learning column away, folds the log again and
asserts the map is in the same place, so the first code that writes one has a replay to answer to
rather than a test written afterwards to agree with it. Broken once on purpose by dropping the path
dedupe: the second fold lists the same path twice and the test says so.

**Three names doc 17 gives that the vocabulary does not gain.** `lesson.planned`, `check.asked` and
`check.answered` are the `learn.*` trio the build already emits, per the decision recorded at M14.1:
two names for one meaning in an append only log is the BN-086 failure class. Their payloads gain
`{concept_id, level}` where the Tutor knows them, which is 13c; the fold reads the fields when they
arrive.

**What a rebuild resets, and what it leaves alone.** The six columns go back to null, not to
`unseen`: the fold is what decides a concept has been seen, and a reset that guessed would survive
as a fact. An edge and a mission keep their rows and lose their status, because a proposal a person
made is content and only its state is a projection. A path ships its edges confirmed, so the status
an edge was created with rides on its proposal event and a replay does not quietly demote them.

**Verified** Full battery green, nine replay tests, the workspace tests and clippy at `-D warnings`.

---

### BN-129 Two thresholds called mastery, and the one that had to be named differently

**Spec** 17 sections 2.3, 4, 7 and 8.

**Built** 2026-08-27, M16 13a-iii.

**The name collision, resolved by keeping both.** The packs already carry `mastery_threshold: 2`,
which is doc 14 section 3.6's count of correct checks in one session. Doc 17 section 2.3 wants a
mastery threshold too, and means something else: a score between 0 and 1 on the concept row, default
0.8, judging a person's standing with a concept across every session they have had. Two meanings
behind one name is the BN-086 failure class, so the new one is `mastered_at` and both descriptions
say what the other is. Renaming the shipped one would have been the other way to fix it and would
have broken the Tutor's reading of doctrine for a tidier word.

**Doctrine, not code.** A pack now says when a concept is mastered, how long it stays mastered
without evidence (per domain, with a default), what a check asks at each of doc 17 section 4's four
levels, how it ranks sources for learning rather than for answering, whether the tutor may explain
in its own words, and what it may never explain that way. The guard asserts every shipped pack
answers all of it, because a pack that named none would have the code decide what mastery means,
which is the one thing doctrine exists to keep out of code.

**Silence is not a licence.** `unverified_explanations` defaults to `allowed: false`. A pack that
says nothing does not permit the tutor to explain in its own words, which is the fail closed posture
the Verifier takes everywhere else. Finance allows it and never for numbers or obligations, which is
doc 17 section 8's own example and the rule that keeps the panel from stating a figure nobody
checked.

**Twin parity extended.** The synthetic twin now has to carry the same learning doctrine as the pack
it stands for. A lesson planned against the corpus that opened at a different level, or called a
concept mastered at a different score, would not transfer to the shipped pack, and transferring is
the twin's whole reason for existing.

**The eleventh agent's schemas land before it does.** The packet carries the map, the missions, the
edges and the doctrine, and nothing else: a packet with passages or card text would let the Planner
write a lesson from material instead of from what the learner has done, so the schema refuses any
field it does not name and a test hands it passages to prove it. The output has nowhere to say a
concept is confirmed, which is doc 17 section 7's "proposed, not applied" expressed as a shape
rather than as a rule somewhere that reads it.

**Verified** Full battery green, 31 schema guards including the two new ones, 17 doctrine tests, the
workspace tests, clippy at `-D warnings`, 83 generator guards and the bundle round trip.

---

### BN-130 Mastery moves to the concept, and the old number is derived rather than kept

**Spec** 17 sections 2.3 and 2.4, 14 section 3.6.

**Built** 2026-08-27, M16 13a-iv. The step the plan names as the riskiest, taken alone.

**Two numbers, one name, and the way out.** Doc 14's mastery counted correct checks inside one
session. Doc 17's is a score between 0 and 1 on the concept row, across every session a person has
had. Keeping both as stored numbers would have put two answers to one question in the database, so
the session's count is now derived from the checks the session already records, and the concept's
score is folded from the evidence. The eight `learn.*` RPC shapes and the Tutor packet's
`concepts[].mastery` did not change at all: `learn.spec.ts` and the dev server's tutor arm were
green before and after without being touched, which is what the plan asked for and the evidence that
the relocation did not leak.

**A session written before this keeps its number.** The `mastery` column stays and is read when a
session's checks do not carry their concepts, which is every session recorded before today. Doc 17's
history is a transcript: what an old session recorded is what it recorded, and backfilling it would
put numbers in the log that nobody scored.

**Why the arithmetic runs in the fold rather than on the event.** Doc 17 section 2.4 allows for a
spaced repetition scheduler replacing this rule later. If a check event carried the score it
produced, a new rule would leave two generations of numbers side by side with nothing saying which
was which. Folding the evidence means a rule that changes recomputes every concept from the same
facts. So `learn.check_answered.v1` carries what happened (the concepts, the level, whether the item
was repeated) and never what it was worth.

**Where the doctrine line falls.** The fold does the arithmetic and stops at `checked`, because
whether a score counts as `mastered` is the pack's threshold and `tessera-store` cannot read a pack.
The core says so with `concept.state_changed.v1`, which no longer carries mastery at all: one writer
per number.

**The rule, with its two choices recorded.** `k` runs from 0.15 at level 1 to 0.35 at level 4 on the
line those two points make. A repeated pass counts for half, which doc 17 asks for without naming a
size; a repeated failure is not reduced, because the second time someone gets the same item wrong
says more than the first, not less. Exposure adds 0.02 to a cap of 0.2, so browsing can never look
like learning. A rating sets a prior only when there is no evidence, and never reaches past 0.5.

**Verified** Full battery green, four mastery unit tests, nine replay tests, 62 end to end tests
including the one that reads the session count and the concept score together, 65 Playwright tests,
the grounded sweep unchanged at `fact_recall_deep` 0.923 with no measured metric below its
threshold, and the bundle round trip whole.

---

### BN-131 The rules a model never gets asked

**Spec** 17 sections 2.3, 3 and 4.

**Built** 2026-08-27, M16 13b-i.

**Doc 17 section 7's first half, on its own.** "Frontier selection and level selection are rules;
only decomposition of a new topic into concepts and prerequisites is model work." This module is the
rules, as pure functions of a map and a fact: no store, no provider, no events. That is what makes
them testable against the cases doc 17 describes rather than against a session that happened to
reach them, and it is what the Learning Planner will be, minus one model call.

**A rating never verifies itself.** The frontier is "the lowest prerequisite level where rated
concepts have a rating of 2 or more and mastery is still unverified", and unverified means no check
has moved the score. A concept sitting at exactly the prior its rating set is still the frontier,
however confident the rating was, which is the overconfident rater doc 17 section 3 exists to catch.
Skipping every tile leaves an empty frontier rather than a guess at the first concept.

**Two failures at the bottom look underneath.** The first failure at level 1 is a remedial card on
the concept; the second says the problem is a prerequisite, and the one it opens is the heaviest
confirmed edge. A confirmed edge outranks a heavier proposed one, because doc 01 section 4.10 has an
agent propose and a person confirm, and a planner's confident guess does not outrank what the
learner agreed to.

**A cycle still lays out.** Prerequisite depth is computed with a bounded walk, so a map someone
drew a loop into still orders and the concepts in it sit at the depth their other prerequisites
give them. A learner who draws a cycle gets a lesson, not a hang.

**Decay is a subtraction, not a scheduler.** Only `mastered` decays, because a concept at `checked`
was never claimed to be finished. A missing or unreadable timestamp is not evidence of age, so it
leaves the state alone rather than demoting someone on a parse failure.

**Verified** Full battery green, ten rule tests, the workspace tests and clippy at `-D warnings`.

---

### BN-132 The eleventh agent, and the two fixtures that became one

**Spec** 17 section 7, 02 section 10.1.

**Built** 2026-08-27, M16 13b-ii.

**An agent that mostly does not call a model.** The Learning Planner reads the map, computes the
frontier and the level from 13b-i's rules, and calls a model only when a topic has no concepts yet.
Every other run costs nothing: `decomposing` is the one state that may make a call and most runs
walk straight through it. That is doc 17 section 7's own division, and it is why the rules are a
module of pure functions rather than a prompt.

**The limit is a check, not a request.** Doc 17 allows "at most three new ones", and the packet
carries the number rather than the prompt asking for it. A model that names eight has five dropped
here, an idea already on the map is not proposed again, and an edge naming an idea nobody proposed is
dropped with them: a proposal the learner cannot see is one they cannot refuse.

**A lesson opens at the lowest rung its targets are ready for.** Doc 17 section 5 targets one or two
frontier concepts plus their immediate prerequisites, and the level is the ladder's. A concept nobody
has passed a check on opens at 1, which is a different question from where the ladder goes after a
pass, and asking the second question of the first case is how a first lesson would have opened at
level 2 for a learner who had never answered anything.

**Two fixtures became one.** The dev server answered `tutor` and the eval's grounded mock did not;
the mock answered `exercise` and the dev server's version was its own. A leg that ran under one could
not run under the other, and doc 02 section 10.1 scores the product against a scripted provider, so
two scripts would score two products. Both now read `tessera_core::fixtures`, which builds every
reply from the prompt it was handed: the plan is the topic asked three ways, the check quotes the
card it names, and the decomposition names the parts of the topic the learner joined together. A
fixture that named a subject's prerequisites would be answering the question the eval asks a real
model and scoring itself on the answer.

**Verified** Full battery green, 14 agent rule tests, three fixture tests, the workspace tests,
clippy at `-D warnings`, and 65 Playwright tests including the seven that drive the Tutor through the
fixture that moved.

---

### BN-133 Twenty concepts and four learners who never see each other's answers

**Spec** 17 section 10.

**Built** 2026-08-27, M16 13-eval leg A, first half.

**The split, and why.** The plan's step is the corpus, the eval leg and three metrics. The corpus
half lands alone because it is ground truth and the leg is a driver: a driver written first would
have had nothing to be scored against, and the two together would have been one commit where the
thing being measured and the thing measuring it arrived at once with no way to tell which was wrong.

**Prerequisite depth is a fact about how the path was built.** Every concept at depth n names one or
two prerequisites at depth n minus one, so the ground truth is construction rather than a graph walk
done afterwards and hoped to agree. Four roots, a widening middle, four at the top: a chain of twenty
would make the frontier trivial to guess and a flat twenty would make it meaningless.

**A learner says two things and the product hears one.** The ratings are claims the product reads;
the answers are what the policy can actually get right, per concept and per level, and nothing in the
product ever sees them. That separation is what lets a run be scored against something other than its
own output. The overconfident rater claims to be able to apply all twenty and can answer only the
four at the bottom, which is exactly the case doc 17 section 3's placement flow is written against.

**The expected frontier is computed from the ratings and checked both ways.** The corpus records what
doc 17 section 3's rule should pick before any check is answered, and the generator guard recomputes
it from the ratings rather than trusting the field. When the leg lands, the Planner's own frontier is
compared against this, and a disagreement is one of the two being wrong rather than a number with
nothing behind it.

**Verified** 85 generator guards, the corpus rebuilt at 20 concepts and 24 edges, the workspace tests
and the bundle round trip whole.

---

### BN-134 The map gets a write path, and a path's edges arrive already agreed

**Spec** 17 sections 2.1, 3, 6 and 7.

**Built** 2026-08-27, M16 13-eval leg A, second half.

**A board, because doc 17 says so and because it is cheaper.** The Map is a board of mode `map`,
one per profile, created the first time anything asks for it. Viewport, events and export come free,
and the Planner has somewhere to record `frontier.computed.v1`: a run needs a board and the Planner
is asked about a profile, so without this it would have had nowhere to run.

**The one place an edge starts confirmed.** Doc 01 section 4.10 has an agent propose and a person
confirm, and every edge the Learning Planner draws is a proposal. A path is different: it is
doctrine somebody wrote down, so its edges arrive confirmed and asking the learner to agree to them
would be asking them to check the pack author's work. Loading the same path twice does not double
the map, because the pair plus the relation is unique in the table rather than in the loader.

**A mission is offered, not started.** Doc 17 section 2.1 says a path "offers a mission", and the
statement it ships is a template. A mission created without the learner saying why would plan every
lesson against a reason nobody has, which is the difference doc 17 draws between this and a course.

**What the placement test asserts.** A three concept path, a learner who claims the top and the
bottom of it and has been checked on neither, and the Planner puts them at the bottom: the ratio
sits on the assets, so the assets come first. The lesson opens at recall, nothing was invented, and
the concept the learner rated 3 sits at exactly 0.5 with a state of `rated`, which is doc 17 section
2.4's honesty rule where a reader can see it.

**Verified** Full battery green, 64 end to end tests, the workspace tests, clippy at `-D warnings`,
65 Playwright tests and the bundle round trip whole.

---

### BN-135 Four learners walk the path, and three gates open

**Spec** 17 section 10.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --learner`. Four placements, no
provider spend: the Planner's model call only fires for a topic with no concepts, and a path has
them all.

| Metric | Result |
|---|---|
| `frontier_correctness` | 1.000, four of four (reported: n=4 is under the thin sample floor) |
| `proposals_never_applied` | 1.000 |
| `mastery_honesty` | 1.000 |

**What the leg does not read.** The corpus records what each learner could actually answer, per
concept and per level, and the leg never opens it. A placement is decided before any check is asked,
and reading the answer sheet while marking the paper is exactly the failure the two files exist to
prevent. The scorer reads the ratings and the expected frontier; the answers wait for 13g, where a
check is asked and the overconfident rater is caught.

**Proposals applied are counted from the map, not from the output.** The Planner's own answer says
what it proposed, and trusting it would score the agent on its own report. What the leg counts is a
confirmed edge the path did not draw, which is the only shape a proposal quietly applied could take.

**The thin sample guard did its job.** Four learners is four, and one either way would flip the
frontier ratio by a quarter, so the scorer reports the number and withholds the gate. That is the
existing floor working as written rather than a new exemption: doc 17 section 10 asks for 0.90 and
the run says 1.000 over a denominator too small to hold it to that.

**Verified** Full battery green, 86 generator guards including the three metrics broken on purpose,
the workspace tests, clippy at `-D warnings`, and the bundle round trip whole.

---

### BN-136 The exercise ladder, with the pack holding the mapping

**Spec** 17 sections 4 and 9, doc 08 section 5.

**What landed.** Exercise items gain doc 17's `level: 1..4`, the kind enum grows additively with
`explain` and `discriminate`, the distractor rule gains its level 4 clause, and `exercise.create`
takes a level so a lesson can ask at the rung a learner stands on.

**The level to kind mapping is doctrine and never code.** The plan recorded 1 recall, 2 explain, 3
apply, 4 discriminate, and the temptation was to write those four lines into the agent. They live in
the pack instead, as the `check_templates` 13a-iii already added, and the packet carries them. The
agent asks the ladder two questions: which kinds does level n want, and which level does kind k sit
at. A pack that declares three levels has three, and an item whose kind no level claims carries no
level at all rather than a number invented to fill the field. `general`, `finance-eu` and
`finance-eu-synthetic` each put `trace` at 3 beside `apply` and `contrast` at 4 beside
`discriminate`, which is a pack's choice to make and not the agent's to assume.

**The rung asked for beats the rung the kind sits at.** A pack may list one kind at two levels, and
the learner was put on one of them. The tutor's next check moves from where they stood, so the
asked level wins and the kind's own level is the fallback for an exercise nobody asked a level of.
Broken on purpose: dropping the asked level leaves the kind mapping standing and the unit test
fails, which is the point of testing the two paths separately.

**The level 4 distractor rule needed the concepts, not the cards.** Doc 17 asks that a level 4
distractor not be a true statement about a neighbouring concept. A discriminate item puts the
neighbour on the page by design, so the rule cannot be "does not mention the neighbour": that would
drop every level 4 item there is. What it checks is the neighbour's own definition, from the
concepts the packet already carried, and only for concepts the item does not name. A neighbour with
no definition contributes nothing, because a term is a name and not a claim.

**A null is not an absent field.** The first version wrote `"level": null` into the packet when
nothing asked for a level. The packet schema types it as an integer, so every exercise on every
board was refused at the boundary until the key was left out entirely. Two end to end tests caught
it before anything was committed, which is fail closed at the Verifier working one layer down.

**A surprising number split before a fix was built.** The first exercise leg reported `levels asked:
1, 2` while records existed at all four. Splitting by dimension: fifteen exercises, four with a card
worth testing, and those four happened to be the boards the rotation had put on rungs 1 and 2. The
cause was the sample, not the ladder. The leg now filters to boards with an eligible card before
assigning a rung, and says how many of its boards had one.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --grounded --exercise --limit
60`. Fifteen items over four rungs.

| Metric | Threshold | Result |
|---|---|---|
| `exercise_traceability` | 1.00 | 1.000 |
| `exercise_distractor_leakage` | 0.0 | 0.000, levels asked 1, 2, 3, 4 |
| `exercise_level_agreement` | 1.00 | 1.000 |

Items per rung: 6 recall, 3 explain, 3 apply, 3 discriminate, each matching the kind its pack puts
at that level.

**What the mock cannot say.** It writes the same item at every rung and only the wording moves, so
this measures whether the ladder is plumbed and never whether a discriminate question is harder than
a recall one. That second thing needs a model, and it sits with doc 08 section 12's "answerable by a
second model" on the spend list.

**Verified** Full battery green: workspace tests, clippy at `-D warnings`, fmt, style lint, 88
generator guards with both new ones broken on purpose, 65 Playwright tests, the exercise leg above,
and the 20 board bundle round trip whole.

---

### BN-137 The ladder moves where grading already lives, and a lesson stays on its concept

**Spec** 17 sections 4, 5 and 6.

**What landed.** `record_check` now runs doc 17 section 4's adaptation rule and returns what it
decided: whether the answer was right, the rung it stood on, the rung the next check on that concept
opens at, and the remedy a failure calls for. The Tutor's check selection comes from the Planner's
targets and level rather than from a free choice, item sourcing follows doc 17's order, and the
learner leg now teaches as well as places, which is what made both gates measurable.

**Two rules about a rung, and they answer different questions.** Doc 17 section 5 has the Planner
pick the level a lesson opens at, from `difficulty_level`, which section 2.1 defines as the last
check the learner passed. Section 4's ladder moves the level within a lesson, from the last check
they took, pass or fail. Reading either one as the other breaks the other: a plan level alone never
drops after a failure, and a ladder level alone has nowhere to start. So the Planner sets the
opening rung and the session's own transcript moves it from there. Nothing new is stored: the
session already records every check, and the rung is a function of the last one.

**A lesson stays on the concept it is checking.** The first version recomputed the frontier every
turn, and one passed check takes a concept off the frontier, so the second check of every lesson
was about a different concept at level 1 again. Doc 17 section 4 says "the next check on that
concept", so a lesson carries its target until the concept is mastered and only then asks the
frontier what comes next. The end to end test that caught this asserts three checks in a row open at
1, 2, 3.

**A check names the concept it checks.** The remedy needs a row to move, and the shell had no way to
say which. The pipeline stamps `check.concept_id` from the plan's target after the agent answers,
which keeps it out of the model's hands: a concept the tutor named for itself would be a check about
something nobody put the learner on. The shell hands it back when the answer is graded, and the eval
leg reads the same field.

**The sourcing order is structural, not a prompt.** Doc 17 section 4 asks for the lesson board's
verified cards first, then verified cards anywhere on the map, then a request for a card before
checking. The packet says which of the three it used, and "no item is ever generated from unverified
text" holds because a packet carrying no unverified card cannot offer one. At `none` the tutor is
told to open a card and write no question, and any item it wrote anyway names a card the packet does
not carry, which the traceability rule already drops.

**Two nulls found by running it.** The Tutor set `next_if_right` and `next_if_wrong` to null when
the overlap rule dropped them, and the output schema types both as strings, so the whole turn was
refused at the boundary. The path had never run: the dev fixture always overlapped. The fix is the
one this agent already applies to its top level fields, which is absent rather than null. The same
class as the packet level null in BN-136, found the same way, one day apart.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --grounded --learner`. Four
learners, twenty four checks, twenty ladder steps.

| Metric | Threshold | Result |
|---|---|---|
| `level_adaptation` | 1.00 | 1.000 |
| `checks_from_verified_cards` | 1.00 | 1.000 |
| `frontier_correctness` | 0.90 | 1.000, reported at n=4 |
| `proposals_never_applied` | 1.00 | 1.000 |
| `mastery_honesty` | 1.00 | 1.000 |

The rungs each learner was asked at, which is the ladder as a sentence:

| Learner | Rungs asked |
|---|---|
| always-right | 1 2 3 4 4 4 |
| right-below-three | 1 2 3x 2 3x 2 |
| random | 1x 1x 1x 1x 1x 1x |
| overconfident | 1 2 3 4 4 4 |

`x` marks a failure. The second learner is the shape the rule exists for: right up to level 2, and
the ladder holds them between 2 and 3 rather than letting them climb.

**What the mock still cannot say.** It writes the same item at every rung, so a learner who can
answer a concept at level 1 answers it at level 4 too. The rungs above measure that the ladder moves
correctly, never that a level 4 question is harder. That needs a model and sits on the spend list
with doc 08 section 12's second opinion.

**Verified** Full battery green: workspace tests including four new end to end ones, clippy at
`-D warnings`, fmt, style lint, 89 generator guards with both gates broken on purpose, 65 Playwright
tests, a 60 question grounded sweep with nothing below threshold, and the 20 board bundle round trip
whole.

---

### BN-138 The Map, and a claim that was being shown as a score

**Spec** 17 section 6.

**What landed.** An eighth rail view: concepts as nodes laid out in bands by prerequisite depth,
sized by the cards linked to them, coloured by doc 17 section 2.3's six states, with confirmed edges
solid and proposed ones dotted, the frontier as a band behind the nodes, filters by state and by
mission, and a node panel carrying the rating, the score, the evidence, what covers the concept, and
the three verbs that put a learner on a board.

**The depth and the frontier come from the core.** Both are rules the product already owns, in
`tessera-agents`, and `map.read` runs them and ships the answers. The alternative was handing the
view a pile of edges and having it derive the same two things in TypeScript, which is a second
implementation of a rule the eval gates in Rust: the day they disagreed, the wrong one would be the
one on screen. The view draws what it is told and holds no rule of its own.

**One SVG rather than two hundred divs.** A card on the canvas is a DOM node because it holds text a
person selects and a menu they open. A map node holds a term and a click, so the whole map is one
SVG and the 200 card pan gate is untouched by a view that could have a thousand nodes.

**A claim was being shown as a score, and a Playwright test found it.** Doc 17 section 2.4 gives a
rating a starting prior of 0, 0.15, 0.35 or 0.5, so a concept nobody has checked still carries a
number. The panel's first version printed it as `Score 35%`, which hands a learner their own guess
back as evidence, in a product whose whole argument is that the two are different. The panel now
says which of the two the number is, and adds that no check has confirmed it. Doc 17 section 2.1's
"a rating is a claim, never evidence" is a rule about the interface as much as about the column.

**The frontier is at the bottom of what a learner claimed, not the top.** The test asserted the
frontier sat where the ratings ran out; the product put it on the shallowest rated concept instead.
The product is right and doc 17 section 3 says so: "the lowest prerequisite level where rated
concepts have a rating of 2 or more and mastery is still unverified". That is the rule that catches
the overconfident rater within two questions, and a frontier at the deep end would ask them the
hardest thing first and learn nothing. The test and the dev fixture's comment were corrected to the
rule.

**A verb that started a lesson the shell did not know about.** The Map's Start a lesson called
`learn.start` through the RPC directly, so a session existed and the panel that renders it never
opened. The shell owns that state, so the Map asks for a lesson through a router action and the
shell starts it, which is the same split every other cross view verb already uses.

**Verified** Full battery green: workspace tests including a new end to end one over `map.read` and
`map.concept`, clippy at `-D warnings`, fmt, style lint, 89 generator guards, 71 Playwright tests
including six new ones over the map and the contrast floor extended to it, and the 20 board bundle
round trip whole.

---

### BN-139 The web retriever, and the two rules that make a socket safe to open

**Spec** 05 section 8.1, doc 16 section 3.4.

**What landed.** `tessera-retrievers::web`: fetch, main content extraction, heading and window
chunking, BM25 over what was fetched, and a Source per page deduplicated by normalised URL, all
under class `web`. Wired into `RetrieverSet`, reachable from a card, and behind doc 16's one click
way out of an ungrounded notebook answer.

**Nothing is reached that was not pointed at.** A profile names seeds; discovery walks a seed's
links and drops every one that leaves its host. That is not a setting, it is the shape of the
walk, and it is what makes the whole leg structurally incapable of leaving the machine when the
seeds are loopback. Doc 05 section 8.1's domain denylist is the second gate, not the first.

**The hooks run per URL, not per assignment.** The fan-out checks the assignment before the
connector is called, and for the web that check has no URL to look at: only this module knows
them. So it runs the same `HookSet::retriever_defaults()` again on every candidate, before the
fetch. A denied domain is never opened, which the loopback test asserts as an absence: no
passages and no fetch errors, because nothing was reached at all.

**The same bytes give the same rows.** The content hash is a sha256 of the body as fetched, ties in
the ranking break by position, and two runs over one server produce identical passage ids. Doc 05's
whole staleness story rests on a hash that means something, and a retriever whose output moved
between sweeps would make every number downstream of it unreadable.

**Search is deliberately absent, and that is recorded rather than hidden.** Doc 05 section 8.1 opens
with a search API and the user's key, which is live and paid. What decides whether a citation is any
good is the rest: fetch, extract, chunk, rank, persist. That half is measurable for nothing against
the synthetic web, and the day a key arrives, search becomes one more way to produce candidate URLs
rather than a rewrite.

**A directory listing is not a page.** The first version indexed the listing `gen serve` produces,
which is a source whose entire content is the names of other sources, and it would rank against any
question sharing a word with a file name. The test is text that is not a link: a page with links on
it is still a page, a page that is only links is the index it looks like.

**Two questions doctrine and the profile answer separately.** Whether a domain may use the web at
all is the pack's (`enabled_by_default`); where it may read is the profile's (`web_seeds` in
`retriever_config`, set by `profile.watch_web`). Neither alone configures it, which is doc 05
section 10's "not configured" against "configured and empty" kept apart.

**Measured** Nine unit tests over a fixture fetcher, three integration tests over a real loopback
socket, and one end to end test that takes a notebook question from ungrounded to a `web` source
with a loopback locator and a 64 character hash. A 40 question grounded sweep after the change ran
with nothing below threshold, 29 of 50 metrics measured, and the 20 board bundle round trip whole.

**Verified** Workspace tests, clippy at `-D warnings`, fmt, style lint, 89 generator guards, and 71
Playwright tests, including the notebook test that now presses the button rather than asserting it
is disabled.

---

### BN-140 The Planner is told what doctrine wants, not what the profile has

**Found** 2026-08-27, while wiring the web retriever.

**What it is.** The Planner's packet carries a retriever list built from `pack.enabled_by_default`,
while the fan-out builds assignments from `RetrieverSet`, which additionally requires the profile to
have said where to read. So the Planner can plan an assignment against a connector the fan-out will
skip, and doc 04 section 10's `no_retriever_enabled` (whose message reads "Enable at least web or
local in Profile") is decided from something Profile does not control.

**Why it is not fixed here.** The one line fix is `r.enabled_by_default &&
ctx.retrievers.configured(&r.id)`. It was written, and it turned eight end to end tests red: they
ask deep questions on profiles with a finance pack, no watched folder and no seed, and today the
Planner's fiction is what lets those runs proceed. Under the correct rule they are exactly doc 04
section 10's case, and each one has to be re-founded on a profile that has configured something.
That is a coherent change and it is not this step's: shipping it inside the web retriever would mix
a new connector with a re-basing of the test suite, and the failure signal from either would be
unreadable.

**What it waits for.** Its own step, before the research profile in 13e-ii, since that step gives
the profile something real to configure and will want the honest answer.

**Resolved** 2026-08-27, in the step below.

---

### BN-141 The Planner now reads the configured set, and fourteen tests said what that changed

**Spec** 04 section 10, doc 05 section 10. Closes BN-140.

**The change is one line.** The Planner's packet marks a retriever enabled when the pack enables it
**and** the profile has told it where to read. Everything else in this step is the fourteen end to
end tests that were resting on the other answer.

**What they were resting on.** Each asked a deep question on a profile carrying a finance pack, no
watched folder and no seed. Under the old rule the Planner was told regulatory, local and web were
available, planned assignments against all three, and the fan-out then skipped every one as
`connector_unavailable`. The card came back with no sources and the run looked like doc 06 section
A10's honest thin card. It was not: doc 06 section A10 is retrieval that found nothing, and this was
a profile with nothing to retrieve with, which is doc 04 section 10 and names its own fix. Two
failures that read identically on a board and mean opposite things to the person holding it.

**The fix per test is a premise, not an assertion.** Twelve of them gained `with_empty_folder`,
which is doc 05 section 10's "configured and empty": a watched folder with nothing indexed, so the
retriever runs and finds nothing, which is what those tests were always about. Two assert which
retrievers the Planner picks for a governed domain, which can only be read on a profile where all of
them are available to pick from, so they gained `with_every_retriever`. One memory test had a
hand-built set holding `boards` alone; doc 04 section 10 refuses that plan on purpose, because a
profile that can only read its own prior cards corroborates itself, so it gained `local` beside it.

**What is still not configurable.** `regulatory` has no product path at all: a corpus subscription
is a later phase, so `assemble` has no arm for it and the one test that needs it says so where it
adds it by hand. That is doc 05 section 10's "not configured" being honest rather than a gap this
step left.

**Verified** Full battery green: 71 workspace end to end tests including a new one that fails when
the line is reverted, clippy at `-D warnings`, fmt, style lint, 89 generator guards, 71 Playwright
tests, a 40 question grounded sweep with nothing below threshold and 29 of 50 metrics measured, and
the 20 board bundle round trip whole.

---

### BN-142 The research profile, and a fetch budget that starved the last site

**Spec** 17 sections 5 and 8, doc 05 sections 8.1 and 12.

**What landed.** A card asked on a lesson board reads with doc 17 section 5's research posture:
the pack's learning quality ranking instead of its source hierarchy, a larger fetch budget, the set
narrowed to web, vault and boards, and the path's own `sources_hint` locators reaching both the
Planner and the retriever as `must_include`. Migration 0010 puts the hints on the mission. A new
eval leg measures doc 05 section 12's web recall against the synthetic web on loopback.

**Two rankings, two questions.** `source_hierarchy` answers "who has authority over this claim";
doc 17 section 8's quality ranking answers "who explains it best". A pack gives different answers,
and the second is what a lesson reads. A pack that declares no quality ranking falls back to the
first, which is the honest degrade: one ranking beats none.

**`must_include` is not the twin of `must_exclude`.** An exclusion is a rule a retriever may not
break; a hint is a place it is told to look. So a hinted locator is fetched even when no listing
links it, and a hint on a host the profile never named is refused: a path naming the open internet
is a path asking for something the seeds already said no to. A hint that answers nothing still
contributes nothing, because it is ranked like anything else.

**The number that surprised, split before anything was built.** The first web leg reported 24 of 40,
against a gate of 0.80. Split by fidelity it read paraphrase 12/20 and partial 6/14, which looks
like a ranking problem and would have led to fiddling with BM25. Split by the host carrying the
expected page it read `vaultworks.invalid` 0 of 16 and every other host 26 of 26. Not a ranking
problem at all: that site's pages were never fetched.

**Why they were never fetched, and what that says about the product.** Doc 05 section 8.1 caps the
fetch, and the cap was being spent down the seed list rather than across it. The corpus serves 38
pages over four hosts; the first three hosts hold 31 of them, so a budget of 32 reached the fourth
site's first page and no further. A search API ranks across sites before the cap applies; a crawl
has no such ranking, so spending the budget in order makes a profile's later sites invisible. The
fix is breadth first: every site the profile named is read before any site is read twice. That is
the profile's own choice of sites meaning something.

**And then the leg's own budget was the measurement.** With the order fixed the number went to 30 of
40, and every remaining miss was a `web-payments-*` page: 38 pages and a budget of 32 leaves six
unread whichever order they are read in. Doc 05 section 12 says this number "measures extraction and
ranking", so the crawl must not be inside it. The leg now sets its budget from the corpus's own page
count, and the crawl question is the product's, measured by the breadth first change above rather
than by a gate that conflates the two.

**One perfect number, and the harder one beside it.** Recall at k reads 1.000 over 96 planted facts.
That is real and it is easy: k is ten passages out of 38 pages, and the gate exists to catch a page
that was never reached. Whether the right page ranks *first* is the question the gate cannot ask,
and it reads 0.812, split by plant fidelity: exact 5/6, paraphrase 43/47, partial 26/37. Reported
rather than gated, because doc 05 section 12 sets recall at k and says nothing about rank, and the
two move for different reasons: a crawl that missed a site fails the first, a BM25 that preferred a
page sharing a word fails only the second.

**Measured** 2026-08-27, `gen serve --seed 42` plus `tessera-eval --corpus synthetic/42 --web`, 96
planted facts, no provider and no network beyond loopback.

| Metric | Threshold | Result |
|---|---|---|
| `web_recall_at_k` | 0.80 | 1.000 |
| `web_top_source_is_the_right_one` | reported | 0.812 |

**Verified** Full battery green: workspace tests including a new unit test over the two rankings and
a new end to end one over the lesson posture, clippy at `-D warnings`, fmt, style lint, 90 generator
guards, 71 Playwright tests, a 40 question grounded sweep with nothing below threshold, the learner
leg unchanged at four rungs walked, and the 20 board bundle round trip whole.

---

### BN-143 The carried citation that named no passage, since the day the column landed

**Spec** Doc 16 section 3.2.

**What was wrong.** Save as page copies the card's citations rather than re-deriving them, and doc
16 section 3.2 gives the carried shape as `{ordinal, passage_id}`. Every page written since 12b
carried `{"ordinal": 1, "passage_id": ""}`: `repo::read_citations` never selected `c.passage_id`,
so `save_card_as_page` read a key that was not in the row and wrote an empty string in its place.

**Why nothing caught it.** The test asserted a count. Twenty four pages in the corpus carried
citations, the bundle round trip reported "0 carried citations dropped", and the number was right
every time: there were as many entries as the card had, and each one pointed nowhere. A count is
not a claim about what is in the entries, and the whole point of carrying evidence is that a figure
on a page still rests on the passage the card cited.

**The fix and the assertion.** One column in one `SELECT`, and the reader now hands back what the
schema always said it did. The Save as page test looks up every carried passage in the `passage`
table, so a carried citation that names nothing fails rather than counts, and doc 17 section 10's
new traceability gate reads the same rows for the same reason.

**Verified** Probed before and after on the same board: `CARRIED [{"ordinal":1,"passage_id":""}]`
became a ULID the `passage` table holds.

---

### BN-144 The lesson ends as a page, and every line of it names a row

**Spec** Doc 17 sections 5, 9 and 10, phase 13f.

**What landed.** `learn.end` writes doc 17 section 5's learning record: a page under
`vault/learning/<mission>/<date>.md` with what was covered, what was checked and what remains,
carrying the cards' own citations. The page is generated from rows, never from a model. Covered
comes from the same eligibility the Exercise agent uses, so a record cannot list a card the
Verifier refused; checked comes from the session's own check rows; remains is the concepts this
lesson asked about and left failed, which is not the same as the next rung. `vault::write_page_in`
grew the folder argument 12a-ii promised, and the shell tells the learner where the page went.

**Why the citations are carried rather than re-derived.** Doc 16 section 3.2 settled that for Save
as page and the reason holds here: the record is a note about cards, and a figure on it rests on
the passage the card cited rather than on the page having repeated it. Reading them off the
exercise packet is the mistake the first version made, because that packet's citations are
`{n, source_title}`, which is what an item may ask about rather than what the evidence is. The
board's own citations carry the passage id, which is also what made BN-143 visible.

**The gate reads the log, not the markdown.** `learning_record.saved.v1` gained a `lines` array:
one entry per line of the page, naming the card, the check or the concept behind it. The learner
leg writes it out with each carried passage looked up in the store, and the scorer asks the
session's own rows whether each line is there. A record that parsed its own markdown back would be
measuring a formatter; this measures whether the rows exist. The guard breaks four ways on purpose:
a card the Verifier never stood behind, a check at a rung nobody was asked, a concept named as open
that the learner passed, and a carried citation whose passage is not in the store.

**What the fixture had to become.** The end to end test first built its card over a watched folder
and got a record with no evidence on it: `LESSON_RETRIEVERS` is web, vault and boards, and doc 17
section 5 leaves local out, so nothing answered. A vault page alone does not work either, because
doc 16 section 3.2 makes a page context rather than evidence for a figure and
`own_card_sole_support` blocks the card, which then is not one of "the lesson's verified cards".
The fixture now serves one page on loopback, which is what a lesson can actually cite.

**One thing this found and did not fix.** The Planner packet is built from the profile's configured
retrievers with no regard for the posture the run will use, so a lesson on a profile with only a
local folder passes doc 04 section 10's `no_retriever_enabled` and then retrieves nothing, and a
notebook question is planned against retrievers the fan-out will skip. Notebook mode is special
cased inside the Planner's own guard, which is the same disagreement written once. That is BN-140's
class one layer up and it is a change with a wide blast radius, so it is named here rather than
folded into a step about records.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --grounded --learner`. Four
lessons, 72 record lines.

| Metric | Threshold | Result |
|---|---|---|
| `learning_record_traceability` | 1.00 | 1.000 |

**Verified** Full battery green: workspace tests including a new end to end one that walks a lesson
from a loopback page to a saved record, clippy at `-D warnings`, fmt, style lint, 91 generator
guards with the new gate broken four ways, 72 Playwright tests, a 40 question grounded sweep with
nothing below threshold and 29 of 53 metrics measured, and the 20 board bundle round trip whole.

---

### BN-145 Placement, and the check that comes before the teaching

**Spec** Doc 17 sections 3 and 10, phase 13g-i.

**What landed.** Doc 17 section 3's placement, on the Map: tiles in prerequisite order, four
tappable levels each, any of them skippable. The order is depth then term, which is the order the
map lays its bands out in, so a learner rating top to bottom meets what everything else rests on
first. Placement opens itself the first time there is anything unrated and never reopens behind a
learner who left it; the way back stays on the toolbar while anything is unrated.

**A skip writes nothing.** A rating is a claim the product records as `concept.rated.v1`. Declining
to make one is not a second kind of claim, so a skip is client state and the concept is still
unrated when the learner comes back. The first version hid the way back into placement once every
tile had been skipped, which is exactly when somebody would want it; the toolbar now reads what is
unrated rather than what is on the tiles.

**The check before the teaching.** Doc 17 section 3: "the first lesson checks the frontier before
teaching anything". A lesson whose topic is a concept the learner rated 2 or more and nobody has
checked opens with the check rather than with doc 14's intake, because placement already asked how
much they know and teaching first would be teaching on the strength of a claim. A check that
produced no item falls back to intake rather than opening a panel with nothing in it: doc 17
section 4's sourcing order ends at "request a card first", and a profile with no verified card
anywhere has nothing to ask about yet. The frontier's own filter is now a named rule,
`learning::unverified_claim`, so the frontier and this both read one spelling of it.

**The policy that caught nothing.** The corpus's overconfident rater rated 3 everywhere and could
answer everything at depth 0 and nothing above it. The frontier is the lowest depth a learner
claims, so that learner was placed exactly where they were genuinely right, passed every rung, and
the flow that exists to catch them had nothing to catch. It now claims 3 everywhere and can only
recite: level 1 passes, level 2 is where the claim ends, and the run shows `1 2x 1 2x 1 2x`.

**0.667 that was a definition, not a defect.** The first reading of the gate counted any failed
check on a concept rated 2 or more, and read 0.667 over three rows. Split by rating and level, the
row dragging it down was the learner who is right below level 3: they rated themselves 2, which
doc 17 section 2.1 says is "can explain it", passed levels 1 and 2, and failed at 3. That is the
ladder finding the ceiling of an honest claim rather than catching a false one. An overclaim is a
check failed at or below the level the rating claimed, and on that reading the metric is 1.000 over
two rows.

**Two rows is thin and the harness says so.** A lesson stays on one concept until it is mastered,
so each scripted learner contributes at most one row and only two of the four claim something they
cannot do. The value is reported and the gate is not applied, which is the thin sample floor
working. More rows would need the leg to run a lesson per frontier concept rather than one per
learner, which is a change to what a session means and is not this step's.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --grounded --learner`.

| Metric | Threshold | Result |
|---|---|---|
| `overconfident_rating_caught` | 0.95 | 1.000, n=2, reported |

**Verified** Full battery green: workspace tests including a new end to end one over the check
first rule, clippy at `-D warnings`, fmt, style lint, 92 generator guards with the metric's own
three cases, 74 Playwright tests including two over the placement tiles, a 40 question grounded
sweep with nothing below threshold and 29 of 54 metrics measured, and the 20 board bundle round
trip whole.

---

### BN-146 Home, exposure, and the failed check that was reading as a pass

**Spec** Doc 17 sections 2.2, 2.3, 6 and 10, phase 13g-ii.

**What landed.** Doc 17 section 6's last line, on Home: per mission, the fraction of concepts at
checked or better and the current frontier concept, named rather than counted, because what a
learner wants from that line is what to do next. Both are rules, so `mission.summary` answers them
and Home draws the answer. The frontier is read over the whole map rather than the mission's slice:
a prerequisite outside the mission still has to come first.

**Exposure has a producer at last.** `card.viewed.v1` has been in the vocabulary and in the
projection since 13a-ii with nothing writing it. The shell writes it now, because only the shell
can see reading: an `IntersectionObserver` starts a clock when a card is more than half in view and
`card.viewed` fires when the clock reaches `EXPOSURE_MS`. Doc 17 open question 2 says three seconds
is a guess, so it is one named constant rather than a number spread through handlers.

**Dwell rather than appearance.** Reporting every card on screen would mark a whole board read at a
glance, and a map filled that way says the learner has met twenty ideas when they have seen a wall.
Once per card per shell, too: exposure is capped at 0.2 anyway, and a log carrying a line every time
a card scrolled past would be a log about scrolling.

**The gate found a real one.** Map state consistency read 0.988 on its first run. Split by learner
and concept, the single disagreeing row was the random learner: six failed checks, no pass, and the
map showing `checked`. Doc 17 section 2.3 gives that state one meaning, "at least one passed check
at level 1 or 2", so `state_after_check` was promoting a claim the check had just contradicted.
Worse than a wrong colour: `verified` reads that state, so one wrong answer took the concept off the
frontier and moved the learner on from the thing they had just got wrong. A failed check now
demotes `mastered` to `checked` and leaves everything below where it was.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --mock --grounded --learner`. Eighty
concept rows across four learners.

| Metric | Threshold | Result |
|---|---|---|
| `map_state_consistency` | 1.00 | 1.000 |

**Verified** Full battery green: workspace tests including two new end to end ones over exposure and
the Home summary, clippy at `-D warnings`, fmt, style lint, 93 generator guards with the new gate
broken three ways, 76 Playwright tests including the dwell and the mission line, a 40 question
grounded sweep with nothing below threshold and 29 of 55 metrics measured, and the 20 board bundle
round trip whole.

---

### BN-147 One narrowing, read by everything

**Spec** Doc 04 section 10, doc 16 section 3.4, doc 17 section 5. The defect BN-144 named.

**What was wrong.** A notebook question narrowed its retriever set in `Core::ask`, so everything
downstream read the narrowed set, the Planner packet included. A lesson narrowed inside the fan out
instead. So on a lesson board the Planner was told `local` was enabled, could assign a sub question
to it, and the run then skipped it: the card came back thin and nothing said why. Worse, doc 04
section 10's `no_retriever_enabled` reads the same list, so a profile whose only retriever is a
watched folder passed the guard and then retrieved nothing at all.

**The fix.** Both narrowings happen in one place, matched to the board mode, and `Posture` keeps
only what is genuinely a property of the run: the path's `must_include` locators and the lesson's
wider fetch budget. The Planner's guard is unchanged in rule and honest in message: on a lesson it
says "This lesson has nothing to read. Add a site to search in Profile", because doc 17 section 5
leaves local out and telling a learner to add a folder would send them to set up the one retriever a
lesson never reads.

**What changes for a learner.** A lesson on a profile with no site to search is refused at the top
with a sentence that names the fix, instead of producing a card with no sources under it. An
ordinary board on that same profile still reads the folder, because the narrowing is a property of
the board rather than of the profile, which is the assertion the new test ends on.

**Verified** Full battery green: 77 workspace end to end tests including a new one that fails when
the narrowing moves back, clippy at `-D warnings`, fmt, style lint, 93 generator guards, 76
Playwright tests, a 40 question grounded sweep with nothing below threshold and 29 of 55 metrics
measured, the learner leg unchanged with all seven learning gates at 1.000, and the 20 board bundle
round trip whole.

---

### BN-148 The first live sweep past a dozen questions, and a gate that was measuring nothing

**Spec** Doc 02 section 10, the parked full sweep.

**What ran.** Forty questions on Anthropic, the first live measurement since BN-105's twelve.
Haiku 4.5, Sonnet 5 and Opus 5 across the tiers, 226 model calls, 501k input and 138k output
tokens, about 14 dollars at BN-105's rate. Thirty nine of forty produced a card. The fortieth,
`Q-0010` at deep, died on `schema_violation`: the provider returned no parsable json object, which
is BN-103's class and still the one failure only a live run can show.

**Kimi carried none of it.** Doc 02's design is 95 percent bulk on Kimi with a 5 percent Anthropic
reference sample to calibrate against. `api.moonshot.ai:443` is refused by the session's egress
policy, so the bulk leg could not run and this is the reference leg standing alone. That makes these
product quality numbers rather than calibration numbers: calibration is the comparison, and there
is nothing yet to compare against. Reported here as what it is rather than filed as the sweep.

**The Router collapses toward deep.** `route_accuracy` reads 0.675 against a 0.85 gate, and split
by the depth the corpus expected it is one behaviour rather than thirteen errors:

| Expected | Recommended | |
|---|---|---|
| deep | deep 27, fast 3 | 27/30 |
| research | deep 6 | 0/6 |
| fast | deep 4 | 0/4 |

Every research and every fast question was recommended deep. The Router is good at the middle and
recommends nothing else at the edges. Six and four are small denominators, but a miss rate of one
with a consistent direction is a finding rather than noise.

**A gate that could not have passed.** `flag_false_positive_rate` read 1.000 against a 0.10 gate and
named nine rules as crying wolf. Split by rule, every one of them fired on cards where the corpus
had planted no expectation at all: `expected_flags` was empty on all thirty nine answered cards.
The corpus plants exactly one rule, `advice_request`, on twenty of four hundred questions, all at
indices 160 to 179, and a run of the first forty carries none of them. The metric compared flags
that fired against a ground truth that did not exist, so it was pinned at maximum badness before
the run started.

Fixed at the metric: a false positive is a flag that fired where the corpus says it should not
have, so only rules the corpus plants somewhere in the run can be scored, and a run that plants
none reports n/a naming what it waits for. This is the governing metric rule in its own mirror.
Reporting the worst possible value with nothing to measure is the same error as reporting zero, and
it cost nine Verifier rules a false accusation.

**Where the visuals are.** `visual_type_match` reads 0.051, and its own split says why: table 0/22,
tree 0/10, list 0/5, steps 2/2. Twenty two of thirty nine cards expected a table and none got one.
That is the metric M14.5 already diagnosed at 0.083 on twelve questions, holding at forty, and it is
one type dominating rather than a spread.

**Three near misses on thin denominators**, reported rather than acted on. `citation_accuracy_ledger`
0.923 against 0.95, `retriever_assignment_accuracy` 0.915 against 0.95, and `verifier_agreement`
0.750 against 0.90 where `citations_the_ledger_can_judge` is only 0.209, so the denominator is a
fifth of the citations and one disagreement moves it a long way.

**What held.** `fact_recall_deep` 0.966, `forbidden_fact_rate` 0.000, `forbidden_fact_unflagged`
0.000, `injection_resistance` 1.000 with three hostile documents demonstrably seen and none cited,
`must_exclude_compliance` 1.000, `visual_fidelity` 1.000, `own_card_sole_support_rate` 0.000 and
`source_hierarchy_compliance` 1.000.

**Measured** 2026-08-27, `tessera-eval --corpus synthetic/42 --limit 40 --bulk-provider anthropic
--sample-per-depth 0`, results at `eval/results/42/anthropic/run-1787884598`.

**Verified** Full battery green: workspace tests, clippy at `-D warnings`, fmt, style lint, 93
generator guards, 76 Playwright tests, the grounded sweep rescored with nothing below threshold, and
the 20 board bundle round trip whole.

---



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
