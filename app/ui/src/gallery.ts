/**
 * A dev only gallery of every visual type and a threaded tile, rendered by the
 * real `tileHTML` with the real stylesheets, so a design change can be looked
 * at in both themes without a running core. `pnpm dev`, then /gallery.html,
 * with `?theme=dark` for the dark theme.
 */

import './styles/fonts.css';
import './styles/tokens.css';
import './styles/components.css';
import './styles/board.css';
import './styles/chrome.css';
import './styles/pages.css';

import { tileHTML } from './canvas/render.js';
import type { Card, Visual, VisualPayload, VisualType } from './canvas/types.js';

const theme = new URLSearchParams(location.search).get('theme');
if (theme === 'dark' || theme === 'light') document.documentElement.dataset.theme = theme;

let next = 0;

function visual(type: VisualType, title: string, payload: VisualPayload): Visual {
  return { id: `v${(next += 1)}`, type, title, payload, block_index: [] };
}

function card(over: Partial<Card>): Card {
  next += 1;
  return {
    id: `c${next}`,
    parent_card_id: null,
    kind: 'root',
    anchor_text: null,
    anchor_block_ref: null,
    question: 'What is a world model?',
    depth: 'fast',
    audience_id: null,
    answer:
      'A world model is a learned simulator of an environment that predicts what happens next from what is seen now [1]. Agents plan inside it before acting [2].',
    findings: [],
    visual: null,
    citations: [
      {
        ordinal: 1,
        source_title: 'World Models, Ha and Schmidhuber',
        source_class: 'paper',
        locator: 'section 2',
        verdict: 'supported',
      },
      {
        ordinal: 2,
        source_title: 'Mastering Atari with discrete world models',
        source_class: 'paper',
        locator: 'p. 3',
        verdict: 'weak',
      },
    ],
    flags: [],
    status: 'done',
    confidence: 0.82,
    model_alias: 'sonnet',
    stages: [],
    position: { x: 0, y: 0, dx: 0, dy: 0, pinned: false },
    ...over,
  };
}

const samples: { name: string; tile: Card; turns?: Card[] }[] = [];

samples.push({
  name: 'Tree',
  tile: card({
    findings: [
      {
        text: 'Perception compresses each frame into a small latent code [1].',
        citation_ordinals: [1],
      },
      {
        text: 'The dynamics model predicts the next latent from the current one [2].',
        citation_ordinals: [2],
      },
    ],
    visual: visual('tree', 'Parts of a world model', {
      root: {
        label: 'World model',
        note: 'Learned simulator',
        children: [
          {
            label: 'Perception',
            note: 'Frames to a latent code',
            children: [
              { label: 'Encoder', note: 'Variational autoencoder' },
              { label: 'Latent state' },
            ],
          },
          {
            label: 'Dynamics predictor',
            note: 'What happens next',
            children: [{ label: 'Recurrent core', note: 'Mixture density output' }],
          },
          { label: 'Controller', note: 'Acts on the latent, not the pixels' },
        ],
      },
    }),
  }),
});

samples.push({
  name: 'Flow',
  tile: card({
    question: 'How does a paper get through review?',
    answer: 'A draft goes to review, which either accepts it or returns it for revision [1].',
    visual: visual('flow', 'Review cycle', {
      nodes: [
        { id: 'a', label: 'Draft', note: 'Author writes' },
        { id: 'b', label: 'Review', note: 'Two referees' },
        { id: 'c', label: 'Revise', note: 'Answer the referees' },
        { id: 'd', label: 'Accepted', note: 'Camera ready' },
      ],
      edges: [
        { from: 'a', to: 'b', label: 'goes to' },
        { from: 'b', to: 'd', label: 'accepts' },
        { from: 'b', to: 'c', label: 'asks for changes' },
        { from: 'c', to: 'b', label: 'returns to' },
      ],
      bottom_line: {
        head: 'Bottom line',
        text: 'Most papers loop through review at least once [1].',
      },
    }),
  }),
});

// The edge labels a live research card wrote: whole clauses with citation
// markers, which on one line ran off both sides of the drawing.
samples.push({
  name: 'Flow with long labels',
  tile: card({
    question: 'How does a card move from the router to the verifier?',
    depth: 'research',
    answer: 'Each stage hands a fixed artefact to the next [1].',
    visual: visual('flow', 'How it connects', {
      nodes: [
        { id: 'h', label: 'Harness' },
        { id: 'r', label: 'Router' },
        { id: 'p', label: 'Planner' },
        { id: 't', label: 'Retrievers' },
        { id: 's', label: 'Synthesizer' },
        { id: 'v', label: 'Verifier' },
      ],
      edges: [
        {
          from: 'h',
          to: 'r',
          label: 'invokes on card.requested.v1, card.rerun.v1 or read.completed.v1 [3]',
        },
        {
          from: 'r',
          to: 'p',
          label: 'resolved policy and reused classification start the pipeline [3][1]',
        },
        {
          from: 'p',
          to: 't',
          label: 'sub-questions with retriever, parameters, constraints and scope [7]',
        },
        { from: 't', to: 's', label: 'retrieved passages [1]' },
        { from: 's', to: 'v', label: 'card with visual for checking [1]' },
        { from: 'v', to: 's', label: 'own_card sources that trigger own_card_sole_support [6]' },
      ],
    }),
  }),
});

samples.push({
  name: 'Stats',
  tile: card({
    question: 'How big was the first shop?',
    answer: 'The first shop opened in 1949 with a small floor [1].',
    visual: visual('stats', 'The first shop', {
      tiles: [
        { value: '1949', label: 'Year it opened' },
        { value: '120', unit: 'm²', label: 'Floor space' },
        { value: '3', label: 'Staff on day one' },
      ],
      bottom_line: { head: 'Bottom line', text: 'It started small and grew by reinvesting [2].' },
    }),
  }),
});

samples.push({
  name: 'Table',
  tile: card({
    question: 'How do the three model families compare?',
    answer: 'They trade context length against cost [1].',
    visual: visual('table', 'Model families compared', {
      columns: ['Family', 'Context', 'Cost per M', 'Best at'],
      rows: [
        ['Haiku', '200k', '$1.00', 'Fast routing'],
        ['Sonnet', '1,000k', '$3.00', 'Everyday work'],
        ['Opus', '1,000k', '$15.00', 'Deep reasoning'],
      ],
      bottom_line: { head: 'Bottom line', text: 'Route by need, not by default [2].' },
    }),
  }),
});

samples.push({
  name: 'List',
  tile: card({
    question: 'What should I read first?',
    answer: 'Start with the survey, then the two founding papers [1].',
    visual: visual('list', 'Reading list', {
      groups: [
        {
          heading: 'Start here',
          items: [
            { name: 'World Models', detail: 'The original paper, short and visual' },
            { name: 'Dreamer', detail: 'Planning in latent space' },
          ],
        },
        {
          heading: 'Then',
          items: [
            { name: 'MuZero', detail: 'A model that predicts only what matters for planning' },
          ],
        },
      ],
    }),
  }),
});

samples.push({
  name: 'Steps',
  tile: card({
    question: 'How do I train one?',
    answer: 'Training runs in three stages [1].',
    visual: visual('steps', 'Training a world model', {
      steps: [
        { label: 'Collect rollouts', note: 'Random actions in the environment' },
        { label: 'Train the encoder', note: 'Compress frames to latents' },
        { label: 'Train the dynamics', note: 'Predict the next latent' },
        { label: 'Train the controller', note: 'Inside the dream' },
      ],
    }),
  }),
});

const head = card({ question: 'What is a world model?' });
samples.push({
  name: 'Thread',
  tile: head,
  turns: [
    card({
      kind: 'follow',
      parent_card_id: head.id,
      question: 'Who came up with the idea?',
      answer:
        'The term goes back to Craik in 1943; Ha and Schmidhuber made it concrete in 2018 [1].',
      citations: [],
    }),
    card({
      kind: 'follow',
      parent_card_id: head.id,
      question: 'Is it used in robots today?',
      status: 'running',
      answer: null,
      stages: [
        { label: 'Routing', done: true },
        { label: 'Searching', done: false },
      ],
    }),
  ],
});

const root = document.getElementById('gallery') as HTMLElement;
root.innerHTML = samples
  .map(
    (s) =>
      `<section class="g-cell"><h2>${s.name}</h2><div class="card" data-card-id="${s.tile.id}" data-status="${s.tile.status}">${tileHTML(
        s.tile,
        s.turns ?? [],
      )}</div></section>`,
  )
  .join('');

const style = document.createElement('style');
style.textContent = `
  body { background: var(--bg); color: var(--ink); margin: 0; font-family: 'IBM Plex Sans', system-ui, sans-serif; }
  #gallery { display: flex; flex-wrap: wrap; gap: 24px; padding: 24px; align-items: flex-start; }
  .g-cell h2 { font-size: 12px; color: var(--ink-2); margin: 0 0 8px; }
  .g-cell .card { position: relative; }
`;
document.head.appendChild(style);
