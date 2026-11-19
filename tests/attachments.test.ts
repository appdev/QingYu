import { expect, it, vi } from 'vitest';
vi.mock('../src/preferences', () => ({ native: false }));
import { attachmentMarkdown, attachmentPlaceholder } from '../src/attachments';
import { Notebook } from '../src/notebook';
import { assetPath, assetRelative } from '../src/sync-files';

it('inserts images and named file links with Markdown-safe Unicode paths', () => {
  expect(attachmentMarkdown({ name: '图片 [1].png', path: '.attachments/id/图片 [1].png', image: true })).toBe('![图片 \\[1\\].png](.attachments/id/%E5%9B%BE%E7%89%87%20%5B1%5D.png)');
  expect(attachmentMarkdown({ name: '报告(1).pdf', path: '.attachments/id/报告(1).pdf', image: false })).toBe('[报告(1).pdf](.attachments/id/%E6%8A%A5%E5%91%8A%281%29.pdf)');
});
it('syncs text/Markdown files as attachments only inside the managed resource directory', () => {
  for (const path of ['.attachments/id/说明.txt', 'nested/.attachments/id/example.md']) expect(assetRelative(assetPath(path))).toBe(path);
  for (const path of ['note.md', 'nested/note.txt', '.attachments/../note.md', '.attachments/id/s3-connection.json']) expect(() => assetRelative(assetPath(path))).toThrow();
});

it('resolves a note-relative pending attachment and shares one lookup per library revision', async () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '');
  book.current.path = '项目/说明.md';
  book.data.sync = { target: 'test', version: 2, placeholderCount: 1, state: {
    deviceId: 'test', vaultId: 'test', baseline: null, hashCache: {},
  } };
  vi.spyOn(book, 'root', 'get').mockReturnValue('/test/library');
  const entry = { hash: 'a'.repeat(64), size: 2_000_000, mtime: 1 };
  const read = vi.spyOn(book, 'readAssetPlaceholders').mockResolvedValue({ '项目/.attachments/报告.pdf': entry });
  await expect(attachmentPlaceholder(book, book.current.id, '.attachments/%E6%8A%A5%E5%91%8A.pdf?x=1')).resolves.toEqual({
    root: '/test/library', path: '项目/.attachments/报告.pdf', ...entry,
  });
  await expect(attachmentPlaceholder(book, book.current.id, 'https://fixture.invalid/image.jpg')).resolves.toBeUndefined();
  await expect(attachmentPlaceholder(book, book.current.id, '../.miaoyan/asset-placeholders.json')).resolves.toBeUndefined();
  await expect(attachmentPlaceholder(book, book.current.id, 'other.pdf')).resolves.toBeUndefined();
  expect(read).toHaveBeenCalledTimes(1);
});
