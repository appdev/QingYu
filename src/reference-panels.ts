import { formatLabel, markupLabel, translatedLabel } from './ui-language';
import { invoke } from '@tauri-apps/api/core';
import { CircleX, Clipboard, Globe, Image, Link, Share2 } from 'lucide';
import { native } from './preferences';
import { copyShareLink, mockWebShareActions, mountMockWebShare, type ShareStore } from './mock-web-share';
import { alertActions, showAlert } from './alert-dialog';
import { dismissPopupMenu, showMenu, type MenuItem } from './popup-menu';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { mountTabIndicator } from './glass-controls';
import { mountWebGlass, usesGlisse } from './web-glass-mount';

type PickerChoice = { title: string; target: string; detail: string };
type PickerChoices = (query: string) => PickerChoice[];
type PickerAction = (target: string) => void;
const lastShareTabs = new Map<string, number>();

export function showNoteLinkPicker(anchor: HTMLElement, choices: PickerChoices, pick: PickerAction, restore: () => void) {
  const panel = document.createElement('div');
  panel.className = 'search-picker note-link-picker';
  panel.popover = 'auto';
  panel.role = 'dialog';
  panel.ariaLabel = translatedLabel('链接到笔记');
  const search = document.createElement('input');
  search.type = 'search'; search.placeholder = panel.ariaLabel;
  search.ariaLabel = translatedLabel('搜索可链接的笔记');
  search.setAttribute('role', 'combobox');
  search.setAttribute('aria-autocomplete', 'list');
  const list = document.createElement('div');
  list.id = 'note-link-choices'; list.role = 'listbox'; list.ariaLabel = translatedLabel('可链接的笔记');
  search.setAttribute('aria-controls', list.id);
  let highlighted: string | undefined, chosen = false, shown = choices(''), options: HTMLButtonElement[] = [];
  const selected = () => shown.find(choice => choice.target === highlighted) ?? shown[0];
  function take(target: string) { chosen = true; panel.hidePopover(); pick(target); }
  function highlight(scroll = false) {
    const item = selected(), active = item && options[shown.indexOf(item)];
    options.forEach(button => { button.ariaSelected = String(button === active); });
    if (active) {
      search.setAttribute('aria-activedescendant', active.id);
      if (scroll) active.scrollIntoView?.({ block: 'nearest' });
    } else search.removeAttribute('aria-activedescendant');
    search.ariaExpanded = String(options.length > 0);
  }
  function render() {
    options = shown.map(({ title, target, detail }, index) => {
      const button = document.createElement('button');
      button.type = 'button'; button.role = 'option'; button.tabIndex = -1;
      button.id = `note-link-choice-${index}`;
      button.innerHTML = '<svg viewBox="0 0 20 20" fill="none" stroke="currentColor"><rect x="3.5" y="4" width="13" height="12" rx="2"/><path d="M6 8h8m-8 3h6"/></svg>';
      button.firstElementChild!.setAttribute('aria-hidden', 'true');
      const label = document.createElement('span'); label.textContent = title;
      button.append(label);
      if (detail) { const folder = document.createElement('small'); folder.textContent = detail; button.append(folder); }
      button.onclick = () => take(target);
      return button;
    });
    list.replaceChildren(...options);
    if (!options.length) {
      const empty = document.createElement('p');
      empty.textContent = search.value.trim() ? translatedLabel('没有匹配的笔记') : translatedLabel('没有其他笔记');
      list.append(empty);
    }
    list.scrollTop = 0;
    highlight(Boolean(highlighted));
  }
  function refresh() { highlighted = undefined; shown = choices(search.value); render(); }
  search.oninput = event => { if (!(event as InputEvent).isComposing) refresh(); };
  search.addEventListener('compositionend', refresh);
  search.onkeydown = event => {
    if (event.isComposing) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const active = selected();
      if (!active) return;
      const next = shown.indexOf(active) + (event.key === 'ArrowDown' ? 1 : -1);
      highlighted = shown[Math.min(shown.length - 1, Math.max(0, next))]?.target;
      highlight(true);
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const active = selected();
      if (active) take(active.target);
    }
  };
  const searchBox = document.createElement('div');
  searchBox.className = 'search-picker-header';
  searchBox.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="10" cy="10" r="6.5"/><path d="m15 15 5 5"/></svg>';
  searchBox.append(search); panel.append(searchBox, list); render(); document.body.append(panel);
  panel.addEventListener('toggle', event => {
    if ((event as ToggleEvent).newState !== 'closed') return;
    const focused = panel.contains(document.activeElement) || document.activeElement === document.body;
    panel.remove();
    if (!chosen && focused) restore();
  });
  panel.showPopover();
  const box = anchor.getBoundingClientRect(), width = panel.offsetWidth;
  panel.style.left = `${Math.max(8, Math.min(box.left + (box.width - width) / 2, innerWidth - width - 8))}px`;
  panel.style.top = `${Math.max(8, Math.min(box.bottom + 8, innerHeight - panel.offsetHeight - 8))}px`;
  search.focus();
}

export function confirmLinkedNote(title: string, create: () => void) {
  if (document.querySelector('.missing-note-link[open]')) return;
  const dialog = document.createElement('dialog');
  dialog.className = 'missing-note-link';
  dialog.setAttribute('aria-labelledby', 'missing-note-link-title');
  dialog.innerHTML = `<h2 id="missing-note-link-title"></h2><p>${markupLabel('还没有这个标题的笔记。')}</p>`;
  dialog.querySelector('h2')!.textContent = formatLabel('新建「{title}」？', { title });
  const confirm = document.createElement('button'), cancel = document.createElement('button');
  confirm.type = cancel.type = 'button'; confirm.textContent = translatedLabel('newNote'); cancel.textContent = translatedLabel('取消');
  cancel.onclick = () => dialog.close();
  confirm.onclick = () => { dialog.close(); create(); };
  dialog.append(alertActions(confirm, cancel));
  dialog.addEventListener('close', () => dialog.remove(), { once: true });
  showAlert(dialog, confirm);
}

export function showShare(anchor: HTMLElement, title: string, text: string, report: (message: string) => void, picture?: import('./note-picture').PictureRequest, noteKey: string = crypto.randomUUID(), shareStore?: ShareStore) {
  const previous = document.querySelector<HTMLElement>('.share-popover:not([inert])');
  const closingCurrent = previous && anchor.ariaExpanded === 'true';
  previous?.hidePopover?.();
  if (closingCurrent) return;
  const root = document.documentElement;
  const glass = usesGlisse(root);
  const nativeShare = native && (root.dataset.platform === 'macos' || root.hasAttribute('data-mobile'));
  async function sendCopy() {
    try {
      if (nativeShare) {
        const { x, y, width, height } = anchor.getBoundingClientRect();
        await invoke('note_share_text', { title, text, anchor: { x, y, width, height } });
      } else await navigator.share({ title, text });
      return true;
    } catch (err) {
      if (!(err instanceof DOMException && err.name === 'AbortError')) report('无法打开系统分享，请重试或复制为纯文本');
      return false;
    }
  }
  async function copyText() {
    try { await navigator.clipboard.writeText(text); return true; }
    catch { report('无法复制，请检查剪贴板权限'); return false; }
  }

  if (native && root.hasAttribute('data-mobile') && anchor instanceof HTMLButtonElement && anchor.id === 'share-button' && !anchor.closest('[data-mobile-wide]')) {
    if (anchor.getAttribute('aria-expanded') === 'true') { dismissPopupMenu(); return; }
    const web = mockWebShareActions(noteKey, title, text, anchor, shareStore);
    const url = web.url, enabled = !web.pending;
    const restore = () => anchor.focus({ preventScroll: true });
    const icon = (node: Parameters<typeof createLucideIcon>[1]) => createLucideIcon(document, node, '');
    const items: MenuItem[] = url ? [
      { text: '复制链接', icon: icon(Link), enabled, needsUserActivation: glass, run: () => copyShareLink(url) },
      { text: '打开网页', icon: icon(Globe), enabled, run: web.preview },
      { text: '停止发布', icon: icon(CircleX), enabled, destructive: true, run: web.stop },
      { text: '' },
    ] : [{ text: '分享链接…', icon: icon(Link), enabled, run: web.publish }];
    items.push(
      { text: '发送笔记…', icon: icon(Share2), run: async () => { await sendCopy(); } },
      { text: '复制为纯文本', icon: icon(Clipboard), needsUserActivation: glass, run: async () => { await copyText(); } },
    );
    if (picture) items.push({ text: '分享为图片…', icon: icon(Image), async run() {
      const { showNotePicture } = await import('./note-picture');
      await showNotePicture(picture, restore);
    } });
    void showMenu(items, { anchor, label: '分享', onError: report, restoreFocus: restore });
    return;
  }

  const panel = document.createElement('div'); panel.className = 'share-popover fk-transition fk-opacity'; panel.setAttribute('popover', 'auto'); panel.role = 'dialog'; panel.ariaLabel = translatedLabel('share');
  if (!glass) panel.classList.add('lb', 'qingyu-glass');
  else panel.classList.add('web-glass-frost');
  panel.tabIndex = -1; panel.autofocus = true;
  panel.addEventListener('keydown', event => event.stopPropagation());
  const tabs = document.createElement('div'); tabs.className = 'share-tabs'; tabs.role = 'tablist'; tabs.ariaLabel = translatedLabel('分享方式');
  const content = document.createElement('div'); content.role = 'tabpanel'; content.id = 'share-content';
  const modes = [
    { index: 0, title: translatedLabel('发送') },
    { index: 1, title: translatedLabel('网页') },
    ...(picture ? [{ index: 2, title: translatedLabel('图片') }] : []),
  ];
  const buttons: HTMLButtonElement[] = [];
  let disposePicture: (() => void) | undefined;
  let tabRevision = 0;
  let indicator: ReturnType<typeof mountTabIndicator>;
  function select(position: number) {
    const { index } = modes[position];
    const revision = ++tabRevision;
    disposePicture?.(); disposePicture = undefined;
    lastShareTabs.set(noteKey, index);
    for (const [i, button] of buttons.entries()) { button.setAttribute('aria-selected', String(i === position)); button.tabIndex = i === position ? 0 : -1; }
    indicator?.select(buttons[position]);
    content.replaceChildren();
    const detail = document.createElement('p'); detail.className = 'share-description';
    detail.textContent = translatedLabel((['发送笔记此刻的内容，之后的修改不会同步给收件人。', '模拟创建网页链接。', '将笔记转成图片，方便发送或发布。'] as const)[index]); content.append(detail);
    if (index === 0) {
      if (nativeShare || typeof navigator.share === 'function') {
        const send = document.createElement('button'); send.textContent = translatedLabel('share') + '…'; send.className = 'button primary-button share-send';
        send.onclick = async () => {
          send.disabled = true;
          try { if (await sendCopy()) panel.hidePopover(); }
          finally { send.disabled = false; }
        };
        content.append(send);
      } else {
        const unavailable = document.createElement('p'); unavailable.className = 'share-unavailable';
        unavailable.textContent = translatedLabel('当前环境不支持系统分享，可复制为纯文本后发送。'); content.append(unavailable);
      }
      const copy = document.createElement('button'); copy.textContent = translatedLabel('复制为纯文本'); copy.className = 'share-copy';
      copy.onclick = async () => { if (await copyText()) panel.hidePopover(); }; content.append(copy);
      return;
    } else if (index === 1) {
      mountMockWebShare(content, noteKey, title, text, anchor, shareStore);
      return;
    } else if (index === 2 && picture) {
      if (native && !root.hasAttribute('data-mobile')) {
        void import('./note-picture').then(({ mountNotePicture }) => {
          if (panel.isConnected && revision === tabRevision) disposePicture = mountNotePicture(content, picture);
        }).catch(() => {
          if (panel.isConnected && revision === tabRevision) report('无法打开图片预览');
        });
        return;
      }
      const open = document.createElement('button');
      open.type = 'button'; open.className = 'button primary-button button-wide'; open.textContent = translatedLabel('分享为图片') + '…';
      open.onclick = async () => {
        panel.hidePopover();
        try {
          const { showNotePicture } = await import('./note-picture');
          await showNotePicture(picture, () => anchor.focus({ preventScroll: true }));
        } catch { report('无法打开图片预览'); }
      };
      content.append(open);
    }
  }
  for (const [position, { title }] of modes.entries()) {
    const button = document.createElement('button'); button.role = 'tab'; button.textContent = title; button.setAttribute('aria-controls', content.id);
    button.onclick = () => select(position); buttons.push(button); tabs.append(button);
    button.onkeydown = event => { if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return; event.preventDefault(); const next = (position + (event.key === 'ArrowRight' ? 1 : buttons.length - 1)) % buttons.length; select(next); buttons[next].focus(); };
  }
  panel.append(tabs, content); document.body.append(panel);
  panel.addEventListener('beforetoggle', event => {
    const closing = (event as ToggleEvent).newState === 'closed';
    anchor.ariaExpanded = String(!closing);
    panel.inert = closing;
  });
  panel.showPopover();
  const unmountGlass = glass ? mountWebGlass(panel, { tint: 78, blur: 12 }) : undefined;
  indicator = mountTabIndicator(tabs);
  select(Math.max(0, modes.findIndex(mode => mode.index === lastShareTabs.get(noteKey))));
  const box = anchor.getBoundingClientRect();
  const center = box.left + box.width / 2;
  const left = Math.max(8, Math.min(center - panel.offsetWidth / 2, innerWidth - panel.offsetWidth - 8));
  const pointer = Math.max(20, Math.min(center - left, panel.offsetWidth - 20));
  panel.style.left = `${left}px`;
  panel.style.setProperty('--share-pointer-x', `${pointer}px`);

  const below = innerHeight - box.bottom - 20;
  const above = box.top - 20;
  if (below < panel.offsetHeight && above > below) {
    panel.dataset.placement = 'above';
    panel.style.bottom = `${innerHeight - box.top + 12}px`;
  } else {
    panel.style.top = `${box.bottom + 12}px`;
  }
  panel.style.setProperty('--share-available-height', `${panel.dataset.placement === 'above' ? above : below}px`);
  panel.addEventListener('toggle', event => {
    if ((event as ToggleEvent).newState !== 'closed') return;
    tabRevision++;
    if (panel.contains(document.activeElement) || document.activeElement === document.body) anchor.focus({ preventScroll: true });
    const remove = () => { indicator?.destroy(); unmountGlass?.(); disposePicture?.(); panel.remove(); };
    const animations = panel.getAnimations?.() ?? [];
    if (animations.length) void Promise.allSettled(animations.map(animation => animation.finished)).then(remove);
    else remove();
  });
}
