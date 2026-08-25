# 05. Retriever Agents v0.2 (Web, Local, Regulatory, Structured, Boards)

Changelog v0.2: added the boards retriever (section 8.5) for cross-board memory. Design rationale in 15.

Register: working. Depends on: 01, 02, 04. Load bearing patterns: 2, 3, 4, 5 (freshness), 7, 11 (pre and post tool hooks), 13.

## 1. Purpose, scope, non-goals

Retrievers fetch passages. They are the only agents that touch external data, and the only ones that create Source and Passage rows. They contain no model in the common path; where a model is used (query rewriting, chunk ranking), it never writes to the user.

Four retrievers share one substrate contract and differ in doctrine and connector:

| Retriever | Connector | Creates Sources of class |
|---|---|---|
| web | Search API with the user's key, then page fetch | web |
| local | Watched folders, local index | local_document |
| regulatory | Subscribed corpora (downloaded consolidated texts with version tracking) | regulatory |
| structured | Read only queries against a user configured table or spreadsheet | structured_query |
| boards | The profile's own verified cards across all boards, local index | own_card |

Out of scope: judging truth; ranking across retrievers (the Synthesizer and the doctrine hierarchy do that); writing to any external system.

## 2. Architectural position

Fan out from the Planner's assignments, run in parallel, results collected by the harness. Pre tool hooks (Pattern 11) enforce exclusions and rate limits before any fetch; post tool hooks record provenance and content hashes after.

Substrate: the retriever interface, Source and Passage creation, dedupe, hashing, freshness checks, hooks. Doctrine: corpus subscriptions, folder inclusions, trust ranks, freshness classes, the structured query templates.

## 3. Trigger model

- On demand: one invocation per Planner assignment.
- Scheduled: local runs an index refresh on file system events and at most once per minute; regulatory checks subscribed corpora daily for new consolidated versions; web runs re-verification of cited locators weekly (content hash comparison) and emits `source.stale.v1` on change.
- On demand re-verification: when a board is opened and any citation is older than its freshness class.

## 4. Task packet (shared)

```json
{
  "schema_version": "1.0", "run_id": "ulid", "card_id": "ulid", "sq_id": "string",
  "retriever_id": "web | local | regulatory | structured",
  "query": "string",
  "filters": { "corpus": "string | null", "folder": "string | null", "date_from": "ISO8601 | null", "version_ref": "string | null", "language": "string | null" },
  "max_passages": 12,
  "must_exclude": ["string"],
  "doctrine": { "trust_ranks": [ { "issuer_pattern": "string", "rank": 1 } ], "freshness_classes": {} },
  "effort_budget": { "max_fetches": 8, "max_latency_ms": 12000 }
}
```

## 5. Output schema (shared)

```json
{
  "schema_version": "1.0", "agent_id": "retriever.web", "run_id": "ulid", "sq_id": "string",
  "passages": [
    { "passage_id": "ulid", "source_id": "ulid", "text": "string", "location": {}, "score": 0.0,
      "source": { "class": "string", "title": "string", "locator": "string", "issuer": "string", "published_at": "ISO8601 | null", "trust_rank": 3, "freshness_class": "string", "version_ref": "string | null", "content_hash": "string" } }
  ],
  "sources_created": 0, "sources_deduplicated": 0,
  "coverage": "full | partial | none",
  "exclusions_applied": ["string"],
  "confidence": 0.0, "caveats": ["string"]
}
```

Harness rules: passage text length capped at 1,200 characters (longer spans are split); no passage from an excluded path or domain (hook enforced, violation is a hard failure, never a caveat); `trust_rank` set from doctrine, never by the retriever's own judgment.

## 6. State machine

```
received ──► pre_hooks ──► querying ──► fetching ──► extracting ──► chunking ──► ranking
   ──► persisting ──► post_hooks ──► emitting ──► done
retry (once) on transient fetch error; failed on hook_denied or connector_unavailable
```

## 7. Events

```
retrieval.started.v1   { retriever_id, sq_id, query }
retrieval.completed.v1 { retriever_id, sq_id, passage_ids, source_ids, coverage, fetches, latency_ms }
source.created.v1 / source.deduplicated.v1 / source.stale.v1 { source_id, reason: content_changed | locator_gone | superseded_version }
index.updated.v1 { folder_id, files_indexed, files_skipped, errors }
hook.denied.v1   { retriever_id, hook_id, target }
```

## 8. Per retriever pipeline

### 8.1 Web

Pre hooks: domain denylist from doctrine (hate, extremist, known content farms) and the user's own; no personal data in the query string (deterministic check). Query: the Planner's query as written, optionally rewritten once by the small alias when the first search returns fewer than three results. Fetch: top eight results, main content extraction, boilerplate removal. Chunk: by heading then by 800 character windows with 100 overlap. Rank: BM25 over chunks against the query, then a small alias rerank of the top 20 when `max_passages` is under 8. Persist: Source per page (dedupe by normalised URL), Passage per admitted chunk. Post hooks: content hash, `published_at` from page metadata when present, trust rank from issuer pattern match.

### 8.2 Local

Index maintenance: a watcher per included folder; new or changed files are parsed (pdf with text layer via the product's parser, scanned pdf via the Reader's OCR path, docx, xlsx, md, html, txt), chunked, embedded with the embedding alias, and stored as IndexEntry. Excluded folders are never opened. Files that fail to parse are listed once per folder in the Profile with the error. Query: hybrid, BM25 plus vector, fused by reciprocal rank. Filters: folder, language, date. Persist: Source per file (locator is the file name plus a stable file id, never the absolute path beyond the profile), Passage per admitted chunk with page and offsets. A spreadsheet chunk carries the row range.

### 8.3 Regulatory

Corpora are subscriptions: a corpus id, a fetch URL for the consolidated text, and a version discovery rule. Daily check downloads the current version if the published hash changed and records `version_ref`. Query: article aware; the query is matched against article headings and recitals first, then full text. Filters: `version_ref` from the Planner (defaults to the version in force at the run date). Passage location is article and paragraph. Trust rank 1 by doctrine. Freshness: when a cited version is superseded, `source.stale.v1` with reason `superseded_version` and the new version ref, so the Verifier can flag cards that cite the old value.

### 8.4 Structured

Read only. The user registers a table (CSV, xlsx sheet, or a SQLite table) and the doctrine pack supplies query templates with named parameters ("exposures by book and date"). The Planner picks a template and parameters; the retriever runs it and returns the result rows as a Passage whose text is the rendered table and whose location is the query and parameters. This is the only way a computed number enters a card: the Verifier accepts a numeric claim when its citation points at a structured passage that contains the value. Free form SQL from a model is never executed.

### 8.5 Boards

Indexes the profile's own cards: question, answer, findings, visual labels, embedded with the local alias, updated on `card.answered.v1`. Eligibility: status done, depth deep or research, no open block flags, not on a trashed board. Query: hybrid like local, excluding the current board. Returns at most three passages, each a rendered digest of the card with its own citations listed, as a Source of class `own_card` with `locator` = board id plus card id and `trust_rank` from doctrine (finance: 5, below every external class). The Planner adds this retriever to every sub-question when `Profile.memory_enabled`. The Synthesizer receives these passages marked "prior work, context only". The Verifier rejects any numeric or regulatory claim whose only support is an `own_card` passage (rule `own_card_sole_support`, severity block), and the Card records `builds_on` for every own_card passage that was cited or used. Stale propagation: when a source cited by the prior card goes stale, `verify_only` also flags cards that build on it.

## 9. Confidence

Deterministic: coverage `full` (+0.4), at least one trust rank 1 or 2 source (+0.3), no fetch errors (+0.2), query not rewritten (+0.1). Always admitted; the Synthesizer weighs passages by trust rank and score, not by retriever confidence.

## 10. Failure taxonomy

| Type | Recovery |
|---|---|
| `hook_denied` | Hard stop for that assignment; event; the Synthesizer proceeds with other retrievers; card caveat names the exclusion category without naming the excluded item. |
| `connector_unavailable` (no search key, folder unmounted, corpus fetch fails) | Coverage `none`; Profile notification; other retrievers continue. |
| `rate_limited` | Backoff once within budget; then partial. |
| `parse_error` on a file | Skip file, record in index errors. |
| `empty_result` | Coverage `none`; one query rewrite; then report. |
| `budget_exhausted` | Return what was fetched; coverage partial. |
| `unknown` | Evidence bundle; assignment fails; card continues if any other assignment succeeded. |

Posture: tolerant per assignment, strict on hooks. A retriever may return nothing; it may never return something it was told not to touch.

## 11. Review surface

The Profile's Retrievers page shows folders, corpora, tables, index status, and parse errors. Hook denials appear as a counter with a per denial log. No per run review.

## 12. Eval

Recall at k on planted facts by retriever: local 0.90, regulatory 0.95, web 0.80 (the synthetic web is served locally, so this measures extraction and ranking). Exclusion compliance 1.00 on the Sensitive folder. Dedupe: zero duplicate Sources for mirrored pages. Staleness detection at T3: 0.95. Scanned pdf recall 0.70. Index refresh latency under 60 s from file change. Structured: 1.00 correctness on template queries against the synthetic spreadsheet.

## 13. Performance

Web 3 to 10 s per assignment; local under 1 s query, index cost amortised; regulatory under 1 s; structured under 1 s. Parallel across assignments.

## 14. Open questions

1. Embedding alias: local model (fast, private, lower quality) or provider embedding (better, sends chunk text to the provider). Proposal: local by default, provider optional per folder, decided in the architecture spec.
2. Web fetch of pages behind consent walls or login: skip and caveat, or use the desktop app's browser session. Proposal: skip in v1.

## 15. Appendix: hook definitions

```
pre:  exclude_paths      { patterns from doctrine.must_exclude and profile.retriever_config }
pre:  deny_domains       { doctrine denylist + profile denylist }
pre:  no_pii_in_query    { deterministic patterns for account and identity numbers }
pre:  rate_limit         { per provider }
post: hash_and_stamp     { content_hash, retrieved_at, trust_rank, freshness_class }
post: audit_fetch        { locator, status, bytes, latency, into events }
```
