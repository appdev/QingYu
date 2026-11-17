import './sidebar.css';
import brandIcon from '../public/brand/qingyu-mark.svg?raw';
import allIcon from '../public/miaoyan/icons/allNotes.svg?raw';
import folderIcon from '../public/miaoyan/icons/folder.svg?raw';
import newNoteIcon from '../public/miaoyan/icons/newNote.svg?raw';
import sidebarIcon from '../public/miaoyan/icons/sidebar.svg?raw';
import { defaultNoteOrder, groupNotes, noteListDate, noteTitle, visibleNotes, type Note, type Notebook, type NoteOrder } from './notebook';
import { noteText } from './document-text';
import { folderFit, nextSidebarLayout, type SidebarCommand, type SidebarLayout } from './sidebar-layout';
import { createNoteMenu } from './note-menu';
import { createFolderMenu } from './folder-menu';
import { folderRows } from './folder-tree';
import { setupSidebarResize } from './sidebar-resize';
import { popupMenu } from './popup-menu';

const pageDepth = { folders: 0, notes: 1, note: 2 } as const;
type MobilePage = keyof typeof pageDepth;
type MobileRoute = { page: MobilePage; folderId: string | null; trash: boolean };
const backIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 5-7 7 7 7"/></svg>';

export function createSidebar(getBook: () => Notebook, actions: {
  select(id: string | null): Promise<boolean>;
  create(folderId: string, title?: string): Promise<boolean>;
  openToday?(): Promise<void>;
  quickOpen?(): boolean;
  focusEditor?(): void;
  libraryChanged?(): void;
  save(): Promise<boolean>;
  share?(anchor: HTMLElement, note: Note): void;
  reload(): void;
  error(message: string): void;
  status?(message: string): void;
  openNoteWindow?(id: string): Promise<void>;
  /** 列表行尾缩略图的图片地址，没有可显示的图片时返回 undefined。 */
  thumbnail?(note: Note): string | undefined;
  noteWindow?: boolean;
}, mobile = false) {
  const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
  const shell = $('#app-shell'), list = $('#notes-list'), folders = $('#folder-list');
  const folderPane = $('#folder-pane'), notesPane = $('#notes-pane'), documentColumn = $('#document-column');
  const folderToolbar = $('.folder-toolbar'), notesToolbar = $('.notes-toolbar');
  const tablet = mobile && !/iPhone|iPod|Android.*Mobile/i.test(navigator.userAgent)
    ? matchMedia('(min-width: 780px)') : undefined;
  const restored = mobile ? history.state?.qingyuNavigation as MobileRoute | undefined : undefined;
  let page: MobilePage = restored?.page ?? 'folders';
  const search = $<HTMLInputElement>('#note-search'), dialog = $<HTMLDialogElement>('#folder-dialog');
  const folderName = $<HTMLInputElement>('#folder-name'), folderError = $('#folder-error');
  let folderId = restored?.folderId ?? null;
  let trash = restored?.trash ?? Boolean(getBook().current?.trashed);
  const orderKey = 'miaoyan-editor.note-order.v1';
  let order = defaultNoteOrder;
  try {
    const saved = JSON.parse(localStorage.getItem(orderKey) ?? 'null') as Partial<NoteOrder> | null;
    if (saved && (saved.key === 'updatedAt' || saved.key === 'createdAt' || saved.key === 'title') &&
      typeof saved.reverse === 'boolean' && typeof saved.grouped === 'boolean') {
      order = { key: saved.key, reverse: saved.reverse, grouped: saved.grouped };
    }
  } catch { /* Keep the default list order. */ }
  const groupingKey = () => `${orderKey}.grouping:${JSON.stringify([getBook().root ?? null, trash, folderId])}`;
  const groupsByDate = (key: string) => {
    const saved = localStorage.getItem(key);
    return saved === 'true' || (saved !== 'false' && order.grouped);
  };
  const notesIn = (id: string | null, query = search.value, isTrash = trash) => visibleNotes(getBook().data, id, query, isTrash, order);
  let newFolderParent = 'default';
  let treeRoot = getBook().root;
  let displayedLibrary = getBook().data;
  const expansionKey = () => `miaoyan-editor.folder-tree.v1:${getBook().root ?? 'web'}`;
  const folderOrderKey = () => `miaoyan-editor.folder-order.v1:${getBook().root ?? 'web'}`;
  function readFolderIds(key: string): string[] {
    try { const saved: unknown = JSON.parse(localStorage.getItem(key) ?? '[]'); return Array.isArray(saved) ? saved.filter((id): id is string => typeof id === 'string') : []; }
    catch { return []; }
  }
  let expanded = new Set(readFolderIds(expansionKey())), folderOrder = readFolderIds(folderOrderKey());
  function saveExpansion() { try { localStorage.setItem(expansionKey(), JSON.stringify([...expanded])); } catch { actions.error('文件夹展开状态未能保存'); } }
  let layout: SidebarLayout = { columns: innerWidth > 900 && !actions.noteWindow ? 'all' : 'note', unfolding: false };
  let lastWidth = Infinity, autoHidden = false;
  $('#sidebar-brand-icon').innerHTML = brandIcon;
  $('#new-folder-button').innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
  $('#new-note-button').innerHTML = newNoteIcon;
  const today = $<HTMLButtonElement>('#today-note-button');
  const todayGlyph = today.querySelector('text')!;
  let todayTimer: ReturnType<typeof setTimeout>;
  let listDay = '';
  function updateToday() {
    clearTimeout(todayTimer);
    const now = new Date();
    todayGlyph.textContent = String(now.getDate());
    today.disabled = !actions.openToday || getBook().external || getBook().loadError;
    const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    todayTimer = setTimeout(refresh, midnight.getTime() - now.getTime());
  }
  today.onclick = () => { updateToday(); if (!today.disabled) void actions.openToday?.(); };
  window.addEventListener('focus', () => {
    if (new Date().toDateString() !== listDay) refresh();
    else updateToday();
  });
  const toggle = $<HTMLButtonElement>('#sidebar-button'), foldersBack = $<HTMLButtonElement>('#folders-back');
  toggle.innerHTML = mobile ? backIcon : sidebarIcon;
  if (mobile) {
    const toolbarActions = $('.sidebar-footer');
    toolbarActions.classList.replace('sidebar-footer', 'folder-toolbar-actions');
    $('.folder-toolbar').prepend(toolbarActions);
    $('.titlebar').prepend(toggle);
    toggle.title = toggle.ariaLabel = '返回笔记列表';
    toggle.setAttribute('aria-controls', 'notes-pane');
    toggle.removeAttribute('aria-expanded'); toggle.removeAttribute('aria-pressed');
    foldersBack.hidden = false; foldersBack.innerHTML = backIcon;
    $('.folder-toolbar').append($('#new-folder-button'));
    $('.notes-navigation-title').replaceWith($('.notes-summary'));
    const bottom = document.createElement('div'); bottom.className = 'mobile-library-tools';
    bottom.append(search, today, $('#new-note-button')); $('#library-sidebar').append(bottom);
    document.querySelector<HTMLElement>('#library-search-button')?.setAttribute('hidden', '');
    search.placeholder = '搜索笔记'; search.addEventListener('focus', () => { if (page === 'folders') navigate('notes'); });
  }
  const contextMenu = createNoteMenu(getBook, { save: actions.save, share: actions.share, error: actions.error, status: actions.status, mobile, activate: actions.select, folderOrder: () => folderOrder, openNoteWindow: actions.openNoteWindow, noteWindow: actions.noteWindow, revealTitle() {
    if (documentColumn.inert) navigate('note');
  }, async change(mutate, reload) {
    try {
      const focused = list.contains(document.activeElement);
      await mutate();
      if (reload) actions.reload();
      if (!actions.noteWindow) {
        const notes = notesIn(folderId);
        if (!notes.some(note => note.id === getBook().data.activeId)) await actions.select(notes[0]?.id ?? null);
      }
      refresh();
      if (focused) list.querySelector<HTMLElement>('[aria-selected=true]')?.focus({ preventScroll: true });
    } catch (error) { actions.error(error instanceof Error ? error.message : String(error)); }
  } });
  const folderMenu = createFolderMenu(getBook, { create: newFolder, createNote: id => newNote('', id), error: actions.error, change: changeFolder, mobile, reorder(id, direction, apply) {
    const rows = folderRows(getBook().data.folders, expanded, folderOrder);
    const target = rows.find(row => row.folder.id === id);
    if (!target) return false;
    const siblings = rows.filter(row => row.parentId === target.parentId).map(row => row.folder.id);
    const index = siblings.indexOf(id), next = index + direction;
    if (next < 0 || next >= siblings.length) return false;
    if (apply) {
      siblings.splice(index, 1); siblings.splice(next, 0, id);
      const valid = new Set(getBook().data.folders.map(folder => folder.id));
      const order = [...new Set([...siblings, ...folderOrder.filter(id => valid.has(id))])];
      localStorage.setItem(folderOrderKey(), JSON.stringify(order)); folderOrder = order;
      refresh(); folders.querySelector<HTMLElement>(`[data-folder-id="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
    }
    return true;
  } });
  async function changeFolder(run: () => Promise<void | false>) {
    if (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') throw new Error('笔记库正在处理其他操作，请完成后重试');
    document.body.dataset.libraryBusy = 'true'; shell.inert = true;
    try {
      if (!await actions.save()) throw new Error('当前输入尚未保存，请完成后重试');
      const previousPath = getBook().data.folders.find(folder => folder.id === folderId)?.path;
      if (await run() === false) return;
      if (folderId && !getBook().data.folders.some(folder => folder.id === folderId)) { folderId = null; trash = false; }
      if (folderId && previousPath !== getBook().data.folders.find(folder => folder.id === folderId)?.path) {
        revealAncestors(folderId);
      }
      actions.reload();
      const notes = notesIn(folderId);
      if (!notes.some(note => note.id === getBook().data.activeId)) await actions.select(notes[0]?.id ?? null);
      refresh();
    } finally { document.body.dataset.libraryBusy = 'false'; shell.inert = document.body.dataset.syncBusy === 'true'; }
  }

  function applyLayout(next: SidebarLayout) {
    autoHidden = false;
    renderLayout(next);
  }
  function renderLayout(next: SidebarLayout) {
    const restoringList = layout.columns === 'note' && next.columns !== 'note';
    const active = document.activeElement;
    const hiddenFocus = next.columns === 'note' ? $('#library-sidebar').contains(active) : next.columns === 'list' && $('#folder-pane').contains(active);
    layout = next;
    shell.dataset.sidebar = layout.columns;
    resizing.update();
    const wideList = innerWidth > 900 && layout.columns === 'list';
    const toolbarOwner = wideList ? notesToolbar : folderPane;
    const restoreToolFocus = folderToolbar.parentElement !== toolbarOwner && folderToolbar.contains(active) && (wideList || layout.columns === 'all');
    if (folderToolbar.parentElement !== toolbarOwner) toolbarOwner.prepend(folderToolbar);
    const parent = innerWidth <= 900
      ? layout.columns === 'note' ? $('.titlebar') : $('.notes-navigation')
      : layout.columns === 'note' ? $('.titlebar') : folderToolbar;
    if (toggle.parentElement !== parent) {
      parent.prepend(toggle);
    }
    if (innerWidth > 900) {
      const showFolders = layout.columns === 'all';
      toggle.ariaLabel = toggle.title = showFolders ? '隐藏文件夹栏' : layout.columns === 'list' ? '显示文件夹栏' : '显示笔记列表';
      toggle.setAttribute('aria-expanded', String(showFolders));
      toggle.setAttribute('aria-controls', layout.columns === 'note' ? 'library-sidebar' : 'folder-pane');
      toggle.removeAttribute('aria-pressed');
    } else {
      toggle.ariaLabel = toggle.title = '切换笔记列表';
      toggle.setAttribute('aria-expanded', String(layout.columns !== 'note'));
      toggle.setAttribute('aria-controls', 'library-sidebar');
      toggle.setAttribute('aria-pressed', String(layout.columns !== 'all'));
    }
    const overlay = layout.columns !== 'note' && innerWidth <= 900;
    $('#workspace').inert = $('#empty-note').inert = overlay;
    for (const button of document.querySelectorAll<HTMLButtonElement>('.actions button')) if (button !== toggle) button.inert = overlay;
    if (restoringList) {
      const selected = list.querySelector<HTMLElement>('[aria-selected="true"]');
      selected?.scrollIntoView({ block: 'nearest' });
      if (selected) {
        const header = selected.parentElement?.querySelector('.notes-year');
        // 标题栏浮在列表上方时，被它盖住的高度也要让出来。
        const covered = parseFloat(getComputedStyle(list).scrollPaddingTop) || 0;
        const overlap = list.getBoundingClientRect().top + Math.max(covered, header?.getBoundingClientRect().height ?? 0) - selected.getBoundingClientRect().top;
        if (overlap > 0) list.scrollTop -= overlap;
      }
    }
    if (restoreToolFocus && active instanceof HTMLElement) active.focus({ preventScroll: true });
    else if (hiddenFocus || active === toggle) toggle.focus({ preventScroll: true });
  }
  function fitDesktopLayout() {
    const previousWidth = lastWidth;
    lastWidth = innerWidth;
    if (autoHidden && lastWidth >= folderFit.restoreAt) {
      autoHidden = false;
      renderLayout({ columns: 'all', unfolding: false });
    } else if (layout.columns === 'all' && previousWidth >= folderFit.hideBelow && lastWidth < folderFit.hideBelow) {
      autoHidden = true;
      renderLayout({ columns: 'list', unfolding: false });
    } else renderLayout(layout);
  }
  function renderMobile(next: MobilePage, animate = false) {
    const previous = page;
    const active = document.activeElement;
    page = next;
    const wide = Boolean(tablet?.matches);
    shell.toggleAttribute('data-mobile-motion', animate);
    shell.toggleAttribute('data-mobile-wide', wide);
    shell.dataset.mobilePage = page;
    folderPane.inert = page !== 'folders';
    notesPane.inert = page === 'folders' || !wide && page === 'note';
    documentColumn.inert = !wide && page !== 'note';
    toggle.hidden = wide;
    if (page !== previous || documentColumn.inert && documentColumn.contains(active) || notesPane.inert && notesPane.contains(active)) {
      if (page === 'folders') folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]')?.focus({ preventScroll: true });
      else if (page === 'notes' || wide) {
        const selected = page === 'note' || previous === 'note' ? list.querySelector<HTMLElement>('[aria-selected=true]') : null;
        const target = selected ?? foldersBack;
        target.focus({ preventScroll: true });
      } else toggle.focus({ preventScroll: true });
    }
  }
  const routeState = (next: MobilePage) => ({ qingyuNavigation: { page: next, folderId, trash } satisfies MobileRoute });
  function navigate(next: MobilePage) {
    if (page === next) return;
    if (pageDepth[next] < pageDepth[page]) { history.go(pageDepth[next] - pageDepth[page]); return; }
    if (page === 'folders' && next === 'note') history.pushState(routeState('notes'), '');
    history.pushState(routeState(next), '');
    renderMobile(next, true);
  }
  const changeLayout = (command: SidebarCommand) => applyLayout(nextSidebarLayout(layout, command));
  const resizing = setupSidebarResize(shell, () => layout, applyLayout, actions.error, getBook().external, () => autoHidden ? 'all' : layout.columns);
  if (!mobile && innerWidth > 900) layout = { ...layout, columns: resizing.columns };
  async function selectFolder(id: string | null, isTrash = false, openList = false) {
    if (!await folderMenu.finishRename()) return false;
    if (mobile && openList) {
      const changed = folderId !== id || trash !== isTrash;
      folderId = id; trash = isTrash;
      if (changed) { search.value = ''; list.scrollTop = 0; }
      refresh(); navigate('notes'); return true;
    }
    const notes = notesIn(id, '', isTrash);
    const next = notes.find(note => note.id === getBook().data.activeId) ?? notes[0];
    if (!await actions.select(next?.id ?? null)) return false;
    folderId = id; trash = isTrash; search.value = ''; refresh(); return true;
  }
  async function toggleFolder(id: string) {
    if (!await folderMenu.finishRename()) return;
    if (expanded.has(id)) {
      const folder = getBook().data.folders.find(folder => folder.id === id), selected = getBook().data.folders.find(folder => folder.id === folderId);
      const path = folder?.path ?? folder?.name, selectedPath = selected?.path ?? selected?.name;
      if (path && selectedPath?.startsWith(path + '/') && !await selectFolder(id)) return;
      expanded.delete(id);
    } else expanded.add(id);
    saveExpansion(); refresh();
    folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]')?.focus({ preventScroll: true });
  }
  function folderRow(id: string | null, name: string, count: number, isTrash = false) {
    const button = document.createElement('button');
    button.className = 'folder-row';
    button.setAttribute('aria-pressed', String(folderId === id && trash === isTrash));
    button.dataset.folderId = id ?? '';
    button.dataset.folderPath = getBook().data.folders.find(folder => folder.id === id)?.path ?? '';
    const icon = document.createElement('span');
    icon.className = 'sidebar-icon'; icon.ariaHidden = 'true'; icon.innerHTML = id === null ? allIcon : folderIcon;
    const text = document.createElement('span'); text.className = 'folder-name'; text.textContent = name;
    const total = document.createElement('span'); total.className = 'folder-count'; total.textContent = String(count);
    button.append(icon, text, total);
    button.onclick = async () => {
      if (!await selectFolder(id, isTrash, true) || mobile) return;
      folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]')?.focus({ preventScroll: true });
    };
    return button;
  }
  function revealAncestors(id: string) {
    const rows = folderRows(getBook().data.folders, new Set(getBook().data.folders.map(folder => folder.id)));
    let parent = rows.find(row => row.folder.id === id)?.parentId;
    while (parent) { expanded.add(parent); parent = rows.find(row => row.folder.id === parent)?.parentId; }
    saveExpansion();
  }
  function refresh() {
    const now = new Date();
    listDay = now.toDateString();
    updateToday();
    if (displayedLibrary !== getBook().data) {
      displayedLibrary = getBook().data;
      actions.libraryChanged?.();
    }
    const location = document.querySelector('#note-location');
    if (location) location.textContent = getBook().data.folders.find(folder => folder.id === getBook().current?.folderId)?.name ?? '';
    if (actions.noteWindow) return;
    const listTop = list.scrollTop, foldersTop = folders.scrollTop;
    $('#new-folder-button').hidden = getBook().external;
    if (treeRoot !== getBook().root) {
      treeRoot = getBook().root;
      expanded = new Set(readFolderIds(expansionKey())); folderOrder = readFolderIds(folderOrderKey());
    }
    if (folderId && !getBook().data.folders.some(folder => folder.id === folderId)) { folderId = null; trash = false; }
    const previous = folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]');
    if (folderId && previous?.dataset.folderId === folderId && previous.dataset.folderPath !== getBook().data.folders.find(folder => folder.id === folderId)?.path) revealAncestors(folderId);
    const focusedNote = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-note-id]')?.dataset.noteId;
    folders.replaceChildren(folderRow(null, '全部笔记', getBook().data.notes.filter(note => !note.trashed).length));
    for (const { folder, depth, hasChildren } of folderRows(getBook().data.folders, expanded, folderOrder)) {
      const entry = document.createElement('div'); entry.className = 'folder-entry';
      const row = folderRow(folder.id, folder.name, visibleNotes(getBook().data, folder.id, '').length);
      row.style.setProperty('--folder-depth', String(depth)); entry.style.setProperty('--folder-depth', String(depth));
      entry.append(row);
      if (hasChildren) {
        row.setAttribute('aria-expanded', String(expanded.has(folder.id)));
        const disclosure = document.createElement('button'); disclosure.type = 'button'; disclosure.className = 'folder-disclosure';
        disclosure.ariaLabel = `${expanded.has(folder.id) ? '折叠' : '展开'}文件夹 ${folder.name}`; disclosure.setAttribute('aria-expanded', String(expanded.has(folder.id)));
        disclosure.innerHTML = '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m4 2 4 4-4 4"/></svg>';
        disclosure.onclick = () => { void toggleFolder(folder.id); };
        entry.append(disclosure);
      }
      folders.append(entry);
    }
    const trashRow = folderRow(null, '废纸篓', getBook().data.notes.filter(note => note.trashed).length, true);
    trashRow.dataset.trash = 'true'; folders.append(trashRow);
    const notes = notesIn(folderId);
    const name = trash ? '废纸篓' : getBook().data.folders.find(folder => folder.id === folderId)?.name ?? '全部笔记';
    $('#notes-heading').textContent = name;
    $('#notes-count').textContent = `${notes.length} 篇笔记`;
    list.replaceChildren();
    list.toggleAttribute('data-show-folders', !folderId);
    const date = noteListDate(now);
    for (const { label, notes: grouped } of groupNotes(notes, { ...order, grouped: groupsByDate(groupingKey()) }, now)) {
      const group = document.createElement('section'); group.role = 'group'; group.setAttribute('aria-label', label);
      if (label) {
        const heading = document.createElement('h3'); heading.className = 'notes-year'; heading.textContent = label;
        group.append(heading);
      }
      for (const note of grouped) {
        const text = noteText(note.source), row = document.createElement('button');
        row.className = 'note-row'; row.role = 'option'; row.dataset.noteId = note.id;
        row.setAttribute('aria-selected', String(note.id === getBook().data.activeId));
        const title = document.createElement('span'); title.className = 'note-row-title'; title.textContent = noteTitle(note) + (note.conflictOf ? '（冲突副本）' : '');
        const info = document.createElement('span'); info.className = 'note-row-info';
        const time = document.createElement('time'), timestamp = order.key === 'createdAt' ? note.createdAt : note.updatedAt;
        time.dateTime = new Date(timestamp).toISOString(); time.textContent = date(timestamp);
        const separator = document.createElement('span'); separator.ariaHidden = 'true'; separator.textContent = '·';
        const summary = document.createElement('span'); summary.className = 'note-row-summary'; summary.textContent = text.summary;
        info.append(time, separator, summary);
        const location = document.createElement('span'); location.className = 'note-row-folder';
        const icon = document.createElement('span'); icon.className = 'sidebar-icon'; icon.ariaHidden = 'true'; icon.innerHTML = folderIcon;
        const label = document.createElement('span'); label.textContent = getBook().data.folders.find(folder => folder.id === note.folderId)!.name;
        location.append(icon, label); row.append(title, info, location);
        const picture = actions.thumbnail?.(note);
        if (picture) {
          const image = document.createElement('img'); image.className = 'note-row-thumbnail';
          image.alt = ''; image.loading = 'lazy'; image.decoding = 'async'; image.draggable = false; image.src = picture;
          // 图片读不到时只去掉缩略图，行本身照常显示。
          image.onerror = () => { image.remove(); delete row.dataset.thumbnail; };
          row.dataset.thumbnail = ''; row.append(image);
        }
        row.onclick = async () => {
          if (!await actions.select(note.id)) return;
          refresh();
          if (!mobile || tablet?.matches) list.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(note.id)}"]`)?.focus({ preventScroll: true });
          if (mobile) { if (page !== 'folders') navigate('note'); return; }
          if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
        };
        row.oncontextmenu = async event => {
          event.preventDefault();
          if (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') return;
          if (!await actions.save()) return;
          void contextMenu(note.id, event.clientX, event.clientY);
        };
        group.append(row);
      }
      list.append(group);
    }
    if (!notes.length) {
      const empty = document.createElement('p'); empty.className = 'notes-empty';
      empty.textContent = search.value.trim() ? '未找到匹配的笔记' : trash ? '废纸篓为空' : '此文件夹还没有笔记'; list.append(empty);
    }
    if (focusedNote) list.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(focusedNote)}"]`)?.focus({ preventScroll: true });
    if (mobile) { list.scrollTop = listTop; folders.scrollTop = foldersTop; }
  }
  const newNoteFolder = (target = folderId) => target ?? getBook().data.folders[0].id;
  async function newNote(title = '', targetFolder = folderId) {
    if (!await folderMenu.finishRename()) return;
    if (!await actions.create(newNoteFolder(targetFolder), title)) return;
    folderId = targetFolder; trash = false; search.value = '';
    if (mobile) {
      refresh(); navigate('note');
      if (title) actions.focusEditor?.();
    } else {
      refresh(); list.scrollTop = 0;
      if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
    }
    if (!title) document.dispatchEvent(new Event('rename-current-note'));
  }
  async function newFolder(parentId = 'default') {
    if (getBook().external) { actions.error('临时文件模式不管理文件夹，请返回笔记库操作'); return; }
    if (!await folderMenu.finishRename()) return;
    if (!await actions.save()) return;
    newFolderParent = parentId;
    if ('__TAURI_INTERNALS__' in window && document.documentElement.dataset.platform === 'macos') {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await changeFolder(async () => {
          const name = await invoke<string | null>('prompt_folder_name');
          if (name === null) return false;
          await getBook().createFolder(name, parentId);
        });
      } catch (reason) { actions.error(reason instanceof Error ? reason.message : String(reason)); }
      return;
    }
    $('#folder-dialog-title').textContent = parentId === 'default' ? '新建文件夹' : '新建子文件夹';
    folderError.textContent = ''; folderName.value = ''; dialog.showModal(); folderName.focus();
  }
  $('#folder-form').onsubmit = async event => {
    event.preventDefault();
    if (!await actions.save()) return;
    try {
      await changeFolder(async () => { await getBook().createFolder(folderName.value, newFolderParent); });
      dialog.close();
    } catch (error) {
      folderError.textContent = error instanceof DOMException ? '保存失败，文件夹尚未创建' : error instanceof Error ? error.message : '文件夹创建失败';
      folderName.focus();
    }
  };
  $('#cancel-folder').onclick = () => dialog.close();
  $('#new-folder-button').onclick = () => { void newFolder(); };
  const listMore = $<HTMLButtonElement>('#list-more-button');
  listMore.onclick = () => {
    const box = listMore.getBoundingClientRect();
    const keyForGrouping = groupingKey();
    const currentGrouping = groupsByDate(keyForGrouping);
    const update = (patch: Partial<NoteOrder>) => {
      const next = { ...order, ...patch };
      localStorage.setItem(orderKey, JSON.stringify(next)); order = next;
      refresh(); list.scrollTop = 0;
    };
    void popupMenu([
      { text: '排序方式', items: [
        ...([['updatedAt', '修改日期'], ['createdAt', '创建日期'], ['title', '标题']] as const)
          .map(([key, text]) => ({ text, checked: order.key === key, run: () => update({ key }) })),
        { text: '' },
        ...[false, true].map(reverse => ({ text: (order.key === 'title' ? ['升序', '降序'] : ['最新在前', '最早在前'])[Number(reverse)], checked: order.reverse === reverse, run: () => update({ reverse }) })),
      ] },
      { text: '按日期分组', enabled: order.key !== 'title', items: [true, false].map(grouped =>
        ({ text: grouped ? '开启' : '关闭', checked: currentGrouping === grouped, run: () => {
          localStorage.setItem(keyForGrouping, String(grouped));
          refresh();
        } })) },
      ...(!mobile && !getBook().external ? [{ text: '' }, { text: '新建文件夹', run: () => newFolder() }] : []),
    ], box.left, box.bottom + 6, actions.error, () => listMore.focus(), '列表选项');
  };
  $('#new-note-button').onclick = $('#new-empty-note').onclick = () => { void newNote(); };
  search.addEventListener('keydown', event => { if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); void newNote(search.value); } });
  const searchButton = document.querySelector<HTMLButtonElement>('#library-search-button');
  const focusSearch = () => {
    if (mobile) navigate('notes');
    else if (layout.columns === 'note') changeLayout('notes');
    notesPane.dataset.searching = ''; search.focus();
  };
  if (searchButton) searchButton.onclick = () => { if (!actions.quickOpen?.()) focusSearch(); };
  search.addEventListener('blur', () => { if (!search.value) delete notesPane.dataset.searching; });
  if (!mobile) {
    $('.notes-toolbar').prepend($('.notes-summary'));
    // 列表滚到标题栏下面时逐渐模糊淡出，对应 Pinto 的 scrollEdgeEffectStyle(.soft)。
    const edge = document.createElement('glass-progressive-blur');
    edge.className = 'notes-scroll-edge';
    edge.setAttribute('edge', 'top'); edge.setAttribute('blur', '20'); edge.setAttribute('layers', '6'); edge.setAttribute('tint', 'var(--list)');
    notesToolbar.before(edge);
    let frame = 0;
    const markStuck = () => {
      frame = 0;
      const top = list.getBoundingClientRect().top + parseFloat(getComputedStyle(list).paddingTop);
      // 吸在顶部的分组标题加细线和背景模糊；还没滚动时不算吸顶。
      for (const heading of list.querySelectorAll<HTMLElement>('.notes-year')) {
        const stick = top + (parseFloat(getComputedStyle(heading).top) || 0);
        heading.toggleAttribute('data-stuck', list.scrollTop > 0 && heading.getBoundingClientRect().top <= stick + .5);
      }
    };
    list.addEventListener('scroll', () => { frame ||= requestAnimationFrame(markStuck); }, { passive: true });
    new MutationObserver(() => { frame ||= requestAnimationFrame(markStuck); }).observe(list, { childList: true });
  }
  toggle.onclick = () => mobile ? navigate('notes') : changeLayout(innerWidth <= 900 ? 'cycle' : layout.columns === 'note' ? 'notes' : 'folders');
  foldersBack.onclick = () => navigate('folders');
  toggle.onpointerdown = event => event.preventDefault();
  $('#sidebar-backdrop').onclick = () => changeLayout('notes');
  search.oninput = event => { if (!(event as InputEvent).isComposing) refresh(); };
  // compositionend also covers IMEs that do not emit a final input event.
  search.addEventListener('compositionend', refresh);
  // Keep WebKit from blurring/saving and replacing the clicked row before its click arrives.
  $('#library-sidebar').addEventListener('pointerdown', event => {
    if (event.pointerType !== 'touch' && !(event.target as HTMLElement).closest('input') && (event.target as HTMLElement).closest('button')) event.preventDefault();
  });
  if (mobile) {
    let press: { pointerId: number; x: number; y: number; timer: number } | undefined;
    let held = false;
    function cancelPress() { if (press) window.clearTimeout(press.timer); press = undefined; }
    document.addEventListener('pointerdown', event => {
      cancelPress(); held = false;
      if (!event.isPrimary || event.pointerType !== 'touch') return;
      const row = (event.target as HTMLElement).closest('.note-row, .folder-row');
      if (!row || !$('#library-sidebar').contains(row)) return;
      const { pointerId, clientX: x, clientY: y } = event;
      const timer = window.setTimeout(() => {
        press = undefined;
        if (!row.isConnected) return;
        row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y }));
        held = true;
      }, 450);
      press = { pointerId, x, y, timer };
    }, true);
    document.addEventListener('pointermove', event => {
      if (press?.pointerId === event.pointerId && Math.hypot(event.clientX - press.x, event.clientY - press.y) > 10) cancelPress();
    }, true);
    for (const type of ['pointerup', 'pointercancel'] as const) document.addEventListener(type, cancelPress, true);
    $('#library-sidebar').addEventListener('contextmenu', event => {
      const touch = Boolean(press) || 'pointerType' in event && event.pointerType === 'touch';
      cancelPress();
      if (held) { event.preventDefault(); event.stopImmediatePropagation(); }
      if (touch) held = true;
    }, true);
    document.addEventListener('click', event => {
      if (held && event.detail) { held = false; event.preventDefault(); event.stopImmediatePropagation(); }
    }, true);
  }
  $('#folder-pane').oncontextmenu = async event => {
    if (getBook().external) { event.preventDefault(); return; }
    if ((event.target as HTMLElement).closest('input')) return;
    event.preventDefault();
    if ((event.target as HTMLElement).closest('.sidebar-brand, .folder-toolbar') || document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') return;
    const row = (event.target as HTMLElement).closest<HTMLElement>('.folder-row') ?? (event.target as HTMLElement).closest('.folder-entry')?.querySelector<HTMLElement>('.folder-row');
    const id = row?.dataset.folderId || null;
    if (!await folderMenu.finishRename() || !await actions.save()) return;
    const target = getBook().data.folders.find(folder => folder.id === id);
    const type = row?.dataset.trash === 'true' ? 'trash' : !target ? 'all' : target.id === 'default' || target.path === '' ? 'root' : 'folder';
    await folderMenu.open(id, type, Boolean(row), event.clientX, event.clientY);
  };
  $('#notes-pane').addEventListener('contextmenu', event => {
    if (!(event.target as HTMLElement).closest('input')) event.preventDefault();
  });
  document.addEventListener('contextmenu', event => { if (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') event.preventDefault(); }, true);
  $('#folder-pane').addEventListener('keydown', event => {
    if ((event.target as HTMLElement).closest('input') || event.isComposing) return;
    const row = document.activeElement?.closest<HTMLElement>('.folder-row') ?? document.activeElement?.closest('.folder-entry')?.querySelector<HTMLElement>('.folder-row');
    if (!row) return;
    if (!event.metaKey && !event.ctrlKey && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      const id = row.dataset.folderId, item = folderRows(getBook().data.folders, expanded, folderOrder).find(item => item.folder.id === id);
      if (event.key === 'ArrowLeft') {
        if (id && expanded.has(id)) void toggleFolder(id);
        else if (item?.parentId) {
          const parent = folders.querySelector<HTMLButtonElement>(`.folder-row[data-folder-id="${CSS.escape(item.parentId)}"]`);
          if (mobile) parent?.focus(); else parent?.click();
        }
      } else if (event.key === 'ArrowRight') {
        if (item?.hasChildren && !expanded.has(item.folder.id)) void toggleFolder(item.folder.id);
      } else {
        const rows = [...folders.querySelectorAll<HTMLButtonElement>('.folder-row')], index = rows.indexOf(row as HTMLButtonElement);
        const next = rows[event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))];
        if (mobile) next?.focus(); else next?.click();
      }
      return;
    }
    if (event.key === 'ContextMenu' || event.key === 'F10' && event.shiftKey) {
      event.preventDefault(); const box = row.getBoundingClientRect(); row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: box.left + 20, clientY: box.top + 20 })); return;
    }
    const command = event.metaKey || event.ctrlKey;
    if (event.key === 'F6' && event.shiftKey || command && (event.altKey && event.key.toLowerCase() === 'r' || event.key.toLowerCase() === 'j' || event.shiftKey && event.key === 'Backspace')) {
      event.preventDefault();
      const id = row.dataset.folderId || null;
      if (event.key === 'F6') {
        if (id) folderMenu.rename(id);
      } else {
        const isTrash = row.dataset.trash === 'true';
        if (event.altKey && !id && !isTrash || !event.altKey && event.key.toLowerCase() === 'j' && isTrash || event.key === 'Backspace' && (!id || id === 'default')) return;
        void changeFolder(() => getBook().folderAction(event.altKey ? 'reveal' : event.key.toLowerCase() === 'j' ? 'terminal' : 'trash', isTrash ? 'trash' : id)).catch(reason => actions.error(String(reason)));
      }
    }
  });
  list.onkeydown = event => {
    if ((event.key === 'F10' && event.shiftKey) || event.key === 'ContextMenu') {
      const row = document.activeElement?.closest<HTMLElement>('[data-note-id]');
      if (row) {
        event.preventDefault(); const box = row.getBoundingClientRect();
        row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: box.left + 20, clientY: box.top + 20 }));
      }
      return;
    }
    const rows = [...list.querySelectorAll<HTMLButtonElement>('[data-note-id]')];
    const index = rows.findIndex(row => row === document.activeElement);
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || !rows.length) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    if (mobile && !tablet?.matches) rows[next].focus(); else rows[next].click();
  };
  document.addEventListener('keydown', event => {
    if (actions.noteWindow || event.isComposing || document.querySelector('dialog[open]')) return;
    if (event.key === 'Escape') {
      if (event.defaultPrevented) return;
      if (mobile && page !== 'folders') navigate(page === 'note' && !tablet?.matches ? 'notes' : 'folders');
      else if (!mobile && innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
    }
    if (!event.metaKey && !event.ctrlKey) return;
    if (event.key.toLowerCase() === 'n') { event.preventDefault(); if (event.shiftKey) newFolder(); else newNote(); }
    if (event.key === '1' || event.key === '2') {
      event.preventDefault();
      if (mobile) navigate(event.key === '1' ? 'folders' : 'notes');
      else changeLayout(event.key === '1' ? 'folders' : 'notes');
    }
    if (event.key.toLowerCase() === 'f' && event.shiftKey) {
      event.preventDefault();
      if (mobile && notesPane.inert) {
        if (page === 'note') window.addEventListener('popstate', () => { search.focus(); search.select(); }, { once: true });
        navigate('notes');
      } else if (!mobile && layout.columns === 'note') changeLayout('notes');
      notesPane.dataset.searching = ''; search.focus(); search.select();
    }
  });
  if (mobile) {
    if (!restored) history.replaceState({ ...history.state, ...routeState('folders') }, '');
    window.addEventListener('popstate', event => {
      if (event.state?.settings) return;
      const route = event.state?.qingyuNavigation as MobileRoute | undefined;
      const next = route?.page ?? 'folders';
      if (next === page) return;
      if (page === 'note' && !tablet?.matches) void actions.save();
      if (route && next !== 'folders' && (route.folderId !== folderId || route.trash !== trash)) {
        folderId = route.folderId; trash = route.trash; search.value = ''; list.scrollTop = 0; refresh();
      }
      renderMobile(next, true);
    });
    tablet?.addEventListener('change', () => renderMobile(page));
    renderMobile(page);
  } else {
    window.addEventListener('resize', fitDesktopLayout);
    fitDesktopLayout();
  }
  refresh();
  return {
    refresh,
    searchAll(query: string) {
      folderId = null; trash = false; search.value = query;
      refresh(); list.scrollTop = 0; focusSearch(); search.select();
    },
    openCurrent() {
      const note = getBook().current;
      if (folderId && !notesIn(folderId, '', false).some(item => item.id === note.id)) {
        folderId = note.folderId;
        revealAncestors(folderId);
      }
      trash = false; search.value = ''; refresh();
      if (mobile) navigate('note');
      else if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
      list.querySelector<HTMLElement>('[aria-selected=true]')?.scrollIntoView({ block: 'nearest' });
    },
    showCurrentMenu(x: number, y: number, keyboard: boolean) { const note = getBook().current; if (note) return contextMenu(note.id, x, y, { keyboard }); },
    moveCurrentNote() { const note = getBook().current; if (note) return contextMenu(note.id, 0, 0, 'move'); },
    revealCurrent() { folderId = null; trash = Boolean(getBook().current?.trashed); search.value = ''; refresh(); },
  };
}
