import { afterEach, describe, expect, it, vi } from 'vitest';

import { Rpc, RpcError } from './rpc.js';

describe('the browser transport', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('posts the JSON-RPC body to the page origin when there is no Tauri', async () => {
    const fetchMock = vi.fn(() =>
      Promise.resolve(
        new Response(JSON.stringify({ jsonrpc: '2.0', id: 1, result: { boards: [] } })),
      ),
    );
    vi.stubGlobal('fetch', fetchMock);

    const rpc = new Rpc();
    expect(rpc.connected).toBe(true);
    await expect(rpc.listBoards()).resolves.toEqual({ boards: [] });

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/rpc');
    expect(init.method).toBe('POST');
    expect((init.headers as Record<string, string>)['content-type']).toBe('application/json');
    expect(JSON.parse(init.body as string)).toMatchObject({ method: 'board.list' });
  });

  it('turns a refused request into an error the page can show', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() => Promise.resolve(new Response('Not from this page.', { status: 403 }))),
    );
    await expect(new Rpc().listBoards()).rejects.toBeInstanceOf(RpcError);
  });
});
