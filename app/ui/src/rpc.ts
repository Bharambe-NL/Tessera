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

  // Doc 14 section 3.3's triggers, one method each, because doc 14 section
  // 3.4's machine moves on what the learner did.

  startLearn(boardId: string, topic: string) {
    return this.call<{ session_id: string; turn: TutorTurn }>('learn.start', {
      board_id: boardId,
      topic,
    });
  }

  learnSession(boardId: string) {
    return this.call<{ session: LearnSession | null }>('learn.get', { board_id: boardId });
  }

  answerIntake(boardId: string, q: string, a: string) {
    return this.call<{ recorded: boolean }>('learn.answer_intake', { board_id: boardId, q, a });
  }

  buildPlan(boardId: string) {
    return this.call<{ turn: TutorTurn }>('learn.build', { board_id: boardId });
  }

  askCheck(boardId: string, cardId?: string) {
    return this.call<{ turn: TutorTurn }>('learn.check', { board_id: boardId, card_id: cardId });
  }

  answerCheck(boardId: string, item: ExerciseItem, picked: string, conceptIds: string[] = []) {
    return this.call<CheckResult>('learn.answer_check', {
      board_id: boardId,
      item,
      picked,
      concept_ids: conceptIds,
    });
  }

  sayToTutor(boardId: string, message: string) {
    return this.call<{ turn: TutorTurn }>('learn.say', { board_id: boardId, message });
  }

  endLearn(boardId: string) {
    return this.call<{ checks: number; correct: number; mastery: Record<string, number> }>(
      'learn.end',
      { board_id: boardId },
    );
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
   * Doc 10 section 9's update pack. Nothing is retrieved and no answer is
   * rewritten: the cards are judged again under the version the board moves to.
   */
  updateBoardPack(boardId: string) {
    return this.call<{
      board_id: string;
      pack_code: string;
      from_version: string;
      to_version: string;
      updated: boolean;
      cards: { card_id: string; status: string; flags?: number }[];
    }>('board.update_pack', { board_id: boardId });
  }

  // ---------------------------------------------------------- notebook --
  // Doc 16 section 3.4. A session is a board, so what the shell asks for is
  // one board's turns and their grounding.

  openNotebook(boardId?: string) {
    return this.call<{ board_id: string; mode: string }>(
      'notebook.open',
      boardId === undefined ? {} : { board_id: boardId },
    );
  }

  notebookSessions() {
    return this.call<{ sessions: BoardSummary[] }>('notebook.sessions');
  }

  notebookSession(boardId: string) {
    return this.call<NotebookSession>('notebook.session', { board_id: boardId });
  }

  /** Doc 16 section 3.4: a question from the session grows into a board. */
  openOnBoard(boardId: string, cardId: string) {
    return this.call<{ board_id: string; card_id: string; status: string }>(
      'notebook.open_on_board',
      { board_id: boardId, card_id: cardId },
    );
  }

  // --------------------------------------------------------------- map --
  // Doc 17 section 6's Map view, over the concept rows and the edges between
  // them. The depth and the frontier come from the core, because both are rules
  // the product owns and a second answer drawn here would be a second product.

  readMap() {
    return this.call<MapRead>('map.read');
  }

  /** Doc 17 section 6's node panel, read when a node opens rather than always. */
  mapConcept(conceptId: string) {
    return this.call<{ cards: ConceptCard[]; pages: ConceptPage[] }>('map.concept', {
      concept_id: conceptId,
    });
  }

  /** Doc 17 section 2.1: a rating is a claim, and the learner makes it. */
  rateConcept(conceptId: string, rating: number) {
    return this.call<{ concept_id: string; rating: number }>('concept.rate', {
      concept_id: conceptId,
      rating,
    });
  }

  /** Doc 17 section 7: agents propose an edge, the learner confirms it. */
  confirmEdge(edgeId: string) {
    return this.call<{ edge_id: string; confirmed: boolean }>('concept.confirm_edge', {
      edge_id: edgeId,
    });
  }

  // ------------------------------------------------------------- pages --
  // Doc 16 section 3.7's rail item, over the five verbs the core registers.

  pages(limit?: number) {
    return this.call<{ pages: PageRow[] }>('page.list', limit === undefined ? {} : { limit });
  }

  page(pageId: string) {
    return this.call<PageDetail>('page.get', { page_id: pageId });
  }

  writePage(page: { page_id?: string; title: string; body: string }) {
    return this.call<{ page_id: string; title: string | null; file_path: string | null }>(
      'page.write',
      page,
    );
  }

  deletePage(pageId: string) {
    return this.call<{ page_id: string; deleted: boolean }>('page.delete', { page_id: pageId });
  }

  /** Doc 16 section 3.1: an unresolved link creates the page it names. */
  createPageFromLink(title: string) {
    return this.call<{ page_id: string; title: string }>('page.create_from_link', { title });
  }

  /**
   * Doc 16 section 3.2's ninth verb. The page carries the card's citations
   * rather than becoming one, so what the next answer cites is still the
   * passage the card found.
   */
  saveAsPage(boardId: string, cardId: string) {
    return this.call<{
      page_id: string;
      title: string | null;
      file_path: string | null;
      citations_carried?: number;
      created: boolean;
    }>('card.save_as_page', { board_id: boardId, card_id: cardId });
  }

  /**
   * Doc 16 section 3.6's "Add note": a sticky carrying the quote it was made
   * from, and the card it was written beside.
   */
  createNote(
    boardId: string,
    text: string,
    options: { cardId?: string; position?: { x: number; y: number; w: number; h: number } } = {},
  ) {
    return this.call<{ note_id: string; board_id: string }>('note.create', {
      board_id: boardId,
      text,
      ...(options.cardId ? { card_id: options.cardId } : {}),
      ...(options.position ? { position: options.position } : {}),
    });
  }

  /** The undo Add note has. Doc 09 section 5: every verb has one. */
  removeNote(noteId: string) {
    return this.call<{ note_id: string }>('note.remove', { note_id: noteId });
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

  /**
   * Read an image into a card. Doc 07 part A.
   *
   * The bytes go over as base64 because the boundary is JSON-RPC and a webview
   * has no path to the blob store. The core writes them once, by hash.
   */
  addImage(boardId: string, data: string, mime: string, width: number, height: number) {
    return this.call<{ image_id: string }>('board.add_image', {
      board_id: boardId,
      data,
      mime,
      width,
      height,
    });
  }

  read(boardId: string, imageId: string) {
    return this.call<AskResult>('card.read', { board_id: boardId, image_id: imageId });
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

  /** Doc 11 section 6's First run, asked of the core rather than inferred. */
  firstRun() {
    return this.call<FirstRun>('profile.first_run');
  }

  setPack(code: string) {
    return this.call<{ active_pack: string }>('profile.set_pack', { code });
  }

  /**
   * Doc 10 section 9. The core reads the file and validates it; importing does
   * not activate, because a pack change is a deliberate act.
   */
  importPack(path: string) {
    return this.call<{
      code: string;
      version: string;
      name: string | null;
      audiences: number;
      flag_rules: number;
      source_ranks: number;
      retrievers: string[];
      built_in: boolean;
      active: boolean;
    }>('pack.import', { path });
  }

  watchFolder(folder: {
    root: string;
    label: string;
    sensitive?: boolean;
    provider_embeddings?: boolean;
  }) {
    return this.call<{
      folder_id: string;
      label: string;
      sensitive: boolean;
      text_leaves_machine: boolean;
      /** What the walk that added the folder found. Doc 05 sections 8.2 and 11. */
      indexed: number;
      chunks: number;
      excluded: number;
      errors: { path: string; kind: string; detail: string }[];
    }>('profile.watch_folder', folder);
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

/** One concept on the map. Doc 17 sections 2.1 and 6. */
export interface MapConcept {
  concept_id: string;
  term: string;
  /** Doc 17 section 2.3's six states, or null for a concept nothing has touched. */
  learning_state:
    | 'unseen'
    | 'exposed'
    | 'rated'
    | 'checked'
    | 'mastered'
    | 'decayed'
    | null;
  self_rating: number | null;
  mastery: number | null;
  difficulty_level: number | null;
  last_evidence_at: string | null;
  path_ids: string[];
  linked_cards: number;
  /** How deep in the prerequisite order, counting from zero. The core's rule. */
  depth: number;
}

/** One prerequisite edge. Doc 17 section 2.1. */
export interface MapEdge {
  edge_id: string;
  from_concept_id: string;
  to_concept_id: string;
  relation: string;
  status: 'proposed' | 'confirmed' | 'rejected';
  weight: number;
}

export interface MapRead {
  board_id: string;
  concepts: MapConcept[];
  edges: MapEdge[];
  /** Doc 17 section 3: where the learner stands, decided by the core. */
  frontier: string[];
  mission: { mission_id: string; statement: string; target_concept_ids: string[] } | null;
  mastered_at: number;
}

export interface ConceptCard {
  card_id: string;
  board_id: string;
  question: string;
  board_title: string;
}

export interface ConceptPage {
  page_id: string;
  title: string;
}

/**
 * What a graded check decided. Doc 17 section 4.
 *
 * `next_level` is the rung the next check on this concept opens at, and
 * `remedy` is what a failure calls for. Doc 14 section 3.7: a remedy is offered
 * and never taken, so nothing here happens on its own.
 */
export interface CheckResult {
  correct: boolean;
  level: number;
  next_level: number;
  remedy:
    | { kind: 'none' }
    | { kind: 'card'; level: number }
    | { kind: 'prerequisite'; concept_id: string; level: number };
}

/** One item of an exercise. Doc 08 section 5, with doc 17 section 4's ladder. */
export interface ExerciseItem {
  id: string;
  kind: 'recall' | 'apply' | 'contrast' | 'trace' | 'explain' | 'discriminate';
  prompt: string;
  options: { id: string; text: string }[];
  answer_id: string;
  explanation: string;
  source_card_id: string;
  citation_ordinals?: number[];
  concept_ids?: string[];
  /** Which rung of doc 17 section 4 this item checks, when a level was asked for. */
  level?: 1 | 2 | 3 | 4;
}

export interface ExerciseRow {
  id: string;
  items: ExerciseItem[];
  template_id: string;
  audience_id: string | null;
  created_at: string;
  last_score: { correct: number; total: number } | null;
}

/** Doc 14 section 2's LearnSession, as the panel reads it. */
export interface LearnSession {
  session_id: string;
  board_id: string;
  topic: string;
  status: 'intake' | 'building' | 'reading' | 'checking' | 'ended';
  intake: { q: string; a: string }[];
  plan: { question: string; why: string; visual_hint?: string }[];
  checks: { item_id: string; card_id: string; picked: string; correct: boolean }[];
  opened: { question?: string; card_id?: string; reason: string }[];
  mastery: Record<string, number>;
}

/** What one Tutor turn decided. Doc 14 section 3.5. */
export interface TutorTurn {
  stage: string;
  questions?: { q: string; options: string[] }[];
  plan?: { title: string; cards: { question: string; why: string }[] };
  check?: {
    item: ExerciseItem;
    next_if_right?: string | null;
    next_if_wrong?: string | null;
    /** Doc 17 section 6: which concept this check is about, so grading can move it. */
    concept_id?: string;
  };
  reply?: string;
  open?: string | null;
  caveats?: string[];
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

/** Doc 11 section 6's First run. What is set up, and what is still missing. */
export interface FirstRun {
  needs_setup: boolean;
  has_key: boolean;
  boards: number;
  folders: number;
  packs: string[];
  active_pack: string;
  key_refs: string[];
}

/** One question and its answer in a session. Doc 16 section 3.4. */
export interface NotebookTurn {
  card_id: string;
  question: string;
  answer: string | null;
  status: string;
  page_id: string | null;
  citations: { ordinal: number; source_title: string; source_class: string }[];
  /** Doc 16 section 3.4's three states, plus `unknown` for a card asked before
   * the board became a session. */
  grounding: 'grounded' | 'partly_grounded' | 'ungrounded' | 'unknown';
}

export interface NotebookSession {
  board_id: string;
  title: string;
  mode: string;
  turns: NotebookTurn[];
}

/** One page in the explorer. Doc 16 section 3.7. */
export interface PageRow {
  id: string;
  title: string;
  file_path: string;
  updated_at: string;
  /** Saved from a card rather than written by hand. Doc 16 section 3.2. */
  from_card: boolean;
  citations_carried: number;
}

/** One page open, with what points at it. */
export interface PageDetail {
  id: string;
  title: string;
  body: string;
  file_path: string;
  updated_at: string;
  source_card_id: string | null;
  citations_carried: { ordinal: number; passage_id: string }[];
  links: {
    target_kind: 'page' | 'concept' | 'unresolved';
    target_id: string | null;
    target_title: string;
    display_text: string;
    position: number;
  }[];
  backlinks: { page_id: string; title: string; display_text: string; position: number }[];
}

export interface PackStatus {
  code: string;
  /** Ships with the app, so it is the same on every machine. Doc 10 section 9. */
  built_in: boolean;
  active: boolean;
}

/** A pack file in the profile folder that did not load, and why. */
export interface PackProblem {
  file: string;
  detail: string;
}

export interface ProfileSummary {
  profile_id: string;
  packs: string[];
  pack_details?: PackStatus[];
  pack_problems?: PackProblem[];
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
