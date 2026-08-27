/**
 * The JSON-RPC client.
 *
 * Doc 10 section 2: the core is a library behind a JSON-RPC boundary, and the
 * desktop shell is the first client. Everything this UI knows how to do is a
 * method on that surface, so the reduced web client that arrives later swaps
 * this one transport for a socket and changes nothing above it.
 */

import type { Board, Severity } from './canvas/types.js';
import { COPY } from './strings.js';

export interface RpcErrorShape {
  code: number;
  message: string;
  data?: { kind?: string };
}

export class RpcError extends Error {
  readonly code: number;
  /** The failure taxonomy code, so callers branch on the category. */
  readonly kind: string;

  constructor(e: RpcErrorShape) {
    super(e.message);
    this.name = 'RpcError';
    this.code = e.code;
    this.kind = e.data?.kind ?? 'unknown';
  }
}

type Transport = (request: string) => Promise<string>;

function tauriTransport(): Transport | null {
  const g = window as unknown as {
    __TAURI__?: { core?: { invoke?: (cmd: string, args: Record<string, unknown>) => Promise<unknown> } };
  };
  const invoke = g.__TAURI__?.core?.invoke;
  if (!invoke) return null;
  return async (request) => String(await invoke('rpc', { request }));
}

let nextId = 1;

export class Rpc {
  private readonly transport: Transport | null;

  constructor(transport: Transport | null = tauriTransport()) {
    this.transport = transport;
  }

  /** False in a plain browser, where there is no core behind the page. */
  get connected(): boolean {
    return this.transport !== null;
  }

  async call<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    if (!this.transport) {
      throw new RpcError({
        code: -32000,
        message: COPY.notConnected,
        data: { kind: 'disconnected' },
      });
    }

    const raw = await this.transport(
      JSON.stringify({ jsonrpc: '2.0', method, params, id: nextId++ }),
    );

    let parsed: { result?: T; error?: RpcErrorShape };
    try {
      parsed = JSON.parse(raw);
    } catch {
      throw new RpcError({
        code: -32700,
        message: 'The core sent something this page could not read.',
        data: { kind: 'parse_error' },
      });
    }

    if (parsed.error) throw new RpcError(parsed.error);
    return parsed.result as T;
  }

  // The registered surface, one method each. Adding a call here without adding
  // it in the core is a runtime method_not_found, which is the intended
  // failure: the boundary is the contract.

  createBoard(title = 'Untitled board', depth = 'fast') {
    return this.call<{ board_id: string }>('board.create', { title, depth });
  }

  /** Doc 09 open question 1: Trash is a filter on Home, so it is this word. */
  listBoards(status: 'active' | 'trashed' = 'active') {
    return this.call<{ boards: BoardSummary[] }>('board.list', { status });
  }

  trashBoard(boardId: string) {
    return this.call<{ board_id: string }>('board.trash', { board_id: boardId });
  }

  restoreBoard(boardId: string) {
    return this.call<{ board_id: string }>('board.restore', { board_id: boardId });
  }

  /** The one verb with nothing behind it. The core refuses it on a live board. */
  purgeBoard(boardId: string) {
    return this.call<{ board_id: string }>('board.purge', { board_id: boardId });
  }

  /** Doc 09 section 6: open flags across every board, not one board's. */
  flags(limit?: number) {
    return this.call<{ flags: FlagRow[] }>('flag.list', limit === undefined ? {} : { limit });
  }

  decideFlags(flagIds: string[], decision: FlagDecision, note?: string) {
    return this.call<{ review_id: string; decided: number }>('flag.decide', {
      flag_ids: flagIds,
      decision,
      note,
    });
  }

  /** Doc 01 section 4.10: agents propose, the user confirms. */
  decideConcept(conceptId: string, accept: boolean) {
    return this.call<{ concept_id: string; term: string }>('concept.decide', {
      concept_id: conceptId,
      accept,
    });
  }

  /** Doc 08 section 3: on demand from a board. */
  makeExercise(boardId: string, audienceId?: string) {
    return this.call<{ exercise_id: string | null; items: number; dropped: number }>(
      'exercise.create',
      { board_id: boardId, audience_id: audienceId },
    );
  }

  exercises(boardId: string) {
    return this.call<{ exercises: ExerciseRow[] }>('exercise.list', { board_id: boardId });
  }

  /** The score is computed in the store from the exercise's own items. */
  attempt(exerciseId: string, answers: Record<string, string>) {
    return this.call<{ attempt_id: string; correct: number; total: number }>('exercise.attempt', {
      exercise_id: exerciseId,
      answers,
    });
  }

  reportItem(exerciseId: string, itemId: string, reason?: string) {
    return this.call<{ reported: string }>('exercise.report_item', {
      exercise_id: exerciseId,
      item_id: itemId,
      reason,
    });
  }

  sources(limit?: number) {
    return this.call<{ sources: SourceRow[] }>('library.sources', limit === undefined ? {} : { limit });
  }

  concepts(limit?: number) {
    return this.call<{ concepts: ConceptRow[] }>(
      'library.concepts',
      limit === undefined ? {} : { limit },
    );
  }

  getBoard(boardId: string) {
    return this.call<Board>('board.get', { board_id: boardId });
  }

  /**
   * Doc 09 section 5's Branch verb, in its three forms. With no anchor at all
   * the card is a root; with a parent alone it is a follow-up; with a parent and
   * either anchor it is a branch. The core rejects an anchor with no parent,
   * because a span belongs to the card it was selected on.
   */
  ask(boardId: string, question: string, depth?: string, anchor: AskAnchor = {}) {
    return this.call<AskResult>('card.ask', {
      board_id: boardId,
      question,
      depth,
      parent_card_id: anchor.parentCardId,
      anchor_text: anchor.anchorText,
      anchor_block_ref: anchor.anchorBlockRef,
    });
  }

  /** Doc 09 section 5's Rerun verb: check the card again, retrieve nothing. */
  verify(boardId: string, cardId: string) {
    return this.call<AskResult>('card.verify', { board_id: boardId, card_id: cardId });
  }

  rename(boardId: string, title: string) {
    return this.call<{ board_id: string; title: string }>('board.rename', {
      board_id: boardId,
      title,
    });
  }

  history(boardId: string) {
    return this.call<{ events: HistoryEntry[] }>('board.history', { board_id: boardId });
  }

  notifications(boardId: string, after = 0) {
    return this.call<{ notifications: Notification[]; index: number }>('board.notifications', {
      board_id: boardId,
      after,
    });
  }

  profile() {
    return this.call<ProfileSummary>('profile.get');
  }

  /**
   * Hand a key to the keychain. The secret crosses this boundary once, going in,
   * and nothing ever sends it back.
   */
  setKey(keyRef: string, secret: string) {
    return this.call<{ key_ref: string; key_present: boolean }>('profile.set_key', {
      key_ref: keyRef,
      secret,
    });
  }
}

export interface BoardSummary {
  id: string;
  title: string;
  updated_at: string;
  mode: string;
  cards: number;
  open_flags: number;
}

/** Where a new card hangs from. Mirrors `Anchor` in `tessera-core`. */
export interface AskAnchor {
  parentCardId?: string;
  /** The highlighted span, for the highlight to branch verb. */
  anchorText?: string;
  /** A JSON pointer into the parent visual payload, for block investigate. */
  anchorBlockRef?: string;
}

/** Doc 09 section 5's eight verbs, as the four a flag accepts. */
export type FlagDecision = 'accept' | 'dismiss' | 'rerun' | 'edit';

/** One row of the Flags queue. Doc 09 section 6. */
export interface FlagRow {
  id: string;
  rule_id: string;
  severity: Severity;
  reason: string;
  /** The passage excerpt or the stale date, whichever the rule wrote. */
  evidence: unknown;
  created_at: string;
  card_id: string;
  card_title: string;
  board_id: string;
  board_title: string;
}

/** Library, Sources tab. Doc 09 section 9. */
export interface SourceRow {
  id: string;
  title: string;
  class: string;
  issuer: string | null;
  locator: string;
  trust_rank: number;
  last_verified_at: string | null;
  stale: boolean;
  stale_reason: string | null;
  freshness_class: string;
  version_ref: string | null;
  cards: number;
}

/** Library, Concepts tab. Doc 09 section 9. */
export interface ConceptRow {
  id: string;
  term: string;
  status: 'proposed' | 'confirmed';
  definition: string | null;
  aliases: unknown;
  audience_definitions: unknown;
  definition_card_id: string | null;
  updated_at: string;
  links: number;
}

/** One item of an exercise. Doc 08 section 5. */
export interface ExerciseItem {
  id: string;
  kind: 'recall' | 'apply' | 'contrast' | 'trace';
  prompt: string;
  options: { id: string; text: string }[];
  answer_id: string;
  explanation: string;
  source_card_id: string;
  citation_ordinals?: number[];
}

export interface ExerciseRow {
  id: string;
  items: ExerciseItem[];
  template_id: string;
  audience_id: string | null;
  created_at: string;
  last_score: { correct: number; total: number } | null;
}

export interface AskResult {
  card_id: string;
  run_id: string;
  status: string;
  confidence: number;
  flags: number;
}

export interface HistoryEntry {
  event_id: string;
  index: number;
  type: string;
  payload: unknown;
  actor: string;
  actor_type: string;
  card_id: string | null;
  at: string;
}

/**
 * Doc 11 section 6's Profile pages, in one read.
 *
 * `key_present` rather than the key. Doc 10 section 8 and the standing rule:
 * the secret lives in the OS keychain and is never printed, logged or passed
 * as an argument, so the boundary can only report whether the keychain has it.
 */
export interface AliasStatus {
  alias: string;
  provider: string;
  model: string;
  key_ref: string;
  key_present: boolean;
}

export interface RetrieverStatus {
  id: string;
  enabled_by_default: boolean;
  /** Doc 05 section 10 separates this from configured and empty. */
  configured: boolean;
}

export interface Diagnostics {
  boards: number;
  boards_trashed: number;
  cards: number;
  open_flags: number;
  sources: number;
  sources_stale: number;
  concepts: number;
  events: number;
}

export interface ProfileSummary {
  profile_id: string;
  packs: string[];
  active_pack: string;
  provider: string;
  policy: unknown;
  aliases?: AliasStatus[];
  retrievers?: RetrieverStatus[];
  diagnostics?: Diagnostics;
}

/** The bridge's vocabulary. Mirrors `crates/tessera-core/src/bridge.rs`. */
export type Notification =
  | { kind: 'card_stage'; card_id: string; label: string; done: boolean }
  | { kind: 'card_updated'; card_id: string }
  | { kind: 'card_answered'; card_id: string; status: string; confidence: number | null }
  | { kind: 'card_failed'; card_id: string; reason: string }
  | { kind: 'flag_raised'; card_id: string; rule_id: string; severity: string }
  | { kind: 'flag_resolved'; card_id: string }
  | { kind: 'board_updated'; board_id: string }
  | { kind: 'toast'; level: 'info' | 'warn' | 'error'; message: string };
