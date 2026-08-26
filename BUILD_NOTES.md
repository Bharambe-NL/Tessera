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
file named `strings.ts`. That file does not exist yet; M9 writes the copy that goes in it.

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

---

## Measured findings

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
