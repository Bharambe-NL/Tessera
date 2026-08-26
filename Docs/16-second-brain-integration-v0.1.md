# 16. Second Brain Integration v0.1

Register: working. Assesses the "Second Brain Canvas" handoff package (Architecture v2, Replication Spec v1, static React app) and plans how its ideas enter Tessera without disturbing build phases 6 and 7.

## 1. What the package is

A three-view app over one vault: markdown Notes with wikilinks and backlinks, a Canvas of streaming thread cards with typed visuals, and a Notebook that answers questions over the vault with snapshot citations and an explicit "ungrounded" label when nothing matches. Static build on localStorage with a keyword answer engine; a tRPC plus Drizzle plus MySQL backend exists but never deployed. About 1,900 lines of React and TypeScript, forty shadcn components, two short design documents.

## 2. Assessment

### 2.1 Worth adopting

| Idea | Why it matters for Tessera |
|---|---|
| Three views over one vault (memory, thinking, recall) | A clean product framing. Tessera today has thinking (boards) and recall (memory retriever, Library) but no durable memory layer the user writes by hand. Pages fill that gap. |
| The "ungrounded" trust contract | An answer that found nothing in your sources says so, visibly. Tessera already has this in the Synthesizer's `no_passages` path and the fast mode "Unverified" state; the notebook view should make it a first class visual state. |
| Save as note closes the loop | A verified card becoming a page the user owns and edits is the missing step between exploration and a lasting vault. |
| Backlinks as a query | Inbound references shown on every page. Tessera's ConceptLink table already stores the edges; the panel is UI. |
| Citation snapshots survive deletion | A cited note being deleted must not corrupt a saved answer. Tessera's Passage rows already carry verbatim text; the rule extends to pages. |
| Edge handles (`+` on card edges to start a new thread) | Cheap, discoverable, missing from the prototype. |
| Stats tiles visual | Large numerals for "1949", "120m". Tessera has no compact numeric visual; a `stats` type slots under the block index with a citation per tile. |
| Flow with explicit edges | Tessera's tree cannot express a cycle or cross link. A `flow` type (nodes plus edges) generalises it. |
| Context inheritance capped at three ancestors with short summaries | Tessera's Planner reads up to three ancestors already; the package confirms the number and the "summaries, not full text" rule. |

### 2.2 Where it is weaker than Tessera, and must not be imported

| Package choice | Problem | Tessera's answer |
|---|---|---|
| Keyword scoring plus templates as the answer engine | Fine as an honest simulation; the "v3 swap" is described, never built. | The full pipeline exists through phase 7. |
| Citations point at notes, not at the sources the notes came from | A note written from a card that cited a regulation becomes the citation for the next answer. Two hops later the regulation is out of reach and possibly stale. This is exactly the loop the memory rule in 15 forbids. | Pages carry their own citations to Passages; a page is context, its citations are the evidence. |
| No verification stage | Grounded means "matched a note", not "the claim is supported by the quoted text". | The Verifier's support check and `own_card_sole_support` rule extend to pages. |
| Canvas stored as one JSON blob | Argued as write efficiency; loses per card provenance, supersede chains, and the event trail. | Event sourced cards stay. Position churn is already isolated in `Card.position`. |
| Wikilinks resolve by title string | Renames silently break links into "unresolved". | Wikilinks resolve to Concepts (terms with aliases) or Pages by id, with the title as display. Renames survive. |
| No doctrine, no autonomy model, no audit | A general note app has none of the finance pack's needs. | Pages and the notebook view inherit the pack rules and the event log. |
| localStorage; a MySQL backend that requires hosting | Conflicts with local only and keys in the keychain. | SQLite in the profile folder; pages are markdown files on disk as well as rows, so the user's vault is theirs even without the app. |
| React 19 plus tRPC plus Drizzle stack | A second UI stack alongside the Tauri webview. | Adopt ideas and the contract shapes, not the code. |

### 2.3 Verdict

The package's framing is stronger than its engineering. Its three view model, the ungrounded contract, save as note, and backlinks should enter Tessera as a Vault layer. Its engine, storage, and citation model should not. Nothing in it changes phases 6 and 7; everything lands as additive entities, one retriever tweak, two visual types, and UI.

## 3. Integration design

### 3.1 New entity: Page

Tessera's existing `Note` is a sticky on a board. The vault note becomes **Page** to avoid a rename mid build.

| Field | Type | Notes |
|---|---|---|
| id | ulid | |
| profile_id | ulid | |
| title | string | Unique per profile, case insensitive; renames keep id. |
| body | text | Markdown. Wikilinks as `[[Title]]` or `[[Title|alias]]`; on save, resolved to `PageLink` rows. |
| file_path | string | Mirrored as `vault/<slug>.md` in the profile folder; the file is the export; the row is the index. Two way sync on file change through the local watcher. |
| source_card_id | ulid | Set when created by Save as page. |
| citations_carried | json | The card's citations copied as `{ordinal, passage_id}`; the page's own evidence, never re-derived from the page text. |
| doctrine_pack_id, created_at, updated_at, supersedes | | |

**PageLink**: `from_page_id, target_kind: page | concept | unresolved, target_id, display_text, position`. Backlinks are `select from PageLink where target_id = ?`. Unresolved links create the page on click, as in the package.

Concept integration: a wikilink whose title matches a Concept term or alias links to the concept, and the concept detail lists pages beside cards. A page titled like a confirmed concept becomes that concept's `definition_page_id` when the user accepts.

### 3.2 Save as page

Card menu verb (added to the eight verb vocabulary as a variant of Edit, or a ninth verb "Save"). Creates a Page from question, answer, findings, and a markdown rendering of the visual; `source_card_id` and `citations_carried` set; the card header shows a page chip; `page.created_from_card.v1` emitted. Blocked content is excluded. Only done or warn flagged cards can be saved.

### 3.3 Retrieval

The local retriever indexes `vault/` like any folder, so pages are retrievable with no new retriever. Their source class is `page` (new), trust rank in the finance pack 4 (below external sources, above own_card). The Verifier extends `own_card_sole_support` to `page`: a numeric or regulatory claim may not rest on a page alone; the page's `citations_carried` supply the original passage, and the Synthesizer is told to cite those. The memory rule holds: pages are context, their citations are evidence.

### 3.4 Notebook view

A chat layout over the vault rather than a new engine: each question runs the normal pipeline at deep depth with retrievers restricted to `local:vault`, `boards`, and optionally `local:*`, web off by default. The answer renders as a card body without the canvas. States: **Grounded** (citations supported), **Partly grounded** (some claims unsupported), **Ungrounded** (`no_passages`; the answer is the model's, marked as such, with a one click "search the web instead"). Each answer offers Save as page and "Open on a board" (creates a root card from the session). Sessions are boards of `mode: notebook` so history, events, memory, and export come free.

### 3.5 Visual types

Add to 01 section 4.3.1 and the Visualizer:

- `flow`: `{nodes: [{id, label, note, citation_ordinals[]}], edges: [{from, to, label}]}`, rendered with a small layered layout; tree remains for strict hierarchies.
- `stats`: `{tiles: [{value, unit, label, citation_ordinals[]}]}`, at most 6 tiles, every tile cited (a tile without a citation is a `numeric_without_citation` block flag).

### 3.6 Canvas affordances

Edge handles: four `+` handles on hover; drag out creates an empty follow-up card with the composer focused (the prototype's card footer input does the same without the drag; the handle adds discoverability). "Add note" from the highlight menu creates a sticky attached by a dashed edge to the card, with the quote prefilled.

### 3.7 Shell

The left rail gains **Pages** (explorer, editor with write and preview, backlinks panel) and **Notebook**. Home shows pages count and last edited. Library's concept detail lists pages.

## 4. Data model deltas (01 v0.3)

New: Page, PageLink. Changed: Source.class gains `page`; Board.mode gains `notebook`; Card gains `page_id` (set on save). Events: `page.created.v1`, `page.created_from_card.v1`, `page.edited.v1`, `page.renamed.v1`, `page.deleted.v1`, `page.link_resolved.v1`, `page.link_unresolved.v1`, `notebook.asked.v1`, `notebook.grounding.v1 { state }`. Bundles gain `pages.jsonl` and `page_links.jsonl`; pages are included only when the user ticks them on export, the same checklist as local documents.

## 5. Build plan (additive, after phase 7 acceptance)

| Phase | Work | Acceptance |
|---|---|---|
| 12a Vault storage | Page and PageLink tables and migrations; `vault/` mirror with two way sync through the watcher; wikilink parser resolving to pages and concepts; `page` source class indexed by the local retriever. | Edit a file on disk, see the page update; rename a page, links survive; a page appears as a passage in a deep answer with class `page`. |
| 12b Save as page | Card verb, chip, events, citations carried. | Saved page retains the card's citations; Verifier blocks a numeric claim resting on the page alone and admits it when the carried passage is cited. |
| 12c Pages view | Explorer, editor, preview, backlinks, unresolved link creation. | Backlinks are a query over PageLink; no full scan. |
| 12d Notebook view | Chat layout over restricted retrievers; three grounding states; save and open on board; sessions as boards. | The ungrounded state appears whenever `no_passages`; never a silent fallback. |
| 12e Visuals | `flow` and `stats` in the Visualizer, block index, and Verifier. | Every tile and node cited or flagged. |
| 12f Canvas affordances | Edge handles; highlight "Add note". | Both emit events; both undoable. |
| Eval | Synthetic vault: 40 pages with planted facts and carried citations; questions whose answers are only in pages; questions with no vault match. | Grounding state accuracy 0.95; page sole support after verification 0; backlink completeness 1.00. |

Estimated size: about the same as phases 5 and 8 together. Nothing here blocks the current phase 6 and 7 work; 12a can start on a branch once 01 v0.3 is agreed.

## 6. What to take from the package's code

Read, do not port: `contracts/canvas.ts` (the `VisualPayload` union confirms the `flow` and `stats` shapes), `localVault.ts` lines 156 to 215 (the grounded, ungrounded branching is the UX contract to reproduce), `NotesView.tsx` (explorer plus editor plus backlinks layout), `NotebookView.tsx` (citation chips that open the source at the quote). The React and shadcn code stays out of the Tauri webview.

## 7. Risks and open questions

1. **Two note concepts.** Sticky `Note` and vault `Page` will confuse users if both are called notes in the UI. UI copy: "sticky" for the board object, "page" for the vault object. A later rename of the entity is a migration, not a redesign.
2. **Vault mirror conflicts.** A page edited in the app and in an external editor at once. Last write wins with a conflict copy (`<slug> (conflict).md`), as file sync tools do; event recorded.
3. **Page trust rank.** 4 in the finance pack is a proposal. A team may want pages authored by a named reviewer to rank higher; that needs page authorship, which the single user v1 does not have. Defer.
4. **Notebook web fallback.** "Search the web instead" reruns at deep with web on. Whether the ungrounded answer is kept as a superseded version or discarded: proposal, superseded, so the trail shows what was said before sources were found.
5. **Name.** The package uses "second brain"; Tessera's language is "vault" for the folder and "pages" for its notes. Keep Tessera's.
