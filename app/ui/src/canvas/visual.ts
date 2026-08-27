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
import type { BlockIndexEntry, BottomLine, TreeNode, Visual } from './types.js';

const HUES = ['h1', 'h2', 'h3', 'h4'] as const;

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
function block(
  ref: string,
  lookup: BlockLookup,
  render: (attrs: string) => string,
): string {
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
  return `<div class="bottom"><b>${esc(bl.head)}</b>${citeMarkers(esc(bl.text))}</div>`;
}

function treeLevel(nodes: TreeNode[], depth: number, path: string, lookup: BlockLookup): string {
  if (nodes.length === 0) return '';
  const row = `<div class="row">${nodes
    .map((n, i) =>
      block(`${path}/${i}`, lookup, (attrs) => {
        const hue = HUES[(depth + i) % HUES.length];
        const note = n.note ? ` data-note="${esc(n.note)}"` : '';
        return `<span class="node clk ${hue}"${attrs}${note} tabindex="0" role="button">${esc(n.label)}</span>`;
      }),
    )
    .join('')}</div>`;

  // Children render as one row per level, matching the prototype's shape.
  const kids: { node: TreeNode; path: string }[] = [];
  nodes.forEach((n, i) => {
    (n.children ?? []).forEach((c, j) => kids.push({ node: c, path: `${path}/${i}/children/${j}` }));
  });
  if (kids.length === 0) return row;

  // Children of a level share a row; each keeps its own pointer.
  const childRow = `<div class="row">${kids
    .map(({ node, path: p }, i) =>
      block(p, lookup, (attrs) => {
        const hue = HUES[(depth + 1 + i) % HUES.length];
        const note = node.note ? ` data-note="${esc(node.note)}"` : '';
        return `<span class="node clk ${hue}"${attrs}${note} tabindex="0" role="button">${esc(node.label)}</span>`;
      }),
    )
    .join('')}</div>`;

  const grandKids = kids.some(({ node }) => (node.children ?? []).length > 0);
  const deeper = grandKids
    ? `<div class="arrow" aria-hidden="true">↓</div>` +
      treeLevel(
        kids.flatMap(({ node }) => node.children ?? []),
        depth + 2,
        `${path}/children`,
        lookup,
      )
    : '';

  return `${row}<div class="arrow" aria-hidden="true">↓</div>${childRow}${deeper}`;
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
      const root = block('/root', lookup, (attrs) => {
        const note = payload.root.note ? ` data-note="${esc(payload.root.note)}"` : '';
        return `<span class="node clk h0"${attrs}${note} tabindex="0" role="button">${esc(payload.root.label)}</span>`;
      });
      const children = payload.root.children ?? [];
      const body = children.length
        ? `<div class="arrow" aria-hidden="true">↓</div>${treeLevel(children, 0, '/root/children', lookup)}`
        : '';
      return `<div class="vis tree">${head}${root}${body}</div>`;
    }

    case 'table': {
      const payload = v.payload as { columns: string[]; rows: string[][]; bottom_line?: BottomLine };
      const cols = (payload.columns ?? [])
        .map((c, i) =>
          block(`/columns/${i}`, lookup, (attrs) => {
            const hue = i ? 'h1' : 'h2';
            return `<th class="clk ${hue}"${attrs} scope="col">${esc(c)}</th>`;
          }),
        )
        .join('');
      const rows = (payload.rows ?? [])
        .map(
          (r, ri) =>
            `<tr>${r
              .map((cell, ci) =>
                block(`/rows/${ri}/${ci}`, lookup, (attrs) => `<td class="clk"${attrs}>${esc(cell)}</td>`),
              )
              .join('')}</tr>`,
        )
        .join('');
      return `<div class="vis"><div class="scroll-x">${head}<table class="cmp"><thead><tr>${cols}</tr></thead><tbody>${rows}</tbody></table></div>${bottomLine(
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
                  `<div class="it clk"${attrs} tabindex="0" role="button"><b>${esc(it.name)}</b><span>${esc(
                    it.detail ?? '',
                  )}</span></div>`,
              ),
            )
            .join('');
          return heading + items;
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
              `<div class="s"><i aria-hidden="true">${i + 1}</i><div class="clk ${
                HUES[i % HUES.length]
              }"${attrs} tabindex="0" role="button"><b>${esc(s.label)}</b><span>${esc(
                s.note ?? '',
              )}</span></div></div>`,
          ),
        )
        .join('');
      return `<div class="vis">${head}<div class="steps">${steps}</div></div>`;
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
