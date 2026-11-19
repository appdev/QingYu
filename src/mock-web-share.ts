import { CircleCheck, CircleX, Globe, Link, LoaderCircle } from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import type { NoteShare } from './notebook';
import { alertActions, showAlert } from './alert-dialog';

// 分享状态的读写入口。正式服务接入后只换链接的生成方式，状态仍然写进笔记元数据。
export type ShareStore = { get(): NoteShare | undefined; set(share: NoteShare): Promise<void> };
// 没有传入笔记时退回内存，只在本次运行有效。
const publications = new Map<string, NoteShare>();
const memoryStore = (noteKey: string): ShareStore => ({ get: () => publications.get(noteKey), async set(share) { publications.set(noteKey, share); } });
const pending = new Map<string, Promise<void>>();
let noticeTimer = 0;

function showNotice(message: string, state: 'busy' | 'success' | 'error') {
  clearTimeout(noticeTimer);
  document.querySelector('.share-web-notice')?.remove();
  const notice = document.createElement('div'); notice.className = 'editor-history-notice share-web-notice';
  notice.role = 'status'; notice.ariaLive = 'polite'; notice.dataset.state = state;
  const icon = state === 'busy' ? LoaderCircle : state === 'success' ? CircleCheck : CircleX;
  const label = document.createElement('span');
  notice.append(createLucideIcon(document, icon, ''), label);
  (document.querySelector('#rich-pane .cm-editor') ?? document.body).append(notice);
  label.textContent = message;
  if (state !== 'busy') noticeTimer = window.setTimeout(() => notice.remove(), 3200);
}

// 复制分享链接并提示结果。writeText 要在点击处理里同步发起，WebKit 才会保留用户手势。
export async function copyShareLink(url: string) {
  try {
    await navigator.clipboard.writeText(url);
    showNotice('链接已复制', 'success');
  } catch {
    showNotice('无法复制链接，请检查剪贴板权限', 'error');
  }
}

const mockDelay = () => new Promise<void>(resolve => window.setTimeout(resolve, 600));

export function mockWebShareActions(noteKey: string, title: string, text: string, anchor: HTMLElement, store: ShareStore = memoryStore(noteKey)) {
  function publish() {
    if (pending.has(noteKey) || document.querySelector('.share-web-confirm')) return;
    const dialog = document.createElement('dialog'); dialog.className = 'share-web-confirm';
    const heading = document.createElement('h2'); heading.id = 'share-web-confirm-title'; heading.textContent = `公开分享「${title}」？`;
    dialog.setAttribute('aria-labelledby', heading.id);
    const description = document.createElement('p'); description.id = 'share-web-confirm-description';
    description.textContent = '正式发布后，获得链接的人可查看这篇笔记，直到你停止发布。当前仅模拟测试，不会上传或公开笔记。';
    dialog.setAttribute('aria-describedby', description.id);
    const create = document.createElement('button'); create.type = 'button'; create.textContent = '创建链接';
    const cancel = document.createElement('button'); cancel.type = 'button'; cancel.textContent = '取消'; cancel.onclick = () => dialog.close();
    create.onclick = () => {
      if (pending.has(noteKey)) return;
      create.disabled = true;
      dialog.close(); showNotice('正在创建链接…', 'busy');
      const url = `https://share.qingyu.invalid/n/${crypto.randomUUID()}`;
      let copied: Promise<boolean>;
      // Start clipboard access in the click handler so WebKit retains user activation.
      try { copied = navigator.clipboard.writeText(url).then(() => true, () => false); }
      catch { copied = Promise.resolve(false); }
      const operation = Promise.all([copied, mockDelay()]).then(async ([copied]) => {
        try { await store.set({ status: 'shared', url }); }
        catch { showNotice('链接未能保存到笔记，请重试', 'error'); return; }
        showNotice(copied ? '链接已创建并复制' : '链接已创建，但无法复制，请使用“复制链接”重试', copied ? 'success' : 'error');
      }).finally(() => { pending.delete(noteKey); });
      pending.set(noteKey, operation);
    };
    dialog.append(heading, description, alertActions(create, cancel));
    dialog.addEventListener('keydown', event => event.stopPropagation());
    dialog.addEventListener('close', () => { dialog.remove(); anchor.focus(); }, { once: true });
    showAlert(dialog, create);
  }

  function preview() {
    const dialog = document.createElement('dialog'); dialog.className = 'share-web-preview'; dialog.ariaLabel = '本地模拟预览';
    const label = document.createElement('p'); label.className = 'share-unavailable'; label.textContent = '本地模拟预览 · 此链接不对外开放';
    const heading = document.createElement('h2'); heading.textContent = title;
    const body = document.createElement('p'); body.className = 'share-web-body'; body.textContent = text;
    const close = document.createElement('button'); close.type = 'button'; close.className = 'button secondary-button'; close.textContent = '关闭'; close.autofocus = true; close.onclick = () => dialog.close();
    dialog.append(label, heading, body, close);
    dialog.addEventListener('close', () => { dialog.remove(); anchor.focus(); }, { once: true });
    document.body.append(dialog); dialog.showModal();
  }

  function stop() {
    if (pending.has(noteKey)) return;
    showNotice('正在停止…', 'busy');
    // 停止后不保留旧链接，再次发布会生成新链接，和 Pinto 停止分享时丢弃链接一致。
    const operation = mockDelay().then(() => store.set({ status: 'unshared' })).then(
      () => showNotice('分享已停止', 'success'),
      () => showNotice('未能停止分享，请重试', 'error'),
    ).finally(() => { pending.delete(noteKey); });
    pending.set(noteKey, operation);
  }

  return {
    get url() {
      const share = store.get();
      return share?.status === 'shared' ? share.url : undefined;
    },
    get pending() { return pending.get(noteKey); },
    publish, preview, stop,
  };
}

export function mountMockWebShare(root: HTMLElement, noteKey: string, title: string, text: string, anchor: HTMLElement, store?: ShareStore) {
  const web = mockWebShareActions(noteKey, title, text, anchor, store);
  const hint = document.createElement('p'); hint.className = 'share-unavailable';
  hint.textContent = '模拟测试 · 链接不对外开放。';
  const controls = document.createElement('div'); controls.className = 'share-web-controls';
  root.append(controls, hint);
  const closePanel = () => root.closest<HTMLElement>('.share-popover')?.hidePopover();
  function render() {
    controls.replaceChildren();
    const url = web.url;
    if (!url) {
      const preview = document.createElement('div'); preview.className = 'share-preview';
      const heading = document.createElement('strong'), body = document.createElement('p');
      heading.textContent = title; body.textContent = text.slice(0, 240); preview.append(heading, body);
      const publish = document.createElement('button'); publish.type = 'button'; publish.className = 'button primary-button';
      publish.disabled = Boolean(web.pending); publish.textContent = publish.disabled ? '正在创建链接…' : '发布网页…';
      publish.onclick = () => { closePanel(); web.publish(); };
      controls.append(preview, publish);
      return;
    }
    const link = document.createElement('p'); link.className = 'share-web-link'; link.ariaLabel = '模拟分享链接'; link.title = url;
    const prefix = document.createElement('span'), suffix = document.createElement('span');
    prefix.textContent = url.slice(0, -20); suffix.textContent = url.slice(-20); link.append(prefix, suffix);
    const copy = document.createElement('button'); copy.type = 'button'; copy.className = 'button primary-button';
    copy.append(createLucideIcon(document, Link, ''), '复制链接');
    copy.onclick = async () => {
      copy.disabled = true;
      closePanel();
      try { await copyShareLink(url); } finally { copy.disabled = false; }
    };
    const open = document.createElement('button'); open.type = 'button'; open.append(createLucideIcon(document, Globe, ''), '打开网页');
    open.onclick = () => { closePanel(); web.preview(); };
    const stop = document.createElement('button'); stop.type = 'button'; stop.className = 'share-web-stop';
    stop.append(createLucideIcon(document, CircleX, ''), '停止发布');
    stop.onclick = () => { closePanel(); web.stop(); };
    controls.append(link, copy, open, stop);
    if (web.pending) controls.querySelectorAll('button').forEach(button => { button.disabled = true; });
  }
  render();
  void web.pending?.then(() => { if (controls.isConnected) render(); });
}
