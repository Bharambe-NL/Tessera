/**
 * The board read model.
 *
 * These are projections the UI renders, not the storage entities. Names and
 * enum members mirror doc 01 exactly so a field never means two things across
 * the RPC boundary. The core owns the entities; this is the shape they arrive
 * in over JSON-RPC.
 */

export type Depth = 'fast' | 'deep' | 'research';
export type CardKind = 'root' | 'follow' | 'branch' | 'read' | 'exercise';
export type CardStatus = 'queued' | 'running' | 'done' | 'flagged' | 'failed';
export type VisualType =
  | 'tree'
  | 'table'
  | 'list'
  | 'steps'
  | 'figure'
  | 'image'
  | 'chart'
  | 'widget';
export type Severity = 'info' | 'warn' | 'block';
export type CitationVerdict = 'supported' | 'weak' | 'unsupported' | 'unchecked';

/** Doc 01 section 4.3.1. `citation_ordinals` reference `Citation.ordinal`. */
export interface TreeNode {
  label: string;
  note?: string;
  citation_ordinals?: number[];
  children?: TreeNode[];
}

export interface BottomLine {
  head: string;
  text: string;
  citation_ordinals?: number[];
}

export type VisualPayload =
  | { root: TreeNode }
  | { columns: string[]; rows: string[][]; bottom_line?: BottomLine }
  | {
      groups: { heading: string; items: { name: string; detail?: string; citation_ordinals?: number[] }[] }[];
      bottom_line?: BottomLine;
    }
  | { steps: { label: string; note?: string; citation_ordinals?: number[] }[] }
  | { svg: string; caption?: string }
  | { image_id: string; caption?: string; prompt?: string };

/**
 * Doc 01 section 4.3. `ref` is a JSON pointer into `payload`, which is how the
 * frontend raises "Investigate this further" with an exact reference and how
 * the Verifier binds blocks to citations.
 */
export interface BlockIndexEntry {
  ref: string;
  label: string;
  citation_ordinals: number[];
  no_claim?: boolean;
  /** Set by the Verifier's block_actions. A hidden block renders as a placeholder. */
  hidden?: boolean;
  hidden_reason?: string;
}

export interface Visual {
  id: string;
  type: VisualType;
  title: string;
  payload: VisualPayload;
  block_index: BlockIndexEntry[];
}

export interface Citation {
  ordinal: number;
  source_title: string;
  source_class: string;
  locator: string;
  verdict: CitationVerdict;
  stale?: boolean;
}

export interface Finding {
  text: string;
  citation_ordinals: number[];
}

export interface FlagSummary {
  id: string;
  rule_id: string;
  severity: Severity;
  reason: string;
}

/** Layout slot plus the user's drag offset. Doc 01 section 4.2 `position`. */
export interface Position {
  x: number;
  y: number;
  dx: number;
  dy: number;
  pinned: boolean;
}

/** One streaming stage line, derived from events. Doc 09 section 4. */
export interface Stage {
  label: string;
  done: boolean;
}

export interface Card {
  id: string;
  parent_card_id: string | null;
  kind: CardKind;
  anchor_text: string | null;
  anchor_block_ref: string | null;
  question: string;
  depth: Depth;
  audience_id: string | null;
  answer: string | null;
  findings: Finding[];
  visual: Visual | null;
  citations: Citation[];
  flags: FlagSummary[];
  status: CardStatus;
  /** Null until the Verifier has run. Fast mode is fixed at 0 and shows "Unverified". */
  confidence: number | null;
  model_alias: string | null;
  stages: Stage[];
  position: Position;
}

export interface Viewport {
  x: number;
  y: number;
  k: number;
}

export interface Board {
  id: string;
  title: string;
  named_by_user: boolean;
  doctrine_pack: { code: string; version: string };
  /**
   * Whether a newer version of the pack this board pinned is loaded. Doc 10
   * section 9: the board offers the update, and nothing takes it on the board's
   * behalf.
   */
  pack_update?: {
    available: boolean;
    pack_code?: string;
    pinned_version?: string;
    current_version?: string;
    pack_loaded?: boolean;
  };
  default_depth: Depth;
  mode: 'explore' | 'learn';
  parent_board_id: string | null;
  seed_label: string | null;
  viewport: Viewport;
  cards: Card[];
}
