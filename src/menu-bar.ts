import './style.css';
import './menu-bar.css';
import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import { CircleCheck, CirclePlus, Pin, Settings, SquarePen } from 'lucide';
import type { MenuBarNote, MenuBarReply, MenuBarRequest, MenuBarSections } from './menu-bar-model';
import { connectPreferences } from './settings-bridge';
import { createLucideIcon } from './vendor/markra/shared/lucide';

// 菜单栏面板：打字新建笔记或查找，回车保存，⌘回车（Windows/Linux 是 Ctrl+回车）保存后在主窗口打开。
// 笔记都由主窗口去读写，这里只发请求。

type WithoutId<T> = T extends unknown ? Omit<T, 'id'> : never;
type Send = (request: WithoutId<MenuBarRequest>) => Promise<MenuBarReply>;
export type MenuBarPanelHost = {
  send: Send;
  openMain(): Promise<void>;
  openSettings(): Promise<void>;
  quit(): Promise<void>;
  hide(): Promise<void>;
  fit(height: number): void;
};

export function mountMenuBar(root: HTMLElement, host: MenuBarPanelHost) {
  const doc = root.ownerDocument;
  let sections: MenuBarSections = { pinned: [], recent: [], matches: [] };
  let highlight: number | undefined, saved = '', savedTimer = 0, failure = '', listing = 0, timer = 0, busy = false;

  root.className = 'menu-bar';
  const field = root.appendChild(doc.createElement('label')); field.className = 'menu-bar-field';
  field.append(createLucideIcon(doc, SquarePen, ''));
  const input = field.appendChild(doc.createElement('input'));
  input.type = 'text'; input.placeholder = '新建笔记或搜索'; input.ariaLabel = '新建笔记或搜索';
  input.setAttribute('role', 'combobox'); input.setAttribute('aria-controls', 'menu-bar-list'); input.setAttribute('aria-expanded', 'true');
  input.autocomplete = 'off'; input.spellcheck = false;
  root.appendChild(doc.createElement('div')).className = 'menu-bar-divider';
  const list = root.appendChild(doc.createElement('div'));
  list.className = 'menu-bar-list'; list.id = 'menu-bar-list'; list.setAttribute('role', 'listbox'); list.ariaLabel = '笔记';
  root.appendChild(doc.createElement('div')).className = 'menu-bar-divider';
  const footer = root.appendChild(doc.createElement('div')); footer.className = 'menu-bar-footer';
  const button = (text: string, run: () => Promise<void>) => {
    const element = footer.appendChild(doc.createElement('button'));
    element.type = 'button'; element.className = 'button quiet-button'; element.textContent = text; element.onclick = () => { void run().catch(show); };
    return element;
  };
  button('打开轻羽', host.openMain);
  const settings = button('', host.openSettings);
  settings.classList.add('menu-bar-settings'); settings.ariaLabel = '设置'; settings.title = '设置…';
  settings.append(createLucideIcon(doc, Settings, ''));
  button('退出', host.quit).title = '退出轻羽';

  const typing = () => Boolean(input.value.trim());
  const rows = () => sections.matches.length || typing() ? sections.matches : [...sections.pinned, ...sections.recent];
  const count = () => rows().length + (typing() ? 1 : 0);
  function show(error: unknown) { failure = error instanceof Error ? error.message : String(error); render(); }

  function noteRow(note: MenuBarNote, index: number) {
    const row = doc.createElement('button');
    row.type = 'button'; row.className = 'menu-bar-row'; row.setAttribute('role', 'option'); row.tabIndex = -1;
    row.setAttribute('aria-selected', String(highlight === index));
    const title = row.appendChild(doc.createElement('span')); title.className = 'menu-bar-title';
    if (note.pinned) title.append(createLucideIcon(doc, Pin, ''));
    title.appendChild(doc.createElement('span')).textContent = note.title;
    const summary = row.appendChild(doc.createElement('span')); summary.className = 'menu-bar-summary'; summary.textContent = note.summary;
    row.onclick = () => { void reveal(note.id); };
    return row;
  }
  function header(text: string) {
    const element = doc.createElement('h2'); element.className = 'menu-bar-header'; element.textContent = text; return element;
  }
  function render() {
    list.replaceChildren();
    if (saved) {
      const row = list.appendChild(doc.createElement('div'));
      row.className = 'menu-bar-row menu-bar-action menu-bar-saved'; row.setAttribute('role', 'status');
      row.append(createLucideIcon(doc, CircleCheck, ''));
      row.appendChild(doc.createElement('span')).textContent = `已保存“${saved}”`;
    }
    if (failure) {
      const error = list.appendChild(doc.createElement('p')); error.className = 'menu-bar-error'; error.setAttribute('role', 'alert'); error.textContent = failure;
    }
    if (typing()) {
      const create = list.appendChild(doc.createElement('button'));
      create.type = 'button'; create.className = 'menu-bar-row menu-bar-action'; create.setAttribute('role', 'option'); create.tabIndex = -1;
      create.setAttribute('aria-selected', String(highlight === 0));
      create.append(createLucideIcon(doc, CirclePlus, 'menu-bar-create-icon'));
      create.appendChild(doc.createElement('span')).textContent = `新建笔记“${input.value.trim()}”`;
      create.appendChild(doc.createElement('kbd')).textContent = '↩';
      create.onclick = () => { void capture(false); };
      if (sections.matches.length) list.append(header('笔记'));
      sections.matches.forEach((note, index) => list.append(noteRow(note, index + 1)));
    } else if (!sections.pinned.length && !sections.recent.length) {
      const empty = list.appendChild(doc.createElement('p')); empty.className = 'menu-bar-empty'; empty.textContent = '置顶或编辑过的笔记会显示在这里。';
    } else {
      if (sections.pinned.length) list.append(header('置顶'));
      sections.pinned.forEach((note, index) => list.append(noteRow(note, index)));
      if (sections.recent.length) list.append(header('最近'));
      sections.recent.forEach((note, index) => list.append(noteRow(note, sections.pinned.length + index)));
    }
    list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
    host.fit(Math.ceil(root.getBoundingClientRect().height));
  }
  async function refresh() {
    const ticket = ++listing;
    const reply = await host.send({ action: 'list', query: input.value });
    if (ticket !== listing) return;
    if (reply.error) { failure = reply.error; render(); return; }
    sections = reply.sections ?? sections; failure = ''; render();
  }
  async function capture(open: boolean) {
    if (busy || !typing()) return;
    busy = true;
    try {
      const reply = await host.send({ action: 'create', text: input.value });
      if (reply.error || !reply.created) throw new Error(reply.error ?? '没有保存');
      input.value = ''; highlight = undefined; failure = '';
      if (open) { await host.openMain(); await refresh(); return; }
      saved = reply.created.title;
      clearTimeout(savedTimer);
      savedTimer = window.setTimeout(() => { saved = ''; render(); }, 1800);
      await refresh();
    } catch (error) { show(error); }
    finally { busy = false; }
  }
  async function reveal(id: string) {
    if (busy) return;
    busy = true;
    try {
      const reply = await host.send({ action: 'open', noteId: id });
      if (reply.error) throw new Error(reply.error);
      input.value = ''; highlight = undefined;
      await host.openMain();
    } catch (error) { show(error); }
    finally { busy = false; }
  }
  function move(delta: number) {
    const total = count();
    if (!total) return;
    highlight = Math.min(Math.max((highlight ?? (delta > 0 ? -1 : total)) + delta, 0), total - 1);
    render();
  }

  input.addEventListener('input', () => {
    highlight = typing() ? 0 : undefined; failure = '';
    render();
    clearTimeout(timer); timer = window.setTimeout(() => { void refresh(); }, 80);
  });
  input.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); move(event.key === 'ArrowDown' ? 1 : -1); }
    else if (event.key === 'Escape') {
      event.preventDefault();
      if (typing()) { input.value = ''; highlight = undefined; void refresh(); }
      else void host.hide();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      const open = event.metaKey || event.ctrlKey;
      if (typing()) {
        const at = highlight ?? 0;
        if (at === 0 || open) void capture(open);
        else if (rows()[at - 1]) void reveal(rows()[at - 1].id);
      } else if (highlight !== undefined && rows()[highlight]) void reveal(rows()[highlight].id);
    }
  });
  // 点在列表上不抢走输入框的焦点，方向键和回车继续有效。
  root.addEventListener('mousedown', event => { if (!(event.target as Element).closest('input')) event.preventDefault(); });

  /** 每次打开面板：清空、取一遍笔记、光标回到输入框。 */
  function opened() {
    input.value = ''; highlight = undefined; saved = ''; failure = '';
    render(); input.focus();
    void refresh();
  }
  opened();
  return { opened, refresh };
}

if (document.querySelector('#menu-bar-root')) {
  document.documentElement.dataset.platform = /Mac/.test(navigator.platform) ? 'macos' : 'other';
  document.documentElement.dataset.menuBar = '';
  const pending = new Map<string, (reply: MenuBarReply) => void>();
  await listen<MenuBarReply>('menu-bar-reply', ({ payload }) => { pending.get(payload.id)?.(payload); pending.delete(payload.id); }, { target: 'menubar' });
  const send: Send = request => new Promise(resolve => {
    const id = crypto.randomUUID();
    const timer = setTimeout(() => { pending.delete(id); resolve({ id, error: '主窗口没有响应，请打开轻羽后重试' }); }, 8000);
    pending.set(id, reply => { clearTimeout(timer); resolve(reply); });
    void emitTo('main', 'menu-bar-request', { ...request, id }).catch(() => { clearTimeout(timer); pending.delete(id); resolve({ id, error: '无法连接主窗口' }); });
  });
  let fitted = 0;
  const panel = mountMenuBar(document.querySelector<HTMLElement>('#menu-bar-root')!, {
    send,
    openMain: () => invoke('menu_bar_open_main'),
    openSettings: async () => { await invoke('menu_bar_hide'); await invoke('open_settings', { section: 'general' }); },
    quit: () => invoke('menu_bar_quit'),
    hide: () => invoke('menu_bar_hide'),
    fit(height) { if (height !== fitted) { fitted = height; void invoke('menu_bar_fit', { height }).catch(() => {}); } },
  });
  await listen('menu-bar-shown', () => panel.opened(), { target: 'menubar' });
  void connectPreferences().catch(() => {});
}
