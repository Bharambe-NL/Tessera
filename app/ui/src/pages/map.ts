/**
 * The Map. Doc 17 section 6 and phase 13d.
 *
 * "Nodes sized by linked cards, coloured by the six states, confirmed edges
 * solid, proposed dotted, frontier band; layered by prerequisite depth, never
 * hand arranged."
 *
 * Layered means the layout is a fact about the prerequisites rather than a
 * position anyone dragged: a concept sits in the band its depth puts it in, and
 * moving it would be moving what it depends on. The depth and the frontier both
 * come from the core, which owns those rules; this file draws what it is told.
 *
 * The drawing is one SVG. A card on the canvas is a DOM node because it holds
 * text a person selects and a menu they open; a map node holds a term and a
 * click, and two hundred of those as absolutely positioned divs would cost the
 * canvas gate for nothing.
 */

import { esc } from '../canvas/visual.js';
import type { ConceptCard, ConceptPage, MapConcept, MapEdge, MapRead } from '../rpc.js';
import { COPY } from '../strings.js';
import { ago, emptyState } from './shared.js';

/** Doc 17 section 2.3's six states, plus the filter that shows every one. */
export type MapFilter = 'all' | 'unseen' | 'exposed' | 'rated' | 'checked' | 'mastered' | 'decayed';

export interface MapState {
  /** What the last read returned, kept so opening a node needs no second read. */
  map: MapRead | null;
  /** The concept whose panel is open, or null for the map alone. */
  open: MapConcept | null;
  /** Doc 17 section 6's node panel content, read when the node opens. */
  links: { cards: ConceptCard[]; pages: ConceptPage[] } | null;
  filter: MapFilter;
  /** Doc 17 section 6's filter by mission: only what the mission targets. */
  missionOnly: boolean;
  /**
   * Doc 17 section 3's placement pass. Null until a read has been seen, so the
   * first map with something to rate opens on the tiles and one the learner
   * left does not reopen behind them.
   */
  placing: boolean | null;
  /** The tiles this learner passed over. Doc 17 section 3: any tile is skippable. */
  skipped: string[];
}

/**
 * The tiles placement asks about. Doc 17 section 3.
 *
 * "In prerequisite order", which is depth then term: the same order the map
 * lays its bands out in, so a learner who rates top to bottom meets what
 * everything else rests on first.
 *
 * A skip is client state and nothing else. A rating is a claim the product
 * records; declining to make one is not a second kind of claim, and writing it
 * down would put a row in the log saying the learner decided something they
 * decided not to decide.
 */
export function placementTiles(state: MapState): MapConcept[] {
  const skipped = new Set(state.skipped);
  return unrated(state).filter((c) => !skipped.has(c.concept_id));
}

/**
 * Everything the learner has not rated, skips included.
 *
 * What decides whether the way back into placement is on the toolbar. Reading
 * the tiles for that would take the way back away the moment somebody skipped
 * the last one, which is exactly when they are most likely to want it.
 */
export function unrated(state: MapState): MapConcept[] {
  return visible(state)
    .filter((c) => c.self_rating === null)
    .sort((a, b) => a.depth - b.depth || a.term.localeCompare(b.term));
}

/** Node geometry. A band per depth, a column per concept within it. */
const BAND = 96;
const COLUMN = 132;
const MARGIN = 44;
const RADIUS_MIN = 14;
const RADIUS_MAX = 30;

interface Placed {
  concept: MapConcept;
  x: number;
  y: number;
  r: number;
}

export function mapToolsHTML(state: MapState): string {
  if (state.open) {
    return `<div class="seg"><button data-map-act="close">${COPY.mapBack}</button></div>`;
  }
  const tiles = placementTiles(state).length;
  if (state.placing && tiles > 0) {
    return `<div class="seg"><button data-map-act="placed">${COPY.placeDone}</button></div>`;
  }
  const states: [MapFilter, string][] = [
    ['all', COPY.mapAll],
    ['rated', COPY.mapRated],
    ['checked', COPY.mapChecked],
    ['mastered', COPY.mapMastered],
    ['decayed', COPY.mapDecayed],
  ];
  const buttons = states
    .map(
      ([key, label]) =>
        `<button data-map-filter="${key}"${state.filter === key ? ' class="on"' : ''}>` +
        `${label}</button>`,
    )
    .join('');
  // Doc 17 section 6's filter by mission. Shown only when there is one, because
  // a control that can never change anything is a control that misleads.
  const mission = state.map?.mission
    ? `<div class="seg"><button data-map-act="mission"${state.missionOnly ? ' class="on"' : ''}>` +
      `${COPY.mapMissionOnly}</button></div>`
    : '';
  // The way back into placement, shown only while something is unrated.
  const place =
    unrated(state).length > 0
      ? `<div class="seg"><button data-map-act="place">${COPY.placeOpen}</button></div>`
      : '';
  return `<div class="seg">${buttons}</div>${mission}${place}`;
}

/**
 * Doc 17 section 3's placement: tiles in prerequisite order, four tappable
 * levels each, and a skip.
 *
 * Every tile at once rather than one at a time, because the learner is being
 * asked what they know about a subject and the shape of the subject is part of
 * the question: a list they can see the end of is a list they will finish.
 */
function placementHTML(tiles: MapConcept[]): string {
  const levels = [COPY.mapRating0, COPY.mapRating1, COPY.mapRating2, COPY.mapRating3];
  const rows = tiles
    .map((c) => {
      const buttons = levels
        .map(
          (label, n) =>
            `<button data-place-rate="${n}" data-place-concept="${esc(c.concept_id)}">` +
            `${label}</button>`,
        )
        .join('');
      return (
        `<li class="place-tile" data-place-tile="${esc(c.concept_id)}">` +
        `<h4>${esc(c.term)}</h4>` +
        `<div class="seg map-rate">${buttons}</div>` +
        `<button class="place-pass" data-place-skip="${esc(c.concept_id)}">${COPY.placeSkip}</button>` +
        `</li>`
      );
    })
    .join('');
  return (
    `<section class="placement">` +
    `<h3>${COPY.placeHead}</h3>` +
    `<p class="map-line">${COPY.placeWhy}</p>` +
    `<p class="map-line muted">${tiles.length} ${COPY.placeLeft}</p>` +
    `<ul class="place-tiles">${rows}</ul>` +
    `</section>`
  );
}

/**
 * Which concepts a filter leaves. Doc 17 section 6.
 *
 * A concept nothing has touched has a null state, and doc 17 section 2.3 calls
 * that `unseen`, so the filter reads it as such rather than dropping it.
 */
export function visible(state: MapState): MapConcept[] {
  const map = state.map;
  if (!map) return [];
  const targets = new Set(map.mission?.target_concept_ids ?? []);
  return map.concepts.filter((c) => {
    if (state.missionOnly && targets.size > 0 && !targets.has(c.concept_id)) return false;
    if (state.filter === 'all') return true;
    return (c.learning_state ?? 'unseen') === state.filter;
  });
}

/**
 * Where every node sits, from its depth and nothing else.
 *
 * Within a band the order is the term, so a map redrawn after a rating puts
 * every node back where it was. A layout that sorted by mastery would rearrange
 * itself under the reader every time they answered a question.
 */
export function layout(concepts: MapConcept[]): Placed[] {
  const bands = new Map<number, MapConcept[]>();
  for (const c of [...concepts].sort((a, b) => a.term.localeCompare(b.term))) {
    const band = bands.get(c.depth) ?? [];
    band.push(c);
    bands.set(c.depth, band);
  }
  const most = Math.max(1, ...[...bands.values()].map((b) => b.length));
  const placed: Placed[] = [];
  for (const [depth, band] of [...bands].sort((a, b) => a[0] - b[0])) {
    // Centred within the widest band, so a map does not lean left.
    const offset = ((most - band.length) * COLUMN) / 2;
    band.forEach((concept, i) => {
      placed.push({
        concept,
        x: MARGIN + offset + i * COLUMN + COLUMN / 2,
        y: MARGIN + depth * BAND + BAND / 2,
        r: radius(concept.linked_cards),
      });
    });
  }
  return placed;
}

/**
 * Doc 17 section 6: "nodes sized by linked cards".
 *
 * The square root, because area is what a reader compares and a radius that
 * grew with the count would make ten cards look a hundred times the size of
 * one. Capped so one much studied concept does not swallow its neighbours.
 */
function radius(cards: number): number {
  const grown = RADIUS_MIN + Math.sqrt(Math.max(0, cards)) * 5;
  return Math.min(RADIUS_MAX, grown);
}

export function mapHTML(state: MapState): string {
  const map = state.map;
  if (!map) return emptyState(COPY.mapEmpty);
  if (state.open) return panelHTML(state, state.open);

  // Doc 17 section 3: placement comes before the map, because the map is a
  // picture of what the learner knows and placement is where they say.
  const tiles = placementTiles(state);
  if (state.placing && tiles.length > 0) return placementHTML(tiles);

  const shown = visible(state);
  if (shown.length === 0) return emptyState(map.concepts.length === 0 ? COPY.mapEmpty : COPY.mapNone);

  const placed = layout(shown);
  const at = new Map(placed.map((p) => [p.concept.concept_id, p]));
  const width = Math.max(...placed.map((p) => p.x + p.r)) + MARGIN;
  const height = Math.max(...placed.map((p) => p.y + p.r)) + MARGIN;

  // Doc 17 section 6: the frontier is a band across the map, drawn behind the
  // nodes so a node on it reads as standing in it rather than wearing a badge.
  const frontier = new Set(map.frontier);
  const bands = [...new Set(placed.filter((p) => frontier.has(p.concept.concept_id)).map((p) => p.y))]
    .map(
      (y) =>
        `<rect class="map-band" x="0" y="${y - BAND / 2}" width="${width}" height="${BAND}"></rect>`,
    )
    .join('');

  const lines = map.edges
    .filter((e) => e.relation === 'prerequisite_of' && e.status !== 'rejected')
    .map((e) => edgeHTML(e, at))
    .filter(Boolean)
    .join('');

  const nodes = placed.map((p) => nodeHTML(p, frontier.has(p.concept.concept_id))).join('');

  return (
    `<div class="map-wrap">` +
    `<svg class="map" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" ` +
    `role="img" aria-label="${esc(COPY.mapAria)}">` +
    `${bands}${lines}${nodes}</svg>` +
    `<p class="map-key">${esc(COPY.mapKey)}</p>` +
    `</div>`
  );
}

/** Doc 17 section 6: "confirmed edges solid, proposed dotted". */
function edgeHTML(edge: MapEdge, at: Map<string, Placed>): string {
  const from = at.get(edge.from_concept_id);
  const to = at.get(edge.to_concept_id);
  if (!from || !to) return '';
  return (
    `<line class="map-edge ${esc(edge.status)}" data-edge="${esc(edge.edge_id)}" ` +
    `x1="${from.x}" y1="${from.y}" x2="${to.x}" y2="${to.y}"></line>`
  );
}

function nodeHTML(p: Placed, onFrontier: boolean): string {
  const state = p.concept.learning_state ?? 'unseen';
  return (
    `<g class="map-node state-${esc(state)}${onFrontier ? ' frontier' : ''}" ` +
    `data-concept="${esc(p.concept.concept_id)}" tabindex="0" role="button" ` +
    `aria-label="${esc(p.concept.term)}, ${esc(stateLabel(state))}">` +
    `<circle cx="${p.x}" cy="${p.y}" r="${p.r}"></circle>` +
    `<text x="${p.x}" y="${p.y + p.r + 14}" text-anchor="middle">${esc(p.concept.term)}</text>` +
    `</g>`
  );
}

/** Doc 17 section 2.3's states, in the words a learner would use. */
export function stateLabel(state: string): string {
  switch (state) {
    case 'exposed':
      return COPY.mapStateExposed;
    case 'rated':
      return COPY.mapStateRated;
    case 'checked':
      return COPY.mapStateChecked;
    case 'mastered':
      return COPY.mapStateMastered;
    case 'decayed':
      return COPY.mapStateDecayed;
    default:
      return COPY.mapStateUnseen;
  }
}

/** Doc 17 section 6's node panel. */
function panelHTML(state: MapState, concept: MapConcept): string {
  const stateName = stateLabel(concept.learning_state ?? 'unseen');
  // Doc 17 section 2.1: a rating is a claim with four tappable levels, and the
  // learner may change it. Shown as what each level says, not as a number.
  const ratings = [COPY.mapRating0, COPY.mapRating1, COPY.mapRating2, COPY.mapRating3]
    .map(
      (label, n) =>
        `<button data-map-rate="${n}"${concept.self_rating === n ? ' class="on"' : ''}>` +
        `${label}</button>`,
    )
    .join('');

  // Doc 17 section 2.4's score, said as what put it there.
  //
  // A rating sets a starting prior, so a concept nobody has checked still
  // carries a number, and calling that a score would show a claim as a
  // measurement. Doc 17 section 2.1 is explicit that a rating is a claim and
  // never evidence, so the panel says which of the two this number is. A
  // concept with neither has no score at all, because 0 percent would read as a
  // verdict on the learner rather than as an absence.
  const checked = ['checked', 'mastered', 'decayed'].includes(concept.learning_state ?? '');
  const mastery =
    concept.mastery === null
      ? `<p class="map-line">${COPY.mapNoScore}</p>`
      : `<p class="map-line">${checked ? COPY.mapScore : COPY.mapClaimed} ` +
        `${Math.round(concept.mastery * 100)}%` +
        (checked && concept.difficulty_level
          ? ` ${COPY.mapAtLevel} ${concept.difficulty_level}`
          : '') +
        `</p>` +
        (checked ? '' : `<p class="map-line">${COPY.mapUnchecked}</p>`);

  const evidence = concept.last_evidence_at
    ? `<p class="map-line">${COPY.mapLastSeen} ${esc(ago(concept.last_evidence_at))}</p>`
    : `<p class="map-line">${COPY.mapNoEvidence}</p>`;

  const cards = (state.links?.cards ?? [])
    .map(
      (c) =>
        `<li><button data-map-card="${esc(c.board_id)}">${esc(c.question)}</button>` +
        `<span class="muted">${esc(c.board_title)}</span></li>`,
    )
    .join('');
  const pages = (state.links?.pages ?? [])
    .map((p) => `<li><button data-map-page="${esc(p.page_id)}">${esc(p.title)}</button></li>`)
    .join('');

  const linked =
    cards || pages
      ? `<ul class="map-links">${cards}${pages}</ul>`
      : `<p class="map-line">${COPY.mapNoLinks}</p>`;

  return (
    `<section class="map-panel">` +
    `<h3>${esc(concept.term)}</h3>` +
    `<p class="map-line"><span class="chip state-${esc(concept.learning_state ?? 'unseen')}">` +
    `${esc(stateName)}</span></p>` +
    mastery +
    evidence +
    `<h4>${COPY.mapRateHead}</h4>` +
    `<div class="seg map-rate">${ratings}</div>` +
    `<h4>${COPY.mapLinkedHead}</h4>` +
    linked +
    `<div class="seg map-verbs">` +
    `<button data-map-act="lesson">${COPY.mapStartLesson}</button>` +
    `<button data-map-act="check">${COPY.mapCheckNow}</button>` +
    `<button data-map-act="explore">${COPY.mapExplore}</button>` +
    `</div>` +
    `</section>`
  );
}
