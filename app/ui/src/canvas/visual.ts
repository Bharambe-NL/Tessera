/**
 * Visual renderers.
 *
 * Ported from the prototype (`canvas-prototype.html:636`-651) and extended with
 * the block index from doc 01 section 4.3. Every clickable block carries its
 * JSON pointer in `data-ref`, so "Investigate this further" raises an exact
 * reference rather than a label match, and its citation ordinals in
 * `data-cites`.
 *
 * A block the Verifier hid renders as a placeholder carrying the flag reason
 * (doc 07 section B8.3: blocks that fail are hidden, never silently removed).
 */

import { COPY } from '../strings.js';
import type {
  BlockIndexEntry,
  BottomLine,
  FlowEdge,
  FlowNode,
  Tile,
  TreeNode,
  Visual,
} from './types.js';

export function esc(s: string): string {
  return s.replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] as string,
  );
}

/**
 * Turn `[1]` and `[1, 2]` markers into superscripts.
 *
 * Doc 01 open question 1 is resolved as "derived": the stored answer carries no
 * markers, and the core renders them from `Citation.claim_span` before handing
 * the string over. This function is the export rendering path, applied to text
 * the core has already marked up.
 */
export function citeMarkers(s: string): string {
  return s.replace(/\s?\[(\d+(?:,\s*\d+)*)\]/g, (_, group: string) =>
    group
      .split(/,\s*/)
      .map((n) => `<sup class="cite" data-n="${n}" role="doc-noteref">${n}</sup>`)
      .join(''),
  );
}

interface BlockLookup {
  (ref: string): BlockIndexEntry | undefined;
}

function lookupFor(visual: Visual): BlockLookup {
  const byRef = new Map(visual.block_index.map((b) => [b.ref, b]));
  return (ref) => byRef.get(ref);
}

/** A hidden block leaves a placeholder naming why, per doc 09 section 4. */
function hiddenBlock(entry: BlockIndexEntry): string {
  return `<div class="block-hidden" data-ref="${esc(entry.ref)}">${COPY.blockHidden} ${esc(
    entry.hidden_reason ?? COPY.blockHiddenUnexplained,
  )}</div>`;
}

/** Wrap one block's markup with its pointer, citations and hidden state. */
function block(ref: string, lookup: BlockLookup, render: (attrs: string) => string): string {
  const entry = lookup(ref);
  if (entry?.hidden) return hiddenBlock(entry);
  const cites = entry?.citation_ordinals ?? [];
  const attrs =
    ` data-ref="${esc(ref)}"` +
    (cites.length ? ` data-cites="${cites.join(',')}"` : '') +
    (entry?.no_claim ? ' data-no-claim="true"' : '');
  return render(attrs);
}

function bottomLine(bl: BottomLine | undefined): string {
  if (!bl) return '';
  return `<div class="bottom"><b>${esc(bl.head)}</b><p>${citeMarkers(esc(bl.text))}</p></div>`;
}

/** A cell that is a quantity reads right aligned, so its digits line up. */
const NUMERIC = /^[\s(]*[-+−~≈<>]?[$€£¥]?\s*\d[\d.,\s]*(%|bp|bps|[kmbKMB]n?|x)?[)\s]*$/;

/** A node's label and, when it has one, the note that explains it. */
function nodeText(label: string, note: string | undefined): string {
  return `<b>${esc(label)}</b>` + (note ? `<span>${esc(note)}</span>` : '');
}

/**
 * A tree as a nested outline with connector lines.
 *
 * Each child sits under its own parent, so a reader sees which part belongs to
 * which. The earlier renderer flattened every level into one row of chips,
 * which lost exactly that. Depth carries the emphasis: the root is solid, the
 * first level is tinted, deeper levels are plain, so colour means rank rather
 * than cycling through hues that mean nothing.
 */
function treeBranch(nodes: TreeNode[], path: string, depth: number, lookup: BlockLookup): string {
  if (nodes.length === 0) return '';
  const items = nodes
    .map((n, i) => {
      const ref = `${path}/${i}`;
      const node = block(
        ref,
        lookup,
        (attrs) =>
          `<div class="node clk d${Math.min(depth, 2)}"${attrs} tabindex="0" role="button">${nodeText(
            n.label,
            n.note,
          )}</div>`,
      );
      return `<li>${node}${treeBranch(n.children ?? [], `${ref}/children`, depth + 1, lookup)}</li>`;
    })
    .join('');
  return `<ul class="t-kids">${items}</ul>`;
}

/**
 * The edges that close a loop. A depth first walk in the order the nodes were
 * given marks an edge back to a node still on the walk's path, which is the
 * smallest set the reader would call "goes back".
 */
function backEdges(nodes: FlowNode[], edges: FlowEdge[]): Set<FlowEdge> {
  const out = new Map<string, FlowEdge[]>(nodes.map((n) => [n.id, []]));
  for (const e of edges) out.get(e.from)?.push(e);
  const state = new Map<string, 'open' | 'done'>();
  const back = new Set<FlowEdge>();
  const walk = (id: string): void => {
    state.set(id, 'open');
    for (const e of out.get(id) ?? []) {
      const s = state.get(e.to);
      if (s === 'open') back.add(e);
      else if (s === undefined && out.has(e.to)) walk(e.to);
    }
    state.set(id, 'done');
  };
  for (const n of nodes) if (!state.has(n.id)) walk(n.id);
  return back;
}

/**
 * Lay a flow out in layers, longest path first.
 *
 * Doc 16 section 3.5 asks for "a small layered layout". With the loop closing
 * edges set aside the flow has an order, and a node sits one layer below the
 * deepest thing that reaches it, which puts sources at the top and the ends at
 * the bottom. Within a layer a node sits under the average of its parents, so
 * lines cross as little as a small layout can manage, and a node a loop starts
 * from goes last, nearest the gutter its loop runs up.
 */
function layers(nodes: FlowNode[], edges: FlowEdge[], back: Set<FlowEdge>): FlowNode[][] {
  const forward = edges.filter((e) => !back.has(e));
  const depth = new Map(nodes.map((n) => [n.id, 0]));
  for (let pass = 0; pass < nodes.length; pass += 1) {
    let moved = false;
    for (const e of forward) {
      const from = depth.get(e.from);
      const to = depth.get(e.to);
      if (from === undefined || to === undefined) continue;
      if (to <= from) {
        depth.set(e.to, from + 1);
        moved = true;
      }
    }
    if (!moved) break;
  }
  const rows: FlowNode[][] = [];
  for (const n of nodes) {
    const d = Math.min(depth.get(n.id) ?? 0, nodes.length - 1);
    (rows[d] ??= []).push(n);
  }
  const dense = rows.filter((r) => r && r.length);

  const slot = new Map<string, number>();
  const loops = new Set(Array.from(back, (e) => e.from));
  dense.forEach((row, li) => {
    if (li > 0) {
      const at = (n: FlowNode): number => {
        const parents = forward.filter((e) => e.to === n.id && slot.has(e.from));
        if (!parents.length) return Number.MAX_SAFE_INTEGER;
        return parents.reduce((sum, e) => sum + (slot.get(e.from) ?? 0), 0) / parents.length;
      };
      row.sort((a, b) => Number(loops.has(a.id)) - Number(loops.has(b.id)) || at(a) - at(b));
    }
    row.forEach((n, i) => slot.set(n.id, (i + 0.5) / row.length));
  });
  return dense;
}

/** Geometry for a flow diagram, in the drawing's own pixels. */
const FLOW = { w: 392, gap: 12, nodeMax: 172, pitch: 54, pitchLong: 72, labelW: 176 } as const;

interface Placed {
  index: number;
  layer: number;
  x: number;
  y: number;
  w: number;
}

/** A rough width for a label at 11px, enough to reserve room for it. */
function labelWidth(text: string | undefined): number {
  return text ? text.length * 6.1 + 18 : 0;
}

/** A point on a cubic curve, for setting a label on the line it names. */
function cubic(p0: number, p1: number, p2: number, p3: number, t: number): number {
  const u = 1 - t;
  return u * u * u * p0 + 3 * u * u * t * p1 + 3 * u * t * t * p2 + t * t * t * p3;
}

/**
 * Draw a flow as boxes and arrows.
 *
 * Layers run top to bottom. A forward edge drops from the bottom of one box to
 * the top of the next. An edge that closes a loop, the thing a tree cannot
 * show, leaves its box on the right, runs up a gutter beside the diagram and
 * comes back in dashed, so the cycle reads as a cycle; its label sits in that
 * gutter, clear of the forward labels. Where one box fans out, each label sits
 * nearer the box it leads to, so neighbouring labels do not collide.
 *
 * The boxes and labels are HTML placed inside the drawing, so their text can be
 * selected and focused, and each is still a block with its own pointer.
 */
function flowDiagram(v: Visual, nodes: FlowNode[], edges: FlowEdge[], lookup: BlockLookup): string {
  const back = backEdges(nodes, edges);
  const rows = layers(nodes, edges, back);
  const loops = edges.filter((e) => back.has(e));
  const backLabel = Math.max(0, ...loops.map((e) => labelWidth(e.label)));
  const gutter = loops.length
    ? Math.min(110, Math.max(28, backLabel + 16)) + 8 * (loops.length - 1)
    : 0;
  const usable = FLOW.w - gutter;
  const nodeH = nodes.some((n) => n.note) ? 56 : 40;
  // A label longer than a pill wraps to two lines, and the gap between layers
  // grows to hold it. A live research card labelled its edges with whole
  // clauses, which on one line ran off both sides of the drawing.
  const pitch = edges.some((e) => (e.label?.length ?? 0) > 24) ? FLOW.pitchLong : FLOW.pitch;

  const placed = new Map<string, Placed>();
  rows.forEach((row, li) => {
    const w = Math.min(FLOW.nodeMax, (usable - (row.length - 1) * FLOW.gap) / row.length);
    const total = row.length * w + (row.length - 1) * FLOW.gap;
    const left = (usable - total) / 2;
    row.forEach((n, i) => {
      placed.set(n.id, {
        index: nodes.indexOf(n),
        layer: li,
        x: left + i * (w + FLOW.gap),
        y: li * (nodeH + pitch),
        w,
      });
    });
  });
  const height = rows.length * nodeH + (rows.length - 1) * pitch;
  const marker = `arrow-${v.id.replace(/[^\w-]/g, '')}`;
  const fanOut = new Map<string, number>();
  for (const e of edges) if (!back.has(e)) fanOut.set(e.from, (fanOut.get(e.from) ?? 0) + 1);

  const paths: string[] = [];
  const labels: string[] = [];
  let loopIndex = 0;
  edges.forEach((e, i) => {
    const a = placed.get(e.from);
    const b = placed.get(e.to);
    if (!a || !b) return;
    let d: string;
    let box: string;
    if (!back.has(e)) {
      const x1 = a.x + a.w / 2;
      const y1 = a.y + nodeH;
      const x2 = b.x + b.w / 2;
      const y2 = b.y - 2;
      const my = (y1 + y2) / 2;
      d = `M${x1},${y1} C${x1},${my} ${x2},${my} ${x2},${y2}`;
      const t = (fanOut.get(e.from) ?? 1) > 1 ? 0.68 : 0.5;
      const lx = cubic(x1, x1, x2, x2, t);
      const ly = cubic(y1, my, my, y2, t);
      box = `x="${lx - FLOW.labelW / 2}" y="${ly - 19}" width="${FLOW.labelW}" height="38"><div class="flab">`;
    } else {
      const gx = usable + 10 + 8 * loopIndex;
      loopIndex += 1;
      const x1 = a.x + a.w;
      const y1 = a.y + nodeH / 2;
      const x2 = b.x + b.w + 2;
      const y2 = b.y + nodeH / 2;
      const r = Math.min(8, Math.abs(y1 - y2) / 2);
      const up = y2 < y1 ? -1 : 1;
      d =
        `M${x1},${y1} H${gx - r} Q${gx},${y1} ${gx},${y1 + up * r} ` +
        `V${y2 - up * r} Q${gx},${y2} ${gx - r},${y2} H${x2}`;
      const mid = (y1 + y2) / 2;
      box = `x="${gx + 4}" y="${mid - 22}" width="${Math.max(24, FLOW.w - gx - 4)}" height="44"><div class="flab side">`;
    }
    paths.push(
      `<path class="fedge${back.has(e) ? ' back' : ''}" d="${d}" marker-end="url(#${marker})"/>`,
    );
    if (e.label) {
      const label = block(
        `/edges/${i}`,
        lookup,
        (attrs) =>
          `<span class="how clk"${attrs} tabindex="0" role="button" title="${esc(e.label ?? '')}">${citeMarkers(
            esc(e.label ?? ''),
          )}</span>`,
      );
      labels.push(`<foreignObject ${box}${label}</div></foreignObject>`);
    }
  });

  const boxes = Array.from(placed.values())
    .map((p) => {
      const n = nodes[p.index];
      const start = p.layer === 0 ? ' start' : '';
      const box = block(
        `/nodes/${p.index}`,
        lookup,
        (attrs) =>
          `<div class="node clk${start}"${attrs} tabindex="0" role="button" title="${esc(
            n.note ? `${n.label}: ${n.note}` : n.label,
          )}">${nodeText(n.label, n.note)}</div>`,
      );
      return `<foreignObject x="${p.x}" y="${p.y}" width="${p.w}" height="${nodeH}">${box}</foreignObject>`;
    })
    .join('');

  return (
    `<svg class="fdiagram" viewBox="-2 -4 ${FLOW.w + 4} ${height + 8}" ` +
    `width="100%" role="group" aria-label="${esc(v.title)}">` +
    `<defs><marker id="${marker}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">` +
    `<path class="fhead" d="M0,0.5 L7.5,4 L0,7.5 z"/></marker></defs>` +
    paths.join('') +
    boxes +
    labels.join('') +
    `</svg>`
  );
}

/**
 * Sanitise a figure svg.
 *
 * This is a second line of defence only. Doc 01 section 4.3.1 requires the
 * harness to sanitise before storage, as its own Step with its own event, so
 * anything reaching here has already been through the allowlist in the core.
 */
export function safeSvg(s: string | undefined): string {
  if (!s || !/^<svg/i.test(s.trim())) return '';
  return s
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/\son\w+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, '')
    .replace(/<(foreignObject|image|a|use)\b[\s\S]*?<\/\1>/gi, '')
    .replace(/<(foreignObject|image|use)\b[^>]*\/?>/gi, '');
}

export function visualHTML(v: Visual | null): string {
  if (!v) return '';
  const lookup = lookupFor(v);
  const head = `<h4>${esc(v.title)}</h4>`;

  switch (v.type) {
    case 'tree': {
      const payload = v.payload as { root: TreeNode };
      if (!payload.root) return '';
      const root = block(
        '/root',
        lookup,
        (attrs) =>
          `<div class="node clk root"${attrs} tabindex="0" role="button">${nodeText(
            payload.root.label,
            payload.root.note,
          )}</div>`,
      );
      const kids = treeBranch(payload.root.children ?? [], '/root/children', 1, lookup);
      return `<div class="vis tree">${head}<div class="t">${root}${kids}</div></div>`;
    }

    case 'table': {
      const payload = v.payload as {
        columns: string[];
        rows: string[][];
        bottom_line?: BottomLine;
      };
      const rowsIn = payload.rows ?? [];
      // A column is numeric when every filled cell in it is a quantity.
      const numeric = (payload.columns ?? []).map((_, ci) => {
        const cells = rowsIn.map((r) => (r[ci] ?? '').trim()).filter(Boolean);
        return ci > 0 && cells.length > 0 && cells.every((c) => NUMERIC.test(c));
      });
      const cols = (payload.columns ?? [])
        .map((c, i) =>
          block(
            `/columns/${i}`,
            lookup,
            (attrs) =>
              `<th class="clk${numeric[i] ? ' num' : ''}"${attrs} scope="col">${esc(c)}</th>`,
          ),
        )
        .join('');
      const rows = rowsIn
        .map(
          (r, ri) =>
            `<tr>${r
              .map((cell, ci) =>
                block(
                  `/rows/${ri}/${ci}`,
                  lookup,
                  (attrs) =>
                    `<td class="clk${numeric[ci] ? ' num' : ''}${ci === 0 ? ' lead' : ''}"${attrs}>${esc(
                      cell,
                    )}</td>`,
                ),
              )
              .join('')}</tr>`,
        )
        .join('');
      return `<div class="vis table">${head}<div class="scroll-x"><table class="cmp"><thead><tr>${cols}</tr></thead><tbody>${rows}</tbody></table></div>${bottomLine(
        payload.bottom_line,
      )}</div>`;
    }

    case 'list': {
      const payload = v.payload as {
        groups: { heading: string; items: { name: string; detail?: string }[] }[];
        bottom_line?: BottomLine;
      };
      const groups = (payload.groups ?? [])
        .map((g, gi) => {
          const heading = block(
            `/groups/${gi}/heading`,
            lookup,
            (attrs) => `<div class="g"${attrs}>${esc(g.heading)}</div>`,
          );
          const items = (g.items ?? [])
            .map((it, ii) =>
              block(
                `/groups/${gi}/items/${ii}`,
                lookup,
                (attrs) =>
                  `<li class="it clk"${attrs} tabindex="0" role="button"><b>${esc(it.name)}</b>${
                    it.detail ? `<span>${esc(it.detail)}</span>` : ''
                  }</li>`,
              ),
            )
            .join('');
          return `<section class="grp">${heading}<ul>${items}</ul></section>`;
        })
        .join('');
      return `<div class="vis list">${head}${groups}${bottomLine(payload.bottom_line)}</div>`;
    }

    case 'steps': {
      const payload = v.payload as { steps: { label: string; note?: string }[] };
      const steps = (payload.steps ?? [])
        .map((s, i) =>
          block(
            `/steps/${i}`,
            lookup,
            (attrs) =>
              `<li class="s"><i aria-hidden="true">${i + 1}</i><div class="clk"${attrs} tabindex="0" role="button"><b>${esc(
                s.label,
              )}</b>${s.note ? `<span>${esc(s.note)}</span>` : ''}</div></li>`,
          ),
        )
        .join('');
      return `<div class="vis">${head}<ol class="steps">${steps}</ol></div>`;
    }

    case 'flow': {
      const payload = v.payload as {
        nodes: FlowNode[];
        edges?: FlowEdge[];
        bottom_line?: BottomLine;
      };
      const nodes = payload.nodes ?? [];
      if (!nodes.length) return '';
      const edges = payload.edges ?? [];
      return `<div class="vis flow">${head}${flowDiagram(v, nodes, edges, lookup)}${bottomLine(
        payload.bottom_line,
      )}</div>`;
    }

    case 'stats': {
      const payload = v.payload as { tiles: Tile[]; bottom_line?: BottomLine };
      const tiles = (payload.tiles ?? [])
        .map((t, i) =>
          block(
            `/tiles/${i}`,
            lookup,
            (attrs) =>
              `<div class="tile clk"${attrs} tabindex="0" role="button"><b>${esc(t.value)}${
                t.unit ? `<i>${esc(t.unit)}</i>` : ''
              }</b><span>${esc(t.label)}</span></div>`,
          ),
        )
        .join('');
      if (!tiles) return '';
      const count = Math.min((payload.tiles ?? []).length, 3);
      return `<div class="vis">${head}<div class="tiles n${count}">${tiles}</div>${bottomLine(
        payload.bottom_line,
      )}</div>`;
    }

    case 'figure': {
      const payload = v.payload as { svg: string; caption?: string };
      const svg = safeSvg(payload.svg);
      if (!svg) return '';
      return `<figure class="vis figure">${head}${svg}<figcaption>${esc(
        payload.caption ?? '',
      )}</figcaption></figure>`;
    }

    case 'image': {
      const payload = v.payload as { image_id: string; caption?: string };
      return `<figure class="vis figure">${head}<img class="gen" data-image-id="${esc(
        payload.image_id,
      )}" alt="${esc(payload.caption ?? v.title)}"/><figcaption>${esc(
        payload.caption ?? '',
      )}</figcaption></figure>`;
    }

    // chart and widget are v1.1. Doc 01 section 9 keeps the schema stubs so the
    // block index and citation binding do not need redesign; nothing renders yet.
    default:
      return '';
  }
}
