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
