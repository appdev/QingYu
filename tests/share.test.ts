// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { showShare } from '../src/reference-panels';
import { copyShareLink } from '../src/mock-web-share';
import type { NoteShare } from '../src/notebook';

const platform = vi.hoisted(() => ({ native: false }));
vi.mock('../src/preferences', () => ({ get native() { return platform.native; } }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const show = vi.fn(), hide = vi.fn(), report = vi.fn();
beforeEach(() => {
  platform.native = false;
  document.documentElement.dataset.platform = 'other';
  document.documentElement.removeAttribute('data-mobile');
  vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  Object.defineProperty(HTMLElement.prototype, 'showPopover', { configurable: true, value: show });
  Object.defineProperty(HTMLElement.prototype, 'hidePopover', { configurable: true, value: hide });
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.open = true; } });
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value() { this.open = false; this.dispatchEvent(new Event('close')); } });
});
afterEach(() => {
  document.body.replaceChildren();
  delete (HTMLElement.prototype as Partial<HTMLElement>).showPopover;
  delete (HTMLElement.prototype as Partial<HTMLElement>).hidePopover;
  delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).showModal;
  delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).close;
  vi.useRealTimers();
  vi.unstubAllGlobals(); vi.resetAllMocks();
});
function open(noteKey: string = crypto.randomUUID(), title = '测试标题', text = '测试标题\n\n当前副本', store?: { get(): NoteShare | undefined; set(share: NoteShare): Promise<void> }) {
  const anchor = document.body.appendChild(document.createElement('button'));
  showShare(anchor, title, text, report, undefined, noteKey, store);
  return document.querySelector<HTMLElement>('.share-popover')!;
}

it('shares the current title and full text through the browser and closes after handoff', async () => {
  const share = vi.fn().mockResolvedValue(undefined);
  Object.assign(navigator, { share });
  const panel = open();
  expect(panel.textContent).not.toMatch(/邮件|信息|备忘录|无边记|手记|提醒事项|灰色操作/);
  panel.querySelector<HTMLButtonElement>('.share-send')!.click();
  expect(share).toHaveBeenCalledWith({ title: '测试标题', text: '测试标题\n\n当前副本' });
  await vi.waitFor(() => expect(hide).toHaveBeenCalledOnce());
  expect(invoke).not.toHaveBeenCalled();
});

it.each(['macos', 'android', 'ios'])('uses the native share bridge on %s', async os => {
  platform.native = true;
  document.documentElement.dataset.platform = os === 'macos' ? 'macos' : 'other';
  document.documentElement.toggleAttribute('data-mobile', os !== 'macos');
  vi.mocked(invoke).mockResolvedValue(undefined);
  const panel = open();
  panel.querySelector<HTMLButtonElement>('.share-send')!.click();
  expect(invoke).toHaveBeenCalledWith('note_share_text', {
    title: '测试标题', text: '测试标题\n\n当前副本', anchor: { x: 0, y: 0, width: 0, height: 0 },
  });
  await vi.waitFor(() => expect(hide).toHaveBeenCalledOnce());
});

it('keeps copying available without system sharing support', async () => {
  const panel = open();
  expect(panel.querySelector('.share-send')).toBeNull();
  expect(panel.textContent).toContain('当前环境不支持系统分享');
  panel.querySelector<HTMLButtonElement>('.share-copy')!.click();
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith('测试标题\n\n当前副本');
  await vi.waitFor(() => expect(hide).toHaveBeenCalledOnce());
});

it('ignores cancellation, reports failures and allows retry without duplicate requests', async () => {
  let reject!: (reason: unknown) => void;
  const share = vi.fn().mockImplementationOnce(() => new Promise((_, fail) => { reject = fail; }))
    .mockRejectedValueOnce(new DOMException('拒绝', 'NotAllowedError')).mockResolvedValue(undefined);
  Object.assign(navigator, { share });
  const panel = open(), send = panel.querySelector<HTMLButtonElement>('.share-send')!;
  send.click(); send.click();
  expect(share).toHaveBeenCalledOnce();
  reject(new DOMException('取消', 'AbortError'));
  await vi.waitFor(() => expect(send.disabled).toBe(false));
  expect(report).not.toHaveBeenCalled(); expect(hide).not.toHaveBeenCalled();
  send.click();
  await vi.waitFor(() => expect(report).toHaveBeenCalledWith('无法打开系统分享，请重试或复制为纯文本'));
  expect(send.disabled).toBe(false);
  send.click();
  await vi.waitFor(() => expect(hide).toHaveBeenCalledOnce());
});

it('reports clipboard failure and leaves the copy available', async () => {
  vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error('denied'));
  open().querySelector<HTMLButtonElement>('.share-copy')!.click();
  await vi.waitFor(() => expect(report).toHaveBeenCalledWith('无法复制，请检查剪贴板权限'));
  expect(hide).not.toHaveBeenCalled();
});

function web(panel: HTMLElement) {
  [...panel.querySelectorAll<HTMLButtonElement>('[role=tab]')].find(tab => tab.textContent === '网页')!.click();
  return panel;
}
function click(panel: HTMLElement, label: string) {
  [...panel.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === label)!.click();
}

function createLink(panel: HTMLElement) {
  click(panel, '发布网页…');
  click(document.querySelector<HTMLElement>('.share-web-confirm')!, '创建链接');
}

it('confirms before publishing, closes the panel and automatically copies the created link', async () => {
  vi.useFakeTimers();
  vi.stubGlobal('fetch', vi.fn());
  const key = crypto.randomUUID();
  let panel = web(open(key));
  expect(panel.textContent).toContain('模拟测试');
  click(panel, '发布网页…');
  const confirm = document.querySelector<HTMLDialogElement>('.share-web-confirm')!;
  expect(confirm.open).toBe(true);
  expect(confirm.querySelector('h2')!.textContent).toBe('公开分享「测试标题」？');
  expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  click(confirm, '创建链接');
  expect(confirm.isConnected).toBe(false);
  expect(hide).toHaveBeenCalled();
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('正在创建链接…');
  expect(document.querySelector('.share-web-notice')!.getAttribute('data-state')).toBe('busy');
  await vi.advanceTimersByTimeAsync(600);
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('链接已创建并复制');
  panel.remove(); panel = open(key);
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('网页');
  const url = panel.querySelector('.share-web-link')!.textContent;
  expect(new URL(url!).hostname).toBe('share.qingyu.invalid');
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith(url);
  click(panel, '复制链接');
  await vi.advanceTimersByTimeAsync(0);
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('链接已复制');
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith(url);
  await vi.advanceTimersByTimeAsync(3200);
  expect(document.querySelector('.share-web-notice')).toBeNull();
  expect(fetch).not.toHaveBeenCalled(); expect(invoke).not.toHaveBeenCalled(); expect(report).not.toHaveBeenCalled();
});

it('cancels the publishing confirmation without creating or copying a link', () => {
  const key = crypto.randomUUID();
  let panel = web(open(key)); click(panel, '发布网页…');
  click(document.querySelector<HTMLElement>('.share-web-confirm')!, '取消');
  expect(document.querySelector('.share-web-confirm')).toBeNull();
  expect(document.querySelector('.share-web-notice')).toBeNull();
  expect(navigator.clipboard.writeText).not.toHaveBeenCalled();
  panel.remove(); panel = open(key);
  expect(panel.querySelector('.share-web-link')).toBeNull();
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('网页');
});

it('stops sharing directly with progress and success toasts, then creates a new link on republish', async () => {
  vi.useFakeTimers();
  const key = crypto.randomUUID();
  let panel = web(open(key)); createLink(panel); await vi.advanceTimersByTimeAsync(600);
  panel.remove(); panel = open(key);
  const url = panel.querySelector('.share-web-link')!.textContent;
  click(panel, '停止发布');
  expect(document.querySelector('.share-web-confirm, [aria-label="确认停止发布"]')).toBeNull();
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('正在停止…');
  panel.remove(); panel = open(key);
  expect(panel.querySelector<HTMLButtonElement>('.share-web-stop')!.disabled).toBe(true);
  await vi.advanceTimersByTimeAsync(600);
  expect(panel.querySelector('.share-web-link')).toBeNull();
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('分享已停止');
  createLink(panel); await vi.advanceTimersByTimeAsync(600);
  panel.remove(); panel = open(key);
  expect(panel.querySelector('.share-web-link')!.textContent).not.toBe(url);
});

it('retains mock publication across tabs and popovers without mixing notes or libraries', async () => {
  vi.useFakeTimers();
  const key = crypto.randomUUID();
  let panel = web(open(key)); createLink(panel); await vi.advanceTimersByTimeAsync(600);
  panel.remove(); panel = open(key);
  const url = panel.querySelector('.share-web-link')!.textContent;
  click(panel, '发送副本'); web(panel);
  expect(panel.querySelector('.share-web-link')!.textContent).toBe(url);
  panel.remove(); panel = web(open(key));
  expect(panel.querySelector('.share-web-link')!.textContent).toBe(url);
  panel.remove(); panel = web(open('other-library/' + key));
  expect(panel.querySelector('.share-web-link')).toBeNull();
  panel.remove(); panel = web(open(key + '/other-note'));
  expect(panel.querySelector('.share-web-link')).toBeNull();
});

it('previews the latest note locally as text and restores focus on close', async () => {
  vi.useFakeTimers();
  const key = crypto.randomUUID();
  let panel = web(open(key)); createLink(panel); await vi.advanceTimersByTimeAsync(600); panel.remove();
  panel = web(open(key, '<unsafe>', '<script>latest note</script>\n完整内容'));
  click(panel, '打开网页');
  const preview = document.querySelector<HTMLDialogElement>('.share-web-preview')!;
  expect(preview.open).toBe(true);
  expect(preview.textContent).toContain('本地模拟预览');
  expect(preview.textContent).toContain('<script>latest note</script>\n完整内容');
  expect(preview.querySelector('h2')!.textContent).toBe('<unsafe>');
  expect(preview.querySelector('script')).toBeNull();
  preview.querySelector('button')!.click();
  expect(preview.isConnected).toBe(false);
  expect(document.activeElement).toBe(document.body.querySelector(':scope > button:last-of-type'));
});

it('keeps a mock publication active when copying its link fails', async () => {
  vi.useFakeTimers();
  const key = crypto.randomUUID();
  let panel = web(open(key)); createLink(panel); await vi.advanceTimersByTimeAsync(600);
  panel.remove(); panel = open(key);
  const url = panel.querySelector('.share-web-link')!.textContent;
  vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error('denied'));
  click(panel, '复制链接');
  await vi.advanceTimersByTimeAsync(0);
  expect(document.querySelector('.share-web-notice')!.textContent).toContain('无法复制链接');
  expect(panel.querySelector('.share-web-link')!.textContent).toBe(url);
});

it('keeps the created link when automatic copying fails and reports the partial result', async () => {
  vi.useFakeTimers();
  vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error('denied'));
  const key = crypto.randomUUID();
  let panel = web(open(key)); createLink(panel); await vi.advanceTimersByTimeAsync(600);
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('链接已创建，但无法复制，请使用“复制链接”重试');
  panel.remove(); panel = open(key);
  expect(panel.querySelector('.share-web-link')).not.toBeNull();
});

it('prevents duplicate publishing while the mock request is pending', async () => {
  vi.useFakeTimers();
  const key = crypto.randomUUID();
  let panel = web(open(key)); createLink(panel);
  panel.remove(); panel = open(key);
  const publish = panel.querySelector<HTMLButtonElement>('.share-web-controls button')!;
  expect(publish.disabled).toBe(true); publish.click();
  expect(document.querySelector('.share-web-confirm')).toBeNull();
  await vi.advanceTimersByTimeAsync(600);
  expect(panel.querySelector('.share-web-link')).not.toBeNull();
  expect(panel.querySelector<HTMLButtonElement>('.primary-button')!.disabled).toBe(false);
  expect(navigator.clipboard.writeText).toHaveBeenCalledOnce();
});

it('writes publish and stop into the note share state and reads it back when the panel opens', async () => {
  vi.useFakeTimers();
  let saved: NoteShare | undefined;
  const store = { get: () => saved, set: vi.fn(async (share: NoteShare) => { saved = share; }) };
  const key = crypto.randomUUID();
  let panel = web(open(key, undefined, undefined, store)); createLink(panel); await vi.advanceTimersByTimeAsync(600);
  expect(saved?.status).toBe('shared');
  expect(new URL(saved!.url!).hostname).toBe('share.qingyu.invalid');
  panel.remove(); panel = open(key, undefined, undefined, store);
  expect(panel.querySelector('.share-web-link')!.textContent).toBe(saved!.url);
  // 内存兜底不参与：同一个 key 不传 store 时看不到这条链接。
  panel.remove(); panel = web(open(key));
  expect(panel.querySelector('.share-web-link')).toBeNull();
  panel.remove(); panel = open(key, undefined, undefined, store);
  click(panel, '停止发布'); await vi.advanceTimersByTimeAsync(600);
  expect(store.set).toHaveBeenLastCalledWith({ status: 'unshared' });
  panel.remove(); panel = open(key, undefined, undefined, store);
  expect(panel.querySelector('.share-web-link')).toBeNull();
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('分享已停止');
});

it('keeps the note unshared and says so when saving the share state fails', async () => {
  vi.useFakeTimers();
  const store = { get: () => undefined, set: vi.fn().mockRejectedValue(new Error('disk full')) };
  const key = crypto.randomUUID();
  let panel = web(open(key, undefined, undefined, store)); createLink(panel); await vi.advanceTimersByTimeAsync(600);
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('链接未能保存到笔记，请重试');
  expect(document.querySelector('.share-web-notice')!.getAttribute('data-state')).toBe('error');
  panel.remove(); panel = open(key, undefined, undefined, store);
  expect(panel.querySelector('.share-web-link')).toBeNull();
  expect(panel.querySelector<HTMLButtonElement>('.primary-button')!.disabled).toBe(false);
});

it('copies the link from the shared marker and reports success or a clipboard failure', async () => {
  vi.useFakeTimers();
  const url = 'https://share.qingyu.invalid/n/a';
  await copyShareLink(url);
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith(url);
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('链接已复制');
  expect(document.querySelector('.share-web-notice')!.getAttribute('data-state')).toBe('success');
  vi.mocked(navigator.clipboard.writeText).mockRejectedValueOnce(new Error('denied'));
  await copyShareLink(url);
  expect(document.querySelectorAll('.share-web-notice')).toHaveLength(1);
  expect(document.querySelector('.share-web-notice')!.textContent).toBe('无法复制链接，请检查剪贴板权限');
  await vi.advanceTimersByTimeAsync(3200);
  expect(document.querySelector('.share-web-notice')).toBeNull();
});
