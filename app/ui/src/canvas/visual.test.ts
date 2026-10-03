/**
 * The visual renderers keep one contract the rest of the UI leans on: every
 * block carries its JSON pointer in `data-ref`. These cases also pin the parts
 * of the redesign a screenshot cannot: a tree keeps each child under its own
 * parent, a flow finds the edge that closes a loop, and a table aligns only
 * the columns that hold quantities.
 */

import { describe, expect, it } from 'vitest';

import type { Visual, VisualPayload, VisualType } from './types.js';
import { visualHTML } from './visual.js';

function render(type: VisualType, payload: VisualPayload): HTMLElement {
  const v: Visual = { id: 'v1', type, title: 'Title', payload, block_index: [] };
  const host = document.createElement('div');
  host.innerHTML = visualHTML(v);
  return host;
}

describe('visualHTML', () => {
  it('nests a tree so each child sits under its own parent', () => {
    const host = render('tree', {
      root: {
        label: 'Root',
        children: [{ label: 'A', note: 'first', children: [{ label: 'A1' }] }, { label: 'B' }],
      },
    });
    const a1 = host.querySelector('[data-ref="/root/children/0/children/0"]');
    expect(a1?.textContent).toBe('A1');
    // A1's list item sits inside A's, which is the parentage a row of chips lost.
    const aItem = host.querySelector('[data-ref="/root/children/0"]')?.closest('li');
    expect(aItem?.contains(a1 as Node)).toBe(true);
    expect(host.querySelector('[data-ref="/root/children/0"] span')?.textContent).toBe('first');
  });

  it('draws a flow with the edge that closes a loop dashed', () => {
    const host = render('flow', {
      nodes: [
        { id: 'a', label: 'Draft' },
        { id: 'b', label: 'Review' },
        { id: 'c', label: 'Accepted' },
      ],
      edges: [
        { from: 'a', to: 'b', label: 'goes to' },
        { from: 'b', to: 'a', label: 'returns to' },
        { from: 'b', to: 'c' },
      ],
    });
    expect(host.querySelectorAll('.fedge')).toHaveLength(3);
    expect(host.querySelectorAll('.fedge.back')).toHaveLength(1);
    expect(host.querySelector('[data-ref="/edges/1"]')?.textContent).toBe('returns to');
    // An unlabelled edge has a line and no label block.
    expect(host.querySelector('[data-ref="/edges/2"]')).toBeNull();
    // Each layer is lower than the one before it.
    const y = (ref: string) =>
      Number(
        host.querySelector(`[data-ref="${ref}"]`)?.closest('foreignObject')?.getAttribute('y'),
      );
    expect(y('/nodes/1')).toBeGreaterThan(y('/nodes/0'));
    expect(y('/nodes/2')).toBeGreaterThan(y('/nodes/1'));
  });

  it('gives each flow its own arrowhead, so two on one board do not share one', () => {
    const host = render('flow', { nodes: [{ id: 'a', label: 'A' }], edges: [] });
    expect(host.querySelector('marker')?.id).toBe('arrow-v1');
  });

  it('aligns only the columns that hold quantities', () => {
    const host = render('table', {
      columns: ['Name', 'Share', 'Note'],
      rows: [
        ['Alpha', '12%', 'early'],
        ['Beta', '1,204', 'late'],
      ],
    });
    const cells = host.querySelectorAll('tbody tr:first-child td');
    expect(cells[0]?.classList.contains('num')).toBe(false);
    expect(cells[1]?.classList.contains('num')).toBe(true);
    expect(cells[2]?.classList.contains('num')).toBe(false);
  });

  it('keeps a tile unit beside its numeral', () => {
    const host = render('stats', { tiles: [{ value: '120', unit: 'm', label: 'Floor space' }] });
    expect(host.querySelector('.tile b i')?.textContent).toBe('m');
    expect(host.querySelector('.tile span')?.textContent).toBe('Floor space');
    expect(host.querySelector('.tiles')?.classList.contains('n1')).toBe(true);
  });
});
