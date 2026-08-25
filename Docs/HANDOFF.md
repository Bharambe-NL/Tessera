# HANDOFF: Canvas (working name; proposed name Tessera)

Written 2026-08-25 for a Claude Code session that will start the build from the spec set. Everything below is what the specs do not say: where the work stands, what was added late, what was decided in conversation, and what to do first.

## 1. What exists

- `spec/` : twelve markdown documents, v0.1 with v0.2 amendments to 01 and 05. Read `spec/00-README.md` first, then 01, 10, and 12 (build prompt). The build prompt in 11/12 is the operating instruction; this handoff amends it.
- `canvas-prototype.html` : a single-file browser prototype, about 1,300 lines, vanilla JS. It is the reference for interaction and look, not for code structure. It calls the Anthropic API directly from the browser when opened inside claude.ai and falls back to sample data elsewhere. Do not port it as is; rebuild the UI on the RPC protocol in 10 and reuse its interaction code (pan, zoom, layout, highlight to branch, block investigate, ink, notes, paste, tool strip, tutor panel) where it helps.

## 2. What the prototype implements today (so you know what "parity" means)

Infinite canvas with pan, zoom, dot grid; cards with tree, table, list, steps, figure visuals; per card follow-up; highlight to branch (dashed edge); clickable visual blocks with an "Investigate further" popover (branch here or new board with context); auto layout with user offsets and Tidy; multiple boards with persistence (artifact storage or localStorage); Home, Library, Profile (context, standing instructions, default depth, memory toggle, model keys list), Trash with restore; editable board title with breadcrumb to parent board; pen with five colours, eraser, notes, image paste, "Read sketch" and "Read this image" through the vision model; Fast, Deep (web search plus citations plus sources list), Research (plan, per sub-question search, synthesis) with staged progress; Learn mode with a Tutor panel (intake, curated board, checks, remedial or deeper cards, free questions); cross-board memory with a "builds on" chip.

Known prototype gaps versus spec: no Verifier, no Flags queue, no Concept graph, no bundles, no doctrine packs (profile context stands in), no event log, memory retriever is keyword overlap, keys are stored in browser storage (spec: keychain).

## 3. Added after the spec set was drafted (read these before phase 1)

1. **Learn mode and the Tutor agent** (`spec/14`). Tenth agent. Adds `Board.mode`, a LearnSession entity, seven events, a build phase 9b. Curated cards go through the normal pipeline and Verifier.
2. **Memory** (`spec/15`, amendments in 01 v0.2 and 05 v0.2). A fifth retriever, `boards`, over the profile's own verified cards; source class `own_card`; `Card.builds_on`; Verifier rule `own_card_sole_support` (block). The rule to hold: a prior card is context, never evidence.
3. **Name.** Proposed Tessera (product), tesserae (boards informally; the UI still says "board" and "card"). Not yet trademark checked. Use identifier `canvas` in code until confirmed; keep the product name in one config constant.

## 4. Decisions made in conversation that the specs assume

- First user: the author, daily, finance pack; then a Risk plus Product pair as the exchange test. Product scope is broader than finance; finance is a doctrine pack.
- Autonomy: full auto with audit trail; Verifier flags; user reviews flags only. The 0.90 Verifier agreement gate (02 section 10.3) is what enables this per pack; below it the harness falls back to draft mode automatically.
- Everything local on the desktop, user's own keys, private single-author boards, file-based sharing via bundles. Web client later, reduced, same RPC boundary.
- Router plus user override for model choice; override is enforced as a schema rule.
- Data model open questions 1 to 4 resolved as proposed in 01 section 11 (derived citation markers; verbatim passages with a sensitive-folder doctrine rule; concepts owned by profile, scoped by pack; separate `version_ref` on Source).
- Sources and Concepts are first class across boards. Verifier rejects uncited claims in deep and research.
- Trash moves into Home as a filter (09 open question 1); Flags takes the rail slot.

## 5. Open questions still needing the owner

Router 3 (may a pack forbid fast on regulatory questions; proposal yes), Synthesizer 2 (fast on finance at all), Retriever 1 and Verifier 3 (local model for sensitive folders), Architecture 1 (Tauri webview performance check, phase 0), Visual foundation 1 (name), Learn 2 (skip background intake when role is set; proposal yes). Decide by recording in `BUILD_NOTES.md` and continuing unless the answer changes a schema.

## 6. Build sequence amendments

Follow 12 phases 0 to 11 with these insertions: after phase 5, add the boards retriever (05 section 8.5) and its Verifier rule in phase 7; after phase 9, add phase 9b Tutor (14 section 5); the eval additions in 15 section 5 join phase 3. The definition of done in 12 gains: a Learn session runs end to end on a synthetic topic, and a card on a second board shows "builds on" a verified card from the first with the original source cited.

## 7. Style rules for every user-facing string and every document

House style: no dashes of any kind, sentence case, verbs name actions, no apologies. The owner's preference: no em dashes anywhere and no "it is not X, it is Y" constructions. Run these as a lint on UI strings.

## 8. Suggested skills for the next session

- `agentic-systems-design` (Mode B is complete; use pattern references when implementing the harness).
- `systems-builder` for the Rust core decomposition.
- `house-style` and `house-design` before any prose or styled surface leaves the build.
- `qa-lite` per phase; `qa-expert` before phase 11.
- `llm-eval-design` when wiring the eval harness in phase 3.
- `mermaid-tools` if architecture diagrams are wanted in `BUILD_NOTES.md`.

## 9. Redactions

No keys, credentials, or personal data are in the specs or the prototype. The prototype's Profile page stores keys the user enters in browser storage; that behaviour must not carry into the desktop build.
