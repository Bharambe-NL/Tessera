/**
 * The JSON-RPC client.
 *
 * Doc 10 section 2: the core is a library behind a JSON-RPC boundary, and the
 * desktop shell is the first client. Everything this UI knows how to do is a
 * method on that surface, so the reduced web client that arrives later swaps
 * this one transport for a socket and changes nothing above it.
 */

import type { Board } from './canvas/types.js';
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

  listBoards() {
    return this.call<{ boards: BoardSummary[] }>('board.list');
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

export interface ProfileSummary {
  profile_id: string;
  packs: string[];
  provider: string;
  policy: unknown;
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
