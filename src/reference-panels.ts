import { invoke } from '@tauri-apps/api/core';
import { native } from './preferences';
import { mountMockWebShare, type ShareStore } from './mock-web-share';
import { alertActions, showAlert } from './alert-dialog';

type PickerChoice = { title: string; target: string; detail: string };
type PickerChoices = (query: string) => PickerChoice[];
type PickerAction = (target: string) => void;
const lastShareTabs = new Map<string, number>();

export function showNoteLinkPicker(anchor: HTMLElement, choices: PickerChoices, pick: PickerAction, restore: () => void) {
  const panel = document.createElement('div');
  panel.className = 'search-picker note-link-picker';
  panel.popover = 'auto';
  panel.role = 'dialog';
  panel.ariaLabel = '链接到笔记';
  const search = document.createElement('input');
  search.type = 'search'; search.placeholder = panel.ariaLabel;
  search.ariaLabel = '搜索可链接的笔记';
  search.setAttribute('role', 'combobox');
  search.setAttribute('aria-autocomplete', 'list');
  const list = document.createElement('div');
  list.id = 'note-link-choices'; list.role = 'listbox'; list.ariaLabel = '可链接的笔记';
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
      empty.textContent = search.value.trim() ? '没有匹配的笔记' : '没有其他笔记';
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
  dialog.innerHTML = '<h2 id="missing-note-link-title"></h2><p>还没有这个标题的笔记。</p>';
  dialog.querySelector('h2')!.textContent = `新建「${title}」？`;
  const confirm = document.createElement('button'), cancel = document.createElement('button');
  confirm.type = cancel.type = 'button'; confirm.textContent = '新建笔记'; cancel.textContent = '取消';
  cancel.onclick = () => dialog.close();
  confirm.onclick = () => { dialog.close(); create(); };
  dialog.append(alertActions(confirm, cancel));
  dialog.addEventListener('close', () => dialog.remove(), { once: true });
  showAlert(dialog, confirm);
}

export function showLockNotePanel() {
  const dialog = document.createElement('dialog'); dialog.className = 'reference-panel reference-lock';
  dialog.ariaLabel = '设置密码';
  const header = document.createElement('header'), heading = document.createElement('h2'), close = document.createElement('button');
  heading.textContent = dialog.ariaLabel; close.textContent = '取消'; close.onclick = () => dialog.close();
  const content = document.createElement('div'); content.className = 'reference-panel-content';
  content.innerHTML = '<main><p>为锁定的笔记设置密码</p><label>密码<input type="password" disabled autocomplete="off"></label><label>确认密码<input type="password" disabled autocomplete="off"></label><label>密码提示（建议填写）<input disabled></label><p class="reference-unavailable">笔记加密服务尚未接入，当前不会设置密码或加密笔记。</p></main>';
  const done = document.createElement('button'); done.textContent = '完成'; done.disabled = true; header.append(close, heading, done);
  dialog.append(header, content); document.body.append(dialog);
  dialog.addEventListener('keydown', event => event.stopPropagation());
  dialog.addEventListener('close', () => dialog.remove(), { once: true }); dialog.showModal();
}

export function showShare(anchor: HTMLElement, title: string, text: string, report: (message: string) => void, picture?: import('./note-picture').PictureRequest, noteKey: string = crypto.randomUUID(), shareStore?: ShareStore) {
  document.querySelector<HTMLElement>('.share-popover')?.hidePopover?.();
  const panel = document.createElement('div'); panel.className = 'share-popover'; panel.setAttribute('popover', 'auto'); panel.role = 'dialog'; panel.ariaLabel = '分享';
  panel.addEventListener('keydown', event => event.stopPropagation());
  const tabs = document.createElement('div'); tabs.className = 'share-tabs'; tabs.role = 'tablist'; tabs.ariaLabel = '分享方式';
  const content = document.createElement('div'); content.role = 'tabpanel'; content.id = 'share-content';
  const buttons: HTMLButtonElement[] = [];
  let removePicture: (() => void) | undefined, revision = 0;
  function select(index: number) {
    lastShareTabs.set(noteKey, index);
    const selected = ++revision;
    removePicture?.(); removePicture = undefined;
    for (const [i, button] of buttons.entries()) { button.setAttribute('aria-selected', String(i === index)); button.tabIndex = i === index ? 0 : -1; }
    content.replaceChildren();
    const detail = document.createElement('p'); detail.className = 'share-description';
    detail.textContent = ['发送笔记的当前副本，之后的修改不会同步给收件人。', '获得链接的人都可查看。网页会随笔记更新，直到停止发布。', '将笔记转成图片，方便发送或发布。'][index]; content.append(detail);
    if (index === 0) {
      const nativeShare = native && (document.documentElement.dataset.platform === 'macos' || document.documentElement.hasAttribute('data-mobile'));
      if (nativeShare || typeof navigator.share === 'function') {
        const send = document.createElement('button'); send.textContent = '分享…'; send.className = 'primary-button share-send';
        send.onclick = async () => {
          send.disabled = true;
          try {
            if (nativeShare) {
              const { x, y, width, height } = anchor.getBoundingClientRect();
              await invoke('note_share_text', { title, text, anchor: { x, y, width, height } });
            } else await navigator.share({ title, text });
            panel.hidePopover();
          } catch (err) {
            if (!(err instanceof DOMException && err.name === 'AbortError')) report('无法打开系统分享，请重试或复制为纯文本');
          } finally { send.disabled = false; }
        };
        content.append(send);
      } else {
        const unavailable = document.createElement('p'); unavailable.className = 'share-unavailable';
        unavailable.textContent = '当前环境不支持系统分享，可复制为纯文本后发送。'; content.append(unavailable);
      }
      const copy = document.createElement('button'); copy.textContent = '复制为纯文本'; copy.className = 'share-copy';
      copy.onclick = async () => { try { await navigator.clipboard.writeText(text); panel.hidePopover(); } catch { report('无法复制，请检查剪贴板权限'); } }; content.append(copy);
      return;
    } else if (index === 1) {
      mountMockWebShare(content, noteKey, title, text, anchor, shareStore);
      return;
    } else if (index === 2 && picture) {
      void import('./note-picture').then(({ mountNotePicture }) => {
        if (panel.isConnected && selected === revision) removePicture = mountNotePicture(content, picture);
      }).catch(() => report('无法打开图片预览'));
      return;
    } else {
      const preview = document.createElement('div'); preview.className = 'share-preview';
      const heading = document.createElement('strong'), body = document.createElement('p'); heading.textContent = title; body.textContent = text.slice(0, 240); preview.append(heading, body); content.append(preview);
      if (index === 2) {
        const backgrounds = document.createElement('label'); backgrounds.className = 'share-access'; backgrounds.append(document.createTextNode('背景'));
        const select = document.createElement('select'); select.disabled = true; select.ariaLabel = '图片背景';
        for (const name of ['纯色', '纸张', '渐变']) { const option = document.createElement('option'); option.textContent = name; select.append(option); }
        backgrounds.append(select); content.append(backgrounds);
      }
      const actions = document.createElement('div'); actions.className = 'share-actions';
      for (const [i, label] of ['复制图片', '保存…', '分享…'].entries()) {
        const action = document.createElement('button'); action.className = i ? 'secondary-button' : 'primary-button'; action.disabled = true; action.textContent = label; actions.append(action);
      }
      content.append(actions);
    }
    const unavailable = document.createElement('p'); unavailable.className = 'share-unavailable'; unavailable.textContent = '灰色操作尚未接入。'; content.append(unavailable);
  }
  for (const [index, title] of ['发送副本', '网页', '图片'].entries()) {
    const button = document.createElement('button'); button.role = 'tab'; button.textContent = title; button.setAttribute('aria-controls', content.id);
    button.onclick = () => select(index); buttons.push(button); tabs.append(button);
    button.onkeydown = event => { if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return; event.preventDefault(); const next = (index + (event.key === 'ArrowRight' ? 1 : buttons.length - 1)) % buttons.length; select(next); buttons[next].focus(); };
  }
  panel.append(tabs, content); document.body.append(panel); select(lastShareTabs.get(noteKey) ?? 0); panel.showPopover();
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
  panel.addEventListener('toggle', event => { if ((event as ToggleEvent).newState === 'closed') { removePicture?.(); panel.remove(); anchor.focus(); } });
}
