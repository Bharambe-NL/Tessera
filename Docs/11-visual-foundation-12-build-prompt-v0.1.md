# 11. Visual Foundation v0.1

Register: working. Depends on: 09, 10. This document turns the prototype's look into decisions the build can follow.

## 1. Product name

The name is **Tessera**, confirmed by the owner on 2026-08-30. The code identifier is `tessera` everywhere; the working name Canvas and its candidate list are retired.

## 2. Visual register

Product register (house-design). The design serves reading. One accent under 10 percent of the surface; muted node hues reserved for generated visuals so they read as content, not chrome.

Reference inventory: the prototype (cream replaced by tinted off-white), Tana and Kinopio for the notebook feel, tldraw for canvas interaction conventions, Linear for the flag queue density.

## 3. Tokens

Colour, OKLCH, light theme first; dark theme in v1 with the same roles.

```
--bg           oklch(0.985 0.004 80)   canvas background
--grid         oklch(0.88 0.006 80)    dot grid
--surface      oklch(1 0 0)            cards, panels
--line         oklch(0.86 0.006 80)    borders
--line-strong  oklch(0.72 0.008 80)    edges, active borders
--ink          oklch(0.22 0.01 80)     text
--ink-2        oklch(0.42 0.012 80)    secondary text
--ink-3        oklch(0.58 0.012 80)    tertiary text, meets 4.5:1 on --surface for 12px+ only; use --ink-2 below 12px
--accent       oklch(0.45 0.12 250)    one working accent: focus, links, selected
--accent-soft  oklch(0.94 0.03 250)
--user         oklch(0.24 0.01 80)     user message pill
--olive        oklch(0.86 0.06 120) / --olive-ink oklch(0.36 0.07 125)   confidence good, node hue 1
--slate        oklch(0.86 0.04 250) / --slate-ink oklch(0.36 0.06 255)   node hue 2, deep badge
--rust         oklch(0.82 0.08 40)  / --rust-ink  oklch(0.40 0.11 38)    node hue 3, block severity
--amber        oklch(0.86 0.10 85)  / --amber-ink oklch(0.38 0.09 75)    node hue 4, highlights, warn severity, bottom line bars
```

Dark theme: invert lightness on bg, surface, line, ink; keep accent and node hues; node ink values move to 0.80 lightness.

Typography: IBM Plex Sans 400, 500, 600 for UI and prose; IBM Plex Mono 400, 500 for badges, ordinals, code, event lines. Base 13.5px in cards, 13px in chrome, 12px in badges. Line height 1.55 for answers. Display ceiling: 20px; this product has no hero. `text-wrap: pretty` on answers, `balance` on titles.

Spacing scale: 4, 6, 8, 10, 12, 14, 18, 24, 40. Card width 440, gap 60 horizontal, 90 vertical, branch offset 120 (from the prototype's layout, which tested well).

Radii: 6 (nodes, inputs), 8 (buttons, popovers), 10 (visuals), 12 (cards, tool strip), 14 (composer), 999 (pills only).

Elevation: one shadow only, `0 1px 2px oklch(0 0 0 / .04)` on cards; popovers `0 3px 8px oklch(0 0 0 / .1)`. No shadow plus border combination beyond these.

## 4. Component foundation

Hand built components in the prototype's style; no component library, because the canvas surface dominates and libraries fight it. Shared primitives (from `ai-native-ui-primitives.md`): status chip, evidence panel, streaming stage list, queue row, disclosure, toast. Each is one file with tokens only.

## 5. Application shell

Left rail 56px collapsed, 240px open. Main area is the canvas or a page (Home, Flags, Library, Profile). Board title centred at top, editable. Tool strip below it, draggable. Composer bottom centre. Toolbar top right (zoom, tidy, fit, clear). Mode indicator top left (live, offline, verifier below threshold).

## 6. Screen inventory

Board; Home; Flags; Library (Sources, Concepts); Profile (Context, Models, Retrievers, Doctrine, Diagnostics); Trash (as a Home filter); Exercise (modal over the board); Bundle export checklist (modal); Import confirmation (modal); First run (choose pack, add a model key, optionally a folder).

## 7. Motion

Card rise 360 ms quart out on creation; layout moves 380 ms quart out; view animation 420 ms quart out; stage list ticks with a 200 ms crossfade. Reduced motion: instant states, no movement. Nothing else moves.

## 8. Iconography

Line icons, 1.8 stroke, 24 grid, drawn in the prototype's style. No icon font. No illustration anywhere.

## 9. Voice and copy

Working register from house-style. Sentence case. Verbs name what happens: "Rerun as Deep", "Accept flag", "Read this image". Empty states instruct: "Ask something, or paste an image". Errors say what and how: "No search key. Add one in Profile to enable web search." No apologies, no exclamation marks.

## 10. Accessibility

Contrast 4.5:1 for text, 3:1 for large; every action keyboard reachable; focus ring in accent; reduced motion; canvas has a list view alternative (the board's cards as a document) for screen readers, reachable from the title menu.

---

# 12. Build Prompt v0.1

For Claude Code, starting from the spec set in `spec/` and the prototype in `canvas-prototype.html`.

## Mission

Build Canvas v1: a local desktop application (macOS and Windows, Tauri 2 with a Rust core and a webview UI) where a user turns questions into linked cards with verified visuals and cited sources, reads sketches and images into cards, checks understanding with generated exercises, and exchanges boards as portable bundles. Finance is the first doctrine pack. Every agent output is validated against a schema, every action is an event, and the Verifier decides what the user must review.

## The spec set

01 Data model; 02 Synthetic data generator and eval; 03 Router; 04 Planner; 05 Retrievers; 06 Synthesizer and Visualizer; 07 Reader and Verifier; 08 Exercise; 09 Review queue and board UX; 10 Architecture; 11 Visual foundation. The prototype is the reference for canvas interaction (pan, zoom, layout, highlight to branch, block investigate, ink, notes, paste, tool strip, pages) and for the visual look. Where the prototype and the specs disagree, the specs win; note the disagreement in `BUILD_NOTES.md`.

## Operating principles

1. Schema first. Write the JSON schemas for packets, outputs, events, doctrine packs, and bundles before any agent code. Validate at every boundary. Version from the first commit.
2. Events are the state. No UI or run state that cannot be rebuilt from the Event table.
3. Patterns are infrastructure. State machines, hooks, failure taxonomies, and the ledger are built once in the harness and reused by every agent.
4. Doctrine is data. Packs are JSON files with a schema; no domain rule in code.
5. Fail closed at the Verifier. The mock provider returning garbage must produce a flagged card.
6. Synthetic first. The eval harness runs from phase 3; every later phase reports its numbers.
7. Secrets in the keychain only.
8. House style in every user facing string (no dashes, sentence case, verbs name actions).

## Build sequence

**Phase 0. Shell and canvas performance check.** Tauri 2 project; webview loads the prototype's UI; measure pan and zoom with 200 cards on Windows and macOS. Acceptance: 60 fps pan at 200 cards on a mid range laptop; if not, record the finding and switch the layer to canvas rendering for edges and ink before continuing.

**Phase 1. Core storage and schemas.** SQLite schema from 01 with migrations; blob store; Event table with provenance envelope; projections for Run, Step, Card status; JSON schemas for every packet, output, event, pack, and bundle; schema guard on entry. Acceptance: schema tests pass; a scripted sequence of events rebuilds Card state identically after a restart.

**Phase 2. Harness.** Run scheduler and ledger with claim and heartbeat; agent state machine base; retry and failure taxonomy base; hooks (pre and post tool); provider trait with the Anthropic adapter and the deterministic mock provider; keychain integration; JSON-RPC boundary with the UI bridge translating events into UI notifications. Acceptance: a mock run walks every state and emits the expected events; a crash mid run is reclaimed on restart.

**Phase 3. Synthetic generator and eval harness.** The generator from 02 with seed reproducibility; the local static server for the synthetic web; the harness that runs the pipeline with test provenance and computes the metrics. Acceptance: `gen build --seed 42` twice yields identical ledgers; the harness runs end to end on the mock provider and reports every metric as 0 or n/a.

**Phase 4. Router and Planner.** Per 03 and 04. Acceptance: route accuracy and domain accuracy targets on the synthetic set with a real provider; override compliance 1.00.

**Phase 5. Retrievers.** Local (watcher, parsers, chunking, local embeddings, hybrid search), regulatory (subscription manifests, version tracking), web (search adapter for one provider, fetch, extraction), structured (templates over CSV and xlsx). Hooks enforced. Acceptance: recall targets from 05; Sensitive folder exclusion 1.00; dedupe 0 duplicates.

**Phase 6. Synthesizer and Visualizer.** Per 06. Acceptance: fact recall, citation accuracy, forbidden fact rate, advice containment, injection resistance targets; visual fidelity 1.00.

**Phase 7. Verifier.** Per 07 part B, deterministic checks first, then the support check, then doctrine model checks. Acceptance: agreement with the ledger 0.90; fail closed tests pass; flag false positive rate per rule under 0.10 or the rule is disabled and listed.

**Phase 8. UI binding.** Migrate the prototype UI to the RPC protocol: cards render from events; streaming stages; citations and sources; flags inline; the Flags queue; Library; Profile pages (context, models, retrievers, doctrine, diagnostics); Trash as a Home filter; audience lens; "How this was built"; board history. Acceptance: every verb in 09 section 5 works and emits its event; keyboard reachability; contrast checks.

**Phase 9. Reader and Exercise.** Per 07 part A and 08, including the sketch raster path and the OCR service for scanned pages. Acceptance: structure recovery F1 and injection targets; exercise traceability 1.00.

**Phase 10. Bundles and doctrine packs.** Export with the local document checklist; import with merge rules; the three shipped packs; pack import; verify only on pack update. Acceptance: round trip of the 20 synthetic boards; concept collision handled as specified.

**Phase 11. Packaging and release.** Signed and notarised builds; backup and restore; first run flow; diagnostics export; nightly eval in CI with real providers. Acceptance: fresh install to first verified deep card in under five minutes with one model key and one search key.

## What you do not build

Live multi user collaboration or sync; the hosted backend; the web client; chart and widget visual types (schemas only); generated images in the finance pack; free form SQL; OS notifications; telemetry; automatic doctrine rule changes; a marketplace for packs.

## How to ask

Stop and ask when a spec is silent on something that changes a schema or an event, when two specs conflict, or when a target cannot be met and the cause looks like the spec rather than the code. Otherwise decide, record it in `BUILD_NOTES.md` with the section reference, and continue.

## Definition of done

All phases accepted; the eval report for the synthetic corpus at seed 42 meets every threshold in 02 section 10.3 with the default model policy; a real user creates a board, asks three questions at deep, branches from a highlight and from a block, pastes an image and reads it, clears two flags, runs an exercise, exports a bundle, and imports it on a second machine; every one of those actions appears in board history with the right actor; no secret in any file except the keychain.

---

# 13. Cohesion notes

Checked across the set after drafting:

- Entity names and field names in 03 to 08 match 01. `Card.depth`, `Card.audience_id`, `Visual.block_index`, `Citation.verifier_verdict`, `Flag.severity` are used identically.
- Event names in agent specs are all in 01 section 6.3's vocabulary or added there by this pass: `context.stale_noted.v1`, `entity.resolved.v1`, `hook.denied.v1`, `citation.verdict.v1`, `card.blocked.v1`, `visual.declined.v1`, `source.proposed.v1`, `exercise.item_reported.v1`, `run.compacted.v1`. Add these to 01 in v0.2.
- The Verifier's automation gate (0.90 agreement) appears in 02, 07, 10, and 12 with the same number.
- 09 proposes moving Trash into Home; 11 and 12 adopt it. 09's rail list should be updated in v0.2.
- Open questions carried forward: Router 3 (pack minimum depth), Synthesizer 2 (fast on finance), Retriever 1 and Verifier 3 (local models for sensitive folders), Architecture 1 (Tauri check), Visual foundation 1 (name).
