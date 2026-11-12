import { expect, it, vi } from 'vitest';
vi.mock('../src/preferences', () => ({ native: false }));
import { attachmentMarkdown } from '../src/attachments';
import { assetPath, assetRelative } from '../src/sync-files';

it('inserts images and named file links with Markdown-safe Unicode paths', () => {
  expect(attachmentMarkdown({ name: '图片 [1].png', path: '.attachments/id/图片 [1].png', image: true })).toBe('![图片 \\[1\\].png](.attachments/id/%E5%9B%BE%E7%89%87%20%5B1%5D.png)');
  expect(attachmentMarkdown({ name: '报告(1).pdf', path: '.attachments/id/报告(1).pdf', image: false })).toBe('[报告(1).pdf](.attachments/id/%E6%8A%A5%E5%91%8A%281%29.pdf)');
});
it('syncs text/Markdown files as attachments only inside the managed resource directory', () => {
  for (const path of ['.attachments/id/说明.txt', 'nested/.attachments/id/example.md']) expect(assetRelative(assetPath(path))).toBe(path);
  for (const path of ['note.md', 'nested/note.txt', '.attachments/../note.md', '.attachments/id/s3-connection.json']) expect(() => assetRelative(assetPath(path))).toThrow();
});
