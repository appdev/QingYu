import { readFile, writeFile } from 'node:fs/promises';
import { expect, it, vi } from 'vitest';
import { Notebook, type Library } from '../src/notebook';
import { syncNotebook } from '../src/sync';
import { setRequestHandler } from '../src/vendor/mdsync/core';
vi.hoisted(() => {
  Object.assign(globalThis, { window: globalThis });
  if (typeof navigator !== 'undefined') Object.defineProperty(navigator, 'onLine', { value: true, configurable: true });
});

// The isolated QA desktop app must have synced the same test prefix with the suffix "-app" first.
it.skipIf(!process.env.MIAOYAN_S3_CONFIG || !process.env.MIAOYAN_S3_DESKTOP)('receives the actual desktop library and publishes a second-client edit', async () => {
  const config = JSON.parse(await readFile(process.env.MIAOYAN_S3_CONFIG!, 'utf8')); config.prefix += '-app';
  setRequestHandler(async request => {
    if (new URL(request.url).origin !== new URL(config.endpoint).origin) throw new Error('Unexpected test origin');
    const response = await fetch(request.url, { method: request.method, headers: request.headers, body: request.body,
      redirect: 'error', signal: AbortSignal.timeout(30000) });
    const arrayBuffer = await response.arrayBuffer();
    return { status: response.status, headers: Object.fromEntries(response.headers), arrayBuffer, text: new TextDecoder().decode(arrayBuffer), json: {} };
  });
  const now = Date.now();
  const initial: Library = { version: 1, activeId: 'desktop-witness', folders: [{ id: 'default', name: '备忘录' }],
    notes: [{ id: 'desktop-witness', folderId: 'default', createdAt: now, updatedAt: now,
      source: '# S3 双端同步验证\n\n此笔记由独立客户端写入，用于验证桌面应用接收新笔记。' }] };
  let value = JSON.stringify(initial);
  const book = new Notebook({ getItem: () => value, setItem: (_key, next) => { value = next; } }, '');
  await syncNotebook(book, config);
  const guide = book.data.notes.find(note => note.id !== 'desktop-witness' && note.source.startsWith('# 妙言 Markdown 语法指南'));
  expect(guide).toBeDefined(); expect(book.data.notes).toHaveLength(2);
  book.select(guide!.id);
  if (!book.current.source.includes('来自第二客户端的同步验证')) book.save(book.current.source + '\n\n来自第二客户端的同步验证。\n');
  await syncNotebook(book, config);
  await writeFile('artifacts/mdsync-spike/desktop-witness.json', JSON.stringify({ success: true, notes: book.data.notes.length, publishedMarker: true }, null, 2) + '\n');
}, 60000);
