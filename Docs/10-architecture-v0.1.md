# 10. Architecture Spec v0.1

Register: working. Depends on: 01 to 09. Load bearing patterns: 21 (provider abstraction), 24 (event sourced run state), 25 (protocol as a view over the event log), 18 (deterministic mock testing), 11 (hooks), 27 and 32 (work ledger and schema guards, used inside one machine).

## 1. Architectural principles

1. **Local by default.** All state lives on the user's machine. The only outbound traffic is to providers the user configured (model, search, embedding) and to subscribed regulatory corpora. No telemetry in v1.
2. **One core, several shells.** The pipeline, storage, and event log are a library (the core) with a JSON-RPC boundary. The desktop shell is the first client. A future hosted backend and the reduced web client wrap the same core.
3. **Events are the state.** Run, Step, and the card's progress are projections of the Event table. Any UI state can be rebuilt from events; replay is a first class operation.
4. **Schema at every boundary.** Task packets, agent outputs, events, doctrine packs, and bundles are versioned schemas validated on entry (Pattern 32). A schema change goes through a migration registry.
5. **Fail closed at the Verifier, fail open elsewhere.** Retrievers and upstream agents degrade; the Verifier never admits on failure.
6. **Doctrine is data.** Packs are files, versioned, importable, editable in the Profile. No pack content in code.

## 2. Topology

```
┌──────────────────────────── Desktop shell (Tauri) ────────────────────────────┐
│  Webview UI (the prototype, rebuilt on the core protocol)                      │
│     │ JSON-RPC 2.0 over local IPC (Pattern 25)                                  │
│  ┌──▼─────────────────────────── Core (Rust) ──────────────────────────────┐    │
│  │ Harness: run scheduler, work ledger, state machines, hooks, schema guard│    │
│  │ Agents: router, planner, retrievers, synthesizer, visualizer, reader,   │    │
│  │         verifier, exercise (each a module with packet in, output out)   │    │
│  │ Providers: model (anthropic, openai, google, mistral, ollama), search,  │    │
│  │            embedding, image; keychain access                            │    │
│  │ Storage: SQLite (WAL) + blob dir + vector index; event log; projections │    │
│  │ Indexer: folder watcher, parsers, OCR, chunker                          │    │
│  │ Doctrine loader; bundle import/export                                   │    │
│  └─────────────────────────────────────────────────────────────────────────┘    │
└────────────────────────────────────────────────────────────────────────────────┘
        │ HTTPS to configured providers        │ HTTPS to corpus subscriptions
```

The core runs in process with the shell in v1 (a Tauri command layer over the same RPC types), so the web client can later talk to the identical protocol over a socket.

## 3. Component choices

| Component | Choice | Reason |
|---|---|---|
| Shell | Tauri 2 | Small binaries, Rust core in the same process, Windows and macOS signing supported, webview UI reuses the prototype. Electron is the fallback if webview parity problems appear on Windows. |
| Core language | Rust | Matches the reference harness patterns (claw-code is Rust); strong typing for schemas; one binary. |
| UI | The prototype's vanilla approach migrated to a small component layer (Svelte or plain TS modules; decided in the build prompt) | Keep the canvas code, which is already event driven. |
| Storage | SQLite with WAL, `sqlite-vec` for embeddings | One file per profile, portable, no daemon. |
| Blob store | Content addressed directory beside the database | Images, prompts, sanitised svgs. |
| Parsers | pdfium (text and render), docx and xlsx via Rust crates, html via a readability port | The synthetic generator renders with the same libraries. |
| OCR | Tesseract bundled, with the vision alias as an opt in upgrade | Local by default. |
| Embeddings | Local small model (e.g. a bge or nomic class model via candle) by default; provider embedding per folder opt in | Chunk text stays local unless chosen. |
| Search API | User supplied key for one of Brave, Tavily, SerpAPI, or Exa; adapter per provider | No default provider; the Profile requires one to enable web. |
| Keychain | macOS Keychain, Windows Credential Manager via the `keyring` crate | Secrets never in SQLite. |
| Packaging | Tauri bundler; notarised dmg; signed msi | Standard. |

## 4. Data architecture

- Database schema follows 01 exactly; one migration file per version.
- Event table is append only with a `sequence` index; projections (Run, Step, Card status) are updated in the same transaction as the event write so a crash cannot leave them apart.
- Compaction: Steps older than a profile setting are summarised into a `run.compacted.v1` event carrying counts and cost; the Event table itself is never compacted.
- Backups: the profile database and blob directory are one folder; the app offers "Back up now" to a chosen location and a scheduled daily copy. Restore is a folder copy.
- Retention: Trash purges at 30 days by emitting `board.purged.v1` and deleting rows; blobs are garbage collected when unreferenced.

## 5. Event bus

In process, typed, with the provenance envelope from 01 section 6.3. Subscribers: projections, the UI bridge (which translates events into a small set of UI notifications, Pattern 25's projection discipline), the audit exporter, the verify only scheduler. Test and replay provenance are filtered out of policy hooks by default.

## 6. Run scheduling and the work ledger

A single machine still needs a ledger (Pattern 27): a table of runs with `claimed_by` (worker id), `claimed_at`, `heartbeat_at`, so an app crash mid research leaves a claim that the next start reclaims or marks failed (liveness floor, Pattern 28). Concurrency: at most 3 runs in flight, at most 6 retriever assignments in flight, one Verifier at a time per board (so batch stale flags do not race). Provider rate limits are enforced in the provider layer with per provider queues.

## 7. Integration architecture

- Model providers: one trait, `complete(packet) -> output`, with adapters per provider; structured output via JSON mode where the provider has it, else schema prompting plus validation. Tool use (web search inside the model call) is disabled; retrieval is always the core's job so provenance is uniform.
- Search providers: one trait, `search(query, filters) -> results`, adapters per API.
- Corpus subscriptions: a manifest per corpus (fetch URL, version discovery, parser); the finance pack ships manifests for EU consolidated texts as examples; the user can add manifests.
- Image generation: one trait, off unless a provider key is present and the pack allows it.

## 8. Authentication and authorisation

Single user; the OS user is the identity. Profile database is readable only by the OS user (file permissions). Keys in the keychain, unlocked by the OS. Bundles carry the exporter's display name only.

## 9. Reference data

Doctrine packs (`general`, `finance-eu`, `finance-eu-synthetic`) ship in the app bundle as JSON with a schema. The Profile shows the loaded version and allows import of a pack file. A pack update never rewrites a board's pinned version; the board offers "update pack" which reruns `verify_only`.

## 10. Provider abstraction and model policy

Aliases resolve at run start (Router) and are snapshotted on the Run. Fallback order per alias from the policy. Ollama adapter for local models, which makes a fully offline configuration possible with reduced quality; the Profile shows an "offline capable" badge when every stage resolves to a local alias.

## 11. Observability

Local only: a log file per day with structured lines mirroring events; a diagnostics page in Profile showing runs, failures by type, provider latency percentiles, spend by provider. "Export diagnostics" produces a zip with logs and the last N runs' events with prompt text redacted. No remote reporting.

## 12. Security

- Prompt injection: retrieved text and image text are data; the Synthesizer prompt marks passages as quoted data; deterministic detectors on known patterns; the Verifier flags outputs that follow injected instructions; hostile document cases are in the eval and must pass at 100 percent.
- Hooks deny excluded paths and domains before any fetch; denials are logged.
- Sanitisation of svg and of any html in v1.1 widgets by allowlist.
- Bundles are validated against schema on import; blobs are hashed and verified; no code in bundles.
- Updates: signed installers; the app checks for updates only when the user asks in v1.

## 13. Deployment

Two targets, macOS (universal binary, notarised) and Windows (x64, signed msi). CI builds on tag; the eval harness runs on the synthetic corpus in CI with the mock provider for structure and, on a nightly schedule, with real providers using a CI key for the numbers.

The web client (reduced) is a later target: the same UI against a hosted core with keys in the browser session and only the web retriever enabled. It is out of the build prompt's scope and named here so the RPC boundary is kept clean.

## 14. Cost model

Per card, typical: fast 1 medium call; deep 1 small + 1 frontier + 1 medium (~15k tokens total); research 1 small + 1 medium + 3 medium (research notes) + 1 frontier + 1 medium (~45k tokens). The Profile shows spend per provider from `Run.cost`; the composer shows an estimate next to the depth selector after ten runs.

## 15. Disaster recovery

The profile folder is the unit. Back up, restore, and "open profile from folder" are the three operations. A corrupted SQLite file is detected on start; the app offers restore from the last backup and keeps the damaged file aside.

## 16. Regulatory layer

The product processes user documents locally and sends passages to configured providers. The Profile's Retrievers page states, per folder, whether chunk text leaves the machine (provider embeddings, support check) so a user in a regulated organisation can keep sensitive folders fully local. Bundles show the local document checklist on export. These are the two places where data leaves the device by user choice, and both are visible and logged.

## 17. Open questions

1. Tauri versus Electron is decided for Tauri pending a Windows webview check on the canvas performance with 200 cards. The build prompt includes that check as phase 0.
2. Local embedding model choice and size (quality versus download size). Decide in phase 2 of the build with the synthetic recall numbers.
3. Whether the support check may use a local model for sensitive folders at acceptable quality; measure on the synthetic corpus with an Ollama alias before promising it.
