import { CopyPlus, Folder as FolderIcon, Pin, PinOff, Trash2, Undo2 } from 'lucide';
import { noteTitle, type Notebook, type Library, type Note } from './notebook';
import { markdownBody, plainMarkdownLine } from './document-text';
import { showMenu, dismissPopupMenu as dismiss, popupMenuOpen, type MenuItem as Item } from './popup-menu';
import { noteLinkTargets, resolveNoteLink } from './note-links';
import { folderRows } from './folder-tree';
import folderIcon from '../public/miaoyan/icons/folder.svg?raw';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { alertActions, showAlert } from './alert-dialog';
import { formatLabel, markupLabel, translatedLabel, uiText } from './ui-language';

const newWindowIcon: typeof Pin = [
  ['path', { d: 'M11 19H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5M2 8h20M6 5.5h.01M9 5.5h.01' }],
  ['circle', { cx: '17.5', cy: '17.5', r: '4.5' }],
  ['path', { d: 'M17.5 15.5v4m-2-2h4' }],
];

export function noteInformation(library: Library, id: string) {
  const note = library.notes.find(note => note.id === id);
  if (!note) throw new Error('笔记不存在');
  let words = 0, inWord = false;
  // Match MiaoYan String.noteWordCount: CJK scalars singly, other letters/digits in runs.
  for (const scalar of markdownBody(note.source).body) {
    const code = scalar.codePointAt(0)!;
    if ((code >= 0x3040 && code <= 0x30ff) || (code >= 0x3400 && code <= 0x4dbf) || (code >= 0x4e00 && code <= 0x9fff) ||
        (code >= 0xac00 && code <= 0xd7af) || (code >= 0xf900 && code <= 0xfaff) || (code >= 0x20000 && code <= 0x2fa1f)) { words++; inWord = false; }
    else if (/[\p{L}\p{N}]/u.test(scalar) || inWord && /['’]/.test(scalar)) { if (!inWord) words++; inWord = true; }
    else inWord = false;
  }
  const backlinks = [...new Set(library.notes.filter(other => other.id !== id && !other.trashed
    && [...noteLinkTargets(markdownBody(other.source).body)].some(target =>
      resolveNoteLink(library, target, other).note?.id === id)).map(noteTitle))].sort();
  const date = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false });
  return { words, modified: date.format(note.updatedAt), created: date.format(note.createdAt), backlinks };
}

export function showMoveFolderSheet(rows: { name: string; depth: number; current: boolean; disabled?: boolean; run: () => void }[], restoreFocus: () => void) {
  const dialog = document.createElement('dialog');
  dialog.className = 'note-move-sheet';
  dialog.setAttribute('aria-labelledby', 'note-move-title');
  dialog.innerHTML = `<button type="button" class="note-move-grabber" aria-label="${markupLabel('展开文件夹列表')}" aria-expanded="false"></button>
    <header><h2 id="note-move-title" tabindex="-1" autofocus>${markupLabel('移动到')}</h2><button type="button" class="icon-button" aria-label="${markupLabel('取消')}">
      <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="m5 5 10 10M15 5 5 15"/></svg>
    </button></header><ul></ul>`;
  const grabber = dialog.querySelector<HTMLButtonElement>('.note-move-grabber')!;
  const motion = () => typeof dialog.animate === 'function' && !matchMedia('(prefers-reduced-motion: reduce)').matches;
  const slide = { transform: 'translateY(100%)' };
  let opening: Animation | undefined;
  let closing = false;
  function dismiss(choose?: () => void) {
    if (closing) return;
    closing = true;
    const finish = () => { dialog.close(); choose?.(); };
    if (!motion()) { finish(); return; }
    const transform = getComputedStyle(dialog).transform;
    opening?.cancel();
    dialog.inert = true;
    void dialog.animate([{ transform }, slide], { duration: 180, easing: 'ease-in', fill: 'forwards' }).finished.then(finish, finish);
  }
  const setExpanded = (expanded: boolean) => {
    dialog.toggleAttribute('data-expanded', expanded);
    grabber.ariaExpanded = String(expanded);
    grabber.ariaLabel = expanded ? translatedLabel('缩小文件夹列表') : translatedLabel('展开文件夹列表');
  };
  let startY = 0, startHeight = 0, dragged = false;
  grabber.onpointerdown = event => {
    if (!event.isPrimary || event.button !== 0) return;
    startY = event.clientY; startHeight = dialog.getBoundingClientRect().height; dragged = false;
    grabber.setPointerCapture(event.pointerId);
  };
  grabber.onpointermove = event => {
    if (!grabber.hasPointerCapture(event.pointerId) || Math.abs(event.clientY - startY) < 5) return;
    dialog.toggleAttribute('data-dragging', true);
    dialog.style.setProperty('--sheet-drag-height', `${Math.max(96, startHeight + startY - event.clientY)}px`);
  };
  function stopDragging() {
    delete dialog.dataset.dragging;
    dialog.style.removeProperty('--sheet-drag-height');
  }
  grabber.onlostpointercapture = stopDragging;
  grabber.onpointerup = event => {
    if (!grabber.hasPointerCapture(event.pointerId)) return;
    const distance = event.clientY - startY;
    stopDragging();
    if (Math.abs(distance) < 24) return;
    dragged = true;
    if (distance > 0 && !dialog.hasAttribute('data-expanded')) dismiss();
    else setExpanded(distance < 0);
  };
  grabber.onclick = event => { if (!dragged || event.detail === 0) setExpanded(!dialog.hasAttribute('data-expanded')); };
  dialog.querySelector<HTMLButtonElement>('header button')!.onclick = () => dismiss();
  dialog.addEventListener('cancel', event => { event.preventDefault(); dismiss(); });
  dialog.addEventListener('click', event => {
    if (event.target !== dialog) return;
    const bounds = dialog.getBoundingClientRect();
    if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) dismiss();
  });
  const list = dialog.querySelector('ul')!;
  for (const { name, depth, current, disabled, run } of rows) {
    const item = document.createElement('li'), button = document.createElement('button');
    button.type = 'button'; button.disabled = current || Boolean(disabled);
    button.style.setProperty('--folder-depth', String(depth));
    button.innerHTML = `<span class="sidebar-icon" aria-hidden="true">${folderIcon}</span><span></span>`;
    button.lastElementChild!.textContent = name;
    if (current) {
      button.setAttribute('aria-current', 'location');
      button.insertAdjacentHTML('beforeend', '<svg class="note-move-current" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m4 10 4 4 8-8"/></svg>');
    }
    button.onclick = () => dismiss(run);
    item.append(button); list.append(item);
  }
  dialog.addEventListener('close', () => { dialog.remove(); restoreFocus(); }, { once: true });
  document.body.append(dialog);
  dialog.showModal();
  if (motion()) opening = dialog.animate([slide, { transform: 'none' }], { duration: 280, easing: 'cubic-bezier(.2,.8,.28,1)' });
}

export function createNoteMenu(getBook: () => Notebook, actions: {
  save(): Promise<boolean>;
  share?(anchor: HTMLElement, note: Note): void;
  change(mutate: () => Promise<void>, reload?: boolean): Promise<void>;
  error(message: string): void;
  status?(message: string): void;
  revealTitle?(): void;
  mobile?: boolean;
  activate?(id: string): Promise<boolean>;
  selection?(): readonly string[];
  folderOrder?(): readonly string[];
  openNoteWindow?(id: string): Promise<void>;
  noteWindow?: boolean;
}) {
  let infoPanel: HTMLElement | undefined, nativeInfo = false;
  function selectedNotes(id: string) {
    const selected = actions.selection?.() ?? [];
    return (selected.includes(id) ? selected : [id])
      .map(id => getBook().data.notes.find(note => note.id === id)).filter(note => note !== undefined);
  }
  async function changeNotes(notes: Note[], run: (note: Note) => Promise<void>) {
    const book = getBook(), root = book.root;
    if (!await actions.save()) return;
    await actions.change(async () => {
      let completed = 0;
      try {
        for (const note of notes) {
          if (getBook() !== book || book.root !== root) throw new Error('笔记库已切换，请重新选择笔记');
          await run(note); completed++;
        }
      } catch (reason) {
        if (notes.length > 1) throw new Error(`批量操作中断，已完成 ${completed}/${notes.length} 篇：${reason instanceof Error ? reason.message : String(reason)}`);
        throw reason;
      }
    });
  }
  function dismissInfo() {
    infoPanel?.remove(); infoPanel = undefined;
    if (nativeInfo) { nativeInfo = false; void import('@tauri-apps/api/core').then(({ invoke }) => invoke('close_note_info')).catch(() => undefined); }
  }
  document.addEventListener('pointerdown', event => { if (!infoPanel?.contains(event.target as Node)) dismissInfo(); });
  window.addEventListener('resize', () => { dismiss(); dismissInfo(); });
  window.addEventListener('popstate', () => { dismiss(); dismissInfo(); });
  document.querySelector('#notes-list')?.addEventListener('scroll', () => { dismiss(); dismissInfo(); });
  document.addEventListener('keydown', event => { if (event.key === 'Escape' && infoPanel) { dismissInfo(); document.querySelector<HTMLElement>('.note-row[aria-selected=true]')?.focus(); } });
  document.addEventListener('keydown', async event => {
    if (event.isComposing || (!event.metaKey && !event.ctrlKey) || document.querySelector('dialog[open]') || popupMenuOpen()) return;
    const row = document.activeElement?.closest<HTMLElement>('.note-row');
    const note = getBook().data.notes.find(note => note.id === row?.dataset.noteId);
    if (!note) return;
    const selected = selectedNotes(note.id);
    if (selected.length > 1) {
      if (!event.altKey && event.key === 'Backspace' && !note.trashed && !getBook().external) {
        event.preventDefault();
        try { await changeNotes(selected, note => getBook().trashNote(note.id)); }
        catch (reason) { actions.error(reason instanceof Error ? reason.message : String(reason)); }
      }
      return;
    }
    let run: (() => void | Promise<void>) | undefined;
    if (event.altKey && event.key.toLowerCase() === 'i') run = () => showInfo(note.id);
    if (!event.altKey && !note.trashed) {
      if (event.shiftKey && event.key.toLowerCase() === 'p') run = () => actions.change(() => getBook().togglePin(note.id));
      if (!event.shiftKey && event.key.toLowerCase() === 'd') run = () => actions.change(() => getBook().duplicateNote(note.id), true);
      if (!event.shiftKey && event.key.toLowerCase() === 'r' && !note.readOnly) run = () => renameSelected(note.id);
      if (event.key === 'Backspace' && !getBook().external) run = () => actions.change(() => getBook().trashNote(note.id));
    }
    if (run) { event.preventDefault(); if (await actions.save()) await run(); }
  });

  function rename(id: string) {
    const note = getBook().data.notes.find(note => note.id === id);
    if (!note) return;
    actions.revealTitle?.();
    dismissInfo();
    const title = document.querySelector<HTMLElement>('#note-title')!;
    const input = document.createElement('input'); input.id = 'rename-note'; input.dataset.noteId = id; input.ariaLabel = translatedLabel('笔记名称'); input.value = noteTitle(note); input.maxLength = 500;
    if (/^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}\.md$/i.test(note.filename ?? '')) input.value = '';
    input.placeholder = translatedLabel('无标题');
    input.addEventListener('compositionstart', () => { input.dataset.composing = 'true'; });
    input.addEventListener('compositionend', () => { input.dataset.composing = 'false'; });
    title.replaceChildren(input); input.focus(); input.select();
    let finishing = false;
    const finish = async (cancel = false) => {
      if (finishing) return; finishing = true;
      try {
        if (!cancel) {
          if (!await actions.save()) { finishing = false; input.focus(); return; }
          if (!title.contains(input)) return;
          await getBook().renameNote(id, input.value);
          await actions.change(async () => undefined);
        }
        if (getBook().data.activeId === id && (title.contains(input) || !title.querySelector('input'))) title.textContent = noteTitle(getBook().data.notes.find(note => note.id === id) ?? note);
      } catch (reason) {
        finishing = false; input.setAttribute('aria-invalid', 'true'); actions.error(reason instanceof Error ? reason.message : '重命名失败');
        if (title.contains(input)) input.focus();
      }
    };
    input.onkeydown = event => {
      if (event.isComposing) return;
      if (event.key === 'Enter' || event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); void finish(event.key === 'Escape'); }
    };
    input.onblur = () => { void finish(); };
  }
  async function renameSelected(id: string) {
    if (getBook().data.activeId !== id && !await actions.activate?.(id)) return;
    rename(id);
  }
  document.querySelector('#note-title')?.addEventListener('dblclick', () => { if (getBook().current && !getBook().loadError) rename(getBook().current.id); });

  async function showInfo(id: string, toolbar = false) {
    if (!await actions.save()) return;
    const row = document.querySelector<HTMLElement>(toolbar ? '#note-more-button' : `.note-row[data-note-id="${CSS.escape(id)}"]`);
    if (!row) return;
    dismissInfo();
    const info = noteInformation(getBook().data, id), anchor = row.getBoundingClientRect();
    if ('__TAURI_INTERNALS__' in window && document.documentElement.dataset.platform === 'macos') {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('show_note_info', { info, anchor: { x: anchor.x, y: anchor.y, width: anchor.width, height: anchor.height } }); nativeInfo = true;
      } catch { actions.error('笔记简介未能打开，请重试'); }
      return;
    }
    const panel = document.createElement('div'); panel.className = 'note-info'; panel.role = 'dialog'; panel.ariaLabel = translatedLabel('笔记简介'); panel.tabIndex = -1;
    for (const text of [`字数：${info.words}`, `修改：${info.modified}`, `创建：${info.created}`]) { const line = document.createElement('p'); line.textContent = text; panel.append(line); }
    const divider = document.createElement('hr'), backlinks = document.createElement('p'), list = document.createElement('div');
    backlinks.className = 'note-info-backlinks'; backlinks.textContent = formatLabel('反向链接：{count}', { count: info.backlinks.length || translatedLabel('无') });
    list.className = 'note-info-links'; list.textContent = info.backlinks.join('\n'); panel.append(divider, backlinks, list); document.body.append(panel);
    const left = anchor.right + 18, flip = left + panel.offsetWidth > innerWidth - 8;
    panel.classList.toggle('opens-left', flip);
    panel.style.left = `${Math.max(8, flip ? anchor.left - panel.offsetWidth - 18 : left)}px`;
    panel.style.top = `${Math.max(8, Math.min(anchor.top + anchor.height / 2 - panel.offsetHeight / 2, innerHeight - panel.offsetHeight - 8))}px`;
    panel.style.setProperty('--arrow-top', `${Math.max(22, Math.min(anchor.top + anchor.height / 2 - parseFloat(panel.style.top), panel.offsetHeight - 22))}px`);
    infoPanel = panel; panel.focus();
  }
  return async (id: string, x: number, y: number, surface?: 'move' | { keyboard: boolean }) => {
    const toolbar = typeof surface === 'object';
    const anchor = toolbar ? document.querySelector<HTMLButtonElement>('#note-more-button') ?? undefined : undefined;
    const closeToolbar = anchor?.getAttribute('aria-expanded') === 'true';
    dismiss(); dismissInfo();
    if (closeToolbar) return;
    const note = getBook().data.notes.find(note => note.id === id);
    if (!note) return;
    const notes = toolbar || surface === 'move' ? [note] : selectedNotes(id);
    const multiple = notes.length > 1;
    const change = async (run: () => Promise<void>, reload = false) => { if (await actions.save()) await actions.change(run, reload); };
    const row = () => document.querySelector<HTMLElement>(`.note-row[data-note-id="${CSS.escape(id)}"]`);
    const restoreFocus = () => (toolbar ? document.querySelector<HTMLElement>('#note-more-button') : row())?.focus({ preventScroll: true });
    const compact = actions.mobile && !toolbar && !multiple;
    const trash: Item = { text: multiple ? `删除 ${notes.length} 篇笔记` : compact ? '删除' : uiText('moveToTrash'), destructive: true, run: () => changeNotes(notes, note => getBook().trashNote(note.id)) };
    const recover: Item = { text: multiple ? `恢复 ${notes.length} 篇笔记` : uiText('restoreNote'), run: () => changeNotes(notes, note => getBook().trashNote(note.id, false)) };
    if (compact || multiple) recover.icon = createLucideIcon(document, Undo2, '');
    const book = getBook(), root = book.root;
    const purge: Item[] = (root || book.server) && !book.external && !book.noteWindow && notes.every(note => note.trashed) ? [{
      text: '永久删除', destructive: true, icon: createLucideIcon(document, Trash2, ''),
      run() {
        const dialog = document.createElement('dialog'), title = document.createElement('h2'), message = document.createElement('p');
        const confirm = document.createElement('button'), cancel = document.createElement('button');
        title.id = 'purge-note-title';
        title.textContent = multiple ? formatLabel('永久删除 {count} 篇笔记？', { count: notes.length }) : formatLabel('永久删除「{title}」？', { title: noteTitle(note) });
        message.textContent = translatedLabel('这项操作无法撤销。');
        dialog.setAttribute('aria-labelledby', title.id);
        confirm.type = cancel.type = 'button';
        confirm.textContent = translatedLabel('永久删除'); confirm.className = 'destructive-button'; cancel.textContent = translatedLabel('取消');
        cancel.onclick = () => dialog.close();
        confirm.onclick = () => {
          dialog.close();
          void change(() => {
            if (getBook() !== book || book.root !== root) throw new Error('笔记库已切换，请重新选择笔记');
            return book.purgeNotes(notes.map(note => note.id));
          }, true).catch(reason => actions.error(reason instanceof Error ? reason.message : String(reason)));
        };
        dialog.addEventListener('close', () => { dialog.remove(); restoreFocus(); }, { once: true });
        dialog.append(title, message, alertActions(confirm, cancel));
        showAlert(dialog, cancel);
      },
    }] : [];
    const versions: Item = { text: '版本历史…', enabled: getBook().hasNoteHistory, async run() {
      if (!await actions.save()) return;
      const book = getBook();
      if (book.current?.id !== id && !await actions.activate?.(id)) return;
      if (!book.hasNoteHistory || book.current?.id !== id || book !== getBook()) return;
      const { showNoteHistory } = await import('./note-history');
      if (book.current?.id !== id || book !== getBook()) return;
      showNoteHistory(book, book.current, restoreFocus, async () => {
        await actions.change(async () => {}, true);
        actions.status?.('已恢复历史版本');
      });
    } };
    const pin: Item = { text: uiText(note.pinned ? 'unpin' : 'pin'), shortcut: '⇧⌘P', enabled: !getBook().external, run: () => change(() => getBook().togglePin(id)) };
    const duplicate: Item = { text: uiText('duplicate'), shortcut: '⌘D', enabled: !note.readOnly, run: () => change(() => getBook().duplicateNote(id), true) };
    const { folders } = getBook().data;
    const canMoveTo = (folderId: string) => notes.some(note => note.trashed || folderId !== note.folderId);
    const moveTo = (folderId: string) => changeNotes(notes.filter(note => note.trashed || folderId !== note.folderId), note => getBook().moveNote(note.id, folderId));
    const pickDestination = (folderId: string) => { void moveTo(folderId).catch(reason => actions.error(reason instanceof Error ? reason.message : String(reason))); };
    const destination = (folderId: string, text: string): Item => ({
      text, enabled: canMoveTo(folderId), run: () => moveTo(folderId),
      ...(multiple ? { icon: createLucideIcon(document, FolderIcon, '') } : {}),
    });
    const orderedFolders = () => [
      ...folders.filter(folder => folder.id === 'default' || folder.path === '').map(folder => ({ folder, depth: 0, parentId: null, hasChildren: false })),
      ...folderRows(folders, new Set(folders.map(folder => folder.id)), actions.folderOrder?.()),
    ];
    const move: Item = { text: multiple ? `移动 ${notes.length} 篇笔记到…` : compact ? '移动' : '移动到…', enabled: folders.some(folder => canMoveTo(folder.id)) };
    if (multiple) {
      move.icon = createLucideIcon(document, FolderIcon, '');
      trash.icon = createLucideIcon(document, Trash2, '');
    }
    if (compact || surface === 'move') {
      move.run = () => showMoveFolderSheet(orderedFolders().map(({ folder, depth }) => ({
        name: folder.name, depth, current: !canMoveTo(folder.id),
        run: () => pickDestination(folder.id),
      })), restoreFocus);
      if (surface === 'move') { if (move.enabled && !getBook().external) move.run(); return; }
    } else if (!toolbar && !getBook().external) {
      const moveItems: Item[] = [];
      const submenus = new Map<string | null, Item[]>([[null, moveItems]]);
      for (const { folder, parentId, hasChildren } of orderedFolders()) {
        const target = destination(folder.id, folder.name);
        const siblings = submenus.get(parentId)!;
        if (!hasChildren) { siblings.push(target); continue; }
        const items = [target, { text: '' }];
        submenus.set(folder.id, items);
        siblings.push({ text: folder.name, items });
      }
      move.items = moveItems;
    }
    const share: Item = { text: compact ? uiText('share') : `${uiText('share')}…`, enabled: Boolean(actions.share && !note.readOnly), async run() {
      if (!await actions.save()) return;
      const latest = getBook().data.notes.find(item => item.id === id), anchor = row();
      if (latest && !latest.trashed && !latest.readOnly && anchor) actions.share?.(anchor, latest);
    } };
    const browser = getBook().server;
    const opening: Item[] = actions.openNoteWindow && !actions.mobile && !actions.noteWindow && !getBook().external ? [{
      text: uiText(browser ? 'openNoteTab' : 'openNoteWindow'),
      needsUserActivation: browser,
      async run() { if (browser || await actions.save()) await actions.openNoteWindow?.(id); },
    }] : [];
    const details: Item[] = [
      { text: '显示简介', shortcut: '⌥⌘I', run: () => showInfo(id, toolbar) },
      { text: '重命名', shortcut: '⌘R', enabled: !note.readOnly, run: () => renameSelected(id) },
    ];
    if (!actions.mobile && '__TAURI_INTERNALS__' in window && getBook().root && note.path) {
      const platform = navigator.platform;
      const text = /Mac/.test(platform) ? '在 Finder 中显示'
        : /Win/.test(platform) ? '在文件资源管理器中显示' : '在文件管理器中显示';
      details.push({ text, async run() {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('library_reveal_note', { noteId: id });
      } });
    }
    const moving = toolbar || getBook().external ? [] : [move];
    const deleting = getBook().external ? [] : [trash];
    const groups = note.trashed ? (purge.length && (compact || multiple || toolbar) ? [[recover, ...purge]] : [[recover], moving, purge])
      : multiple ? [moving, deleting]
      : toolbar ? [[...opening, pin, duplicate], deleting]
        .map(group => group.filter(item => item.enabled !== false))
      : compact ? [[share, ...moving, ...deleting], [pin, duplicate]]
      : [opening, [pin, duplicate, share], [...moving, ...(getBook().hasNoteHistory ? [versions] : [])], details, deleting];
    const items = groups.filter(group => group.length).flatMap((group, index) => index ? [{ text: '' }, ...group] : group);
    if (anchor) {
      const toolbarIcons: [Item, typeof Pin][] = [[pin, note.pinned ? PinOff : Pin], [duplicate, CopyPlus], [trash, Trash2], [recover, Undo2]];
      for (const item of opening) toolbarIcons.push([item, newWindowIcon]);
      for (const [item, icon] of toolbarIcons) item.icon = createLucideIcon(document, icon, '');
    }
    const preview = compact ? document.createElement('article') : undefined;
    if (preview) {
      const title = document.createElement('strong');
      title.textContent = noteTitle(note);
      const body = document.createElement('div');
      const source = markdownBody(note.source).body.replace(/^\s*#[^\n]*\n?/, '');
      for (const line of source.trim().split(/\r?\n/).slice(0, 12)) {
        const text = plainMarkdownLine(line);
        if (!text) continue;
        const paragraph = document.createElement('p');
        const task = line.match(/^\s*[-*+]\s+\[([ xX])\]/);
        if (task) {
          const marker = document.createElement('span');
          marker.className = 'context-task'; marker.ariaHidden = 'true';
          if (task[1] !== ' ') { marker.classList.add('checked'); marker.textContent = '✓'; }
          paragraph.append(marker);
        }
        paragraph.append(text);
        body.append(paragraph);
      }
      preview.append(title, body);
    }
    await showMenu(items, {
      label: '笔记操作', at: { x, y }, preview, quickActions: compact && !note.trashed ? groups[0].length : 0,
      anchor: toolbar ? anchor : undefined, keyboard: toolbar ? surface.keyboard : undefined, onError: actions.error, restoreFocus,
    });
  };
}
