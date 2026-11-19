// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { EditorView } from '@codemirror/view';
import { createRichEditor } from '../src/rich-editor';
import { mountPendingImage } from '../src/asset-download-view';
import { openMediaViewer } from '../src/vendor/markra/editor/codemirror/media-viewer';
import { mountNotePicture } from '../src/note-picture';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} })); });
vi.mock('glassy-liquid', async importOriginal => ({
  ...await importOriginal<typeof import('glassy-liquid')>(),
  glass: vi.fn(),
}));
afterEach(() => { document.body.replaceChildren(); });

const entry = { root: '/notes', path: 'b.webp', size: 2_300_000, hash: 'a'.repeat(64), mtime: 1 };
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };

it('waits for visibility, shows failure, retries, and reloads at the downloaded hash', async () => {
  const observe = vi.fn(), disconnect = vi.fn();
  let notify!: IntersectionObserverCallback;
  const original = globalThis.IntersectionObserver;
  globalThis.IntersectionObserver = class {
    observe = observe;
    disconnect = disconnect;
    constructor(callback: IntersectionObserverCallback) { notify = callback; }
  } as unknown as typeof IntersectionObserver;
  const host = document.body.appendChild(document.createElement('span'));
  const image = host.appendChild(document.createElement('img'));
  image.width = 160; image.height = 90;
  const run = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(entry.hash);
  const cleanup = mountPendingImage(image, host, '/asset?path=b.webp&v=3', Promise.resolve(entry), run);
  try {
    await flush();
    const button = host.querySelector('button')!;
    expect(observe).toHaveBeenCalledWith(button);
    expect(button.textContent).toBe('2.3 MB');
    expect(button.style.aspectRatio).toBe('160 / 90');
    expect(image.hidden).toBe(true);
    expect(run).not.toHaveBeenCalled();
    notify([{ isIntersecting: true }] as IntersectionObserverEntry[], {} as IntersectionObserver);
    expect(button.ariaBusy).toBe('true');
    await flush();
    expect(button.textContent).toBe('点按重试');
    button.click();
    await flush();
    expect(run).toHaveBeenCalledTimes(2);
    expect(image.hidden).toBe(false);
    expect(new URL(image.src).searchParams.get('v')).toBe(entry.hash);
    expect(host.querySelector('button')).toBeNull();
  } finally { cleanup(); globalThis.IntersectionObserver = original; }
});

it('keeps a list thumbnail as a placeholder without downloading the original', async () => {
  const observe = vi.fn();
  const original = globalThis.IntersectionObserver;
  globalThis.IntersectionObserver = class { observe = observe; disconnect() {} } as unknown as typeof IntersectionObserver;
  const host = document.body.appendChild(document.createElement('span'));
  const image = host.appendChild(document.createElement('img'));
  const run = vi.fn();
  const cleanup = mountPendingImage(image, host, '/asset?path=b.webp', Promise.resolve(entry), run, true);
  try {
    await flush();
    const placeholder = host.querySelector<HTMLElement>('.asset-image-placeholder.note-row-thumbnail')!;
    expect(placeholder.tagName).toBe('SPAN');
    expect(placeholder.getAttribute('aria-label')).toBe('图片尚未下载 · 2.3 MB');
    expect(observe).not.toHaveBeenCalled();
    placeholder.click();
    await flush();
    expect(run).not.toHaveBeenCalled();
    expect(image.hidden).toBe(true);
  } finally { cleanup(); globalThis.IntersectionObserver = original; }
});

it('mounts pending images in the editor and while stepping through the viewer', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const downloads: ((hash: string) => void)[] = [];
  const run = vi.fn(() => new Promise<string>(resolve => { downloads.push(resolve); }));
  const mountImage = (image: HTMLImageElement, frame: HTMLElement, url: string, source: string) =>
    source === 'b.webp' ? mountPendingImage(image, frame, url, Promise.resolve(entry), run) : undefined;
  const editor = createRichEditor(root, () => {}, {
    mountAssetImage(_id, ...args) { return mountImage(...args); },
  });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('![一](a.webp)\n\n![二](b.webp)', 'assets', 'note', '/asset');
    await flush();
    expect(root.querySelector('.markra-image-frame .asset-image-placeholder')).not.toBeNull();
    const viewer = openMediaViewer({
      gallery: { index: 0, labels: { next: '下一张', previous: '上一张' }, items: [
        { alt: '一', source: '/asset?path=a.webp', original: 'a.webp', title: '' },
        { alt: '二', source: '/asset?path=b.webp', original: 'b.webp', title: '' },
      ] },
      media: root.querySelector('img')!, mount: document.body, mountImage,
      labels: { close: '关闭', dialog: '查看图片', enterFullscreen: '全屏', exitFullscreen: '退出全屏',
        reset: '还原大小', viewport: '图片', zoomIn: '放大', zoomOut: '缩小' },
    });
    const dialog = document.querySelector('[role=dialog]')!;
    dialog.querySelector<HTMLButtonElement>('[aria-label="下一张"]')!.click();
    await flush();
    expect(dialog.querySelector('.asset-image-placeholder')?.textContent).toBe('2.3 MB');
    for (const finish of downloads) finish(entry.hash);
    await flush();
    expect(dialog.querySelector('.asset-image-placeholder')).toBeNull();
    expect(new URL(dialog.querySelector('img')!.src).searchParams.get('v')).toBe(entry.hash);
    expect(view.state.doc.toString()).toBe('![一](a.webp)\n\n![二](b.webp)');
    viewer.close();
  } finally { view.destroy(); }
});

it('shows download size on an attachment card and keeps it retryable when opening fails', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const open = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValueOnce(undefined);
  const editor = createRichEditor(root, () => {}, { openAsset: open, pendingAssetSize: async () => entry.size });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('![文件](files/report.pdf)', 'assets', 'note', '/asset');
    await flush();
    const card = root.querySelector<HTMLAnchorElement>('.qingyu-link-card')!;
    expect(card.querySelector('.link-card-download')?.textContent).toBe('下载 · 2.3 MB');
    card.click();
    await vi.waitFor(() => expect(card.querySelector('.link-card-download')?.textContent).toBe('点按重试'));
    card.click();
    await vi.waitFor(() => expect(card.querySelector('.link-card-download')).toBeNull());
    expect(open).toHaveBeenCalledWith('note', 'files/report.pdf');
    expect(open).toHaveBeenCalledTimes(2);
  } finally { view.destroy(); }
});

it('keeps picture export disabled while a required attachment cannot download', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const image = vi.fn();
  const prepareImage = vi.fn(async () => { throw new Error('offline'); });
  const stop = mountNotePicture(root, { title: '图片', source: '![图](files/image.webp)', image, prepareImage }, true);
  try {
    await vi.waitFor(() => expect(root.querySelector('[role=status]')?.textContent).toContain('无法生成图片'));
    expect(prepareImage).toHaveBeenCalledWith('files/image.webp');
    expect(image).not.toHaveBeenCalled();
    expect([...root.querySelectorAll<HTMLButtonElement>('[data-picture-action]')].every(button => button.disabled)).toBe(true);
  } finally { stop(); }
});
