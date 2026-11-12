// @vitest-environment jsdom
import { expect, it } from 'vitest';
import { originalAssetPaths, projectServerAssets } from '../src/server-assets';

it('resolves relative images and attachments without changing serialized Markdown or external URLs', () => {
  const root=document.createElement('div');root.innerHTML='<img src="../images/图.svg"><a href="files/a.zip">下载</a><a href="#标题">标题</a><img src="https://example.com/a.png"><img class="encoded" src="images/a%20b.svg?cache=1#view">';
  projectServerAssets(root,'note-one');
  expect(root.querySelector('img')!.getAttribute('src')).toContain('/api/web/assets/note-one?path=');
  expect(originalAssetPaths(root,()=>root.innerHTML)).toContain('src="../images/图.svg"');
  expect(root.querySelector('a[href="#标题"]')).not.toBeNull();
  expect(root.querySelector('img[src="https://example.com/a.png"]')).not.toBeNull();
  expect(new URL(root.querySelector('.encoded')!.getAttribute('src')!, 'http://test').searchParams.get('path')).toBe('images/a b.svg');
  expect(()=>originalAssetPaths(root,()=>{throw new Error('serialization failed');})).toThrow();
  expect(root.querySelector('img')!.getAttribute('src')).toContain('note-one');
  root.innerHTML = root.innerHTML; // Vditor recreates nodes during undo and block conversion.
  expect(originalAssetPaths(root,()=>root.innerHTML)).toContain('src="../images/图.svg"');
  projectServerAssets(root,'note-two');expect(root.querySelector('img')!.getAttribute('src')).toContain('note-two');
});

it('projects native resources across refreshes and restores original links after undo', () => {
  const root = document.createElement('div');
  root.innerHTML = '<img src=".attachments/id/图片%20一.png"><a href=".attachments/id/报告.pdf">报告.pdf</a><img src="/i/legacy.png">';
  projectServerAssets(root, 'note', 'qingyu-asset://localhost/note?v=1');
  expect(root.querySelector('img')!.getAttribute('src')).toBe('qingyu-asset://localhost/note?v=1&path=.attachments%2Fid%2F%E5%9B%BE%E7%89%87+%E4%B8%80.png');
  projectServerAssets(root, 'note', 'qingyu-asset://localhost/note?v=2');
  expect(root.querySelector('img')!.getAttribute('src')).toContain('?v=2&');
  root.innerHTML = root.innerHTML;
  const restored = originalAssetPaths(root, () => root.innerHTML);
  expect(restored).toContain('src=".attachments/id/图片%20一.png"');
  expect(restored).toContain('href=".attachments/id/报告.pdf"');
  expect(restored).toContain('src="/i/legacy.png"');
  expect(restored).not.toContain('qingyu-asset:');
});
