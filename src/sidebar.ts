import './sidebar.css';
import brandIcon from '../public/brand/qingyu-mark.svg?raw';
import allIcon from '../public/miaoyan/icons/allNotes.svg?raw';
import folderIcon from '../public/miaoyan/icons/folder.svg?raw';
import newNoteIcon from '../public/miaoyan/icons/newNote.svg?raw';
import folderPlusIcon from '../public/miaoyan/icons/folder-badge-plus.svg?raw';
import { defaultNoteOrder, folderNoteCounts, groupNotes, isShared, noteListDate, noteTitle, visibleNotes, type Note, type Notebook, type NoteOrder } from './notebook';
import { noteSummary } from './document-text';
import { folderFit, nextSidebarLayout, type SidebarCommand, type SidebarLayout } from './sidebar-layout';
import { createNoteMenu } from './note-menu';
import { createFolderMenu } from './folder-menu';
import { folderRows } from './folder-tree';
import { setupSidebarResize } from './sidebar-resize';
import { dismissPopupMenu, showMenu, type MenuItem, type MenuOptions } from './popup-menu';
import { ArrowUpDown, CalendarDays, Check, Folder as FolderIcon, FolderPlus, Inbox, Link as LinkIcon, Search, SquarePen, Trash2, X } from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { formatLabel, translatedLabel, uiText } from './ui-language';

const pageDepth = { folders: 0, notes: 1, note: 2 } as const;
type MobilePage = keyof typeof pageDepth;
type MobileRoute = { page: MobilePage; folderId: string | null; trash: boolean };
const backIcon = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 5-7 7 7 7"/></svg>';
// Pinto uses sidebar.left on macOS; adapt the resting geometry from its Charm.swift for the webview.
const sidebarIcon = `<svg viewBox="2 2 20 20" fill="none" stroke="currentColor" aria-hidden="true">
  <rect x="2.8" y="5" width="18.4" height="14" rx="3.3" stroke-width="1.55"/>
  <path d="M9 5v14" stroke-width="1.35"/>
  <path d="M5.2 8.5h1.5m-1.5 2.2h1.5m-1.5 2.2h1.5" stroke-width="1" stroke-linecap="round"/>
</svg>`;

export function createSidebar(getBook: () => Notebook, actions: {
  select(id: string | null): Promise<boolean>;
  create(folderId: string, title?: string): Promise<boolean>;
  openToday?(): Promise<void>;
  quickOpen?(): boolean;
  focusEditor?(): void;
  /** 打开的是一篇还没写过字的新笔记：再点新建只把光标放回它里面，不再叠一篇空白笔记。 */
  blankNote?(): boolean;
  libraryChanged?(): void;
  save(): Promise<boolean>;
  share?(anchor: HTMLElement, note: Note): void;
  reload(): void;
  error(message: string): void;
  status?(message: string): void;
  openNoteWindow?(id: string): Promise<void>;
  /** 列表行尾缩略图的图片地址，没有可显示的图片时返回 undefined。 */
  thumbnail?(note: Note): string | undefined;
  mountThumbnail?(note: Note, image: HTMLImageElement, row: HTMLElement, url: string): (() => void) | undefined;
  noteWindow?: boolean;
}, mobile = false) {
  const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
  const shell = $('#app-shell'), list = $('#notes-list'), folders = $('#folder-list');
  const externalWindow = shell.hasAttribute('data-external-window');
  const folderPane = $('#folder-pane'), notesPane = $('#notes-pane'), documentColumn = $('#document-column');
  const folderToolbar = $('.folder-toolbar'), notesToolbar = $('.notes-toolbar');
  const desktopWeb = !mobile && document.documentElement.hasAttribute('data-web');
  const services = $('.sidebar-services'), footer = $('.sidebar-footer');
  const account = $<HTMLButtonElement>('#sidebar-account'), settings = $<HTMLButtonElement>('#settings-button');
  const logout = $('#logout-button');
  const tablet = mobile && !/iPhone|iPod|Android.*Mobile/i.test(navigator.userAgent)
    ? matchMedia('(min-width: 780px)') : undefined;
  const restored = mobile ? history.state?.qingyuNavigation as MobileRoute | undefined : undefined;
  let page: MobilePage = restored?.page ?? 'folders';
  const search = $<HTMLInputElement>('#note-search'), dialog = $<HTMLDialogElement>('#folder-dialog');
  const folderName = $<HTMLInputElement>('#folder-name'), folderError = $('#folder-error'), createFolder = $<HTMLButtonElement>('#create-folder');
  // 和 iOS 提示框一样，名称为空时「创建」不可点。
  folderName.addEventListener('input', () => { createFolder.disabled = !folderName.value.trim(); folderError.textContent = ''; });
  let folderId = restored?.folderId ?? null;
  let trash = restored?.trash ?? Boolean(getBook().current?.trashed);
  const orderKey = 'miaoyan-editor.note-order.v1';
  const hideSummaryKey = 'qingyu.list.hideSummary';
  window.addEventListener('storage', event => { if (event.key === hideSummaryKey || event.key === null) refresh(); });
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
  let selectedNotes = new Set<string>(), selectionAnchor: string | null = null;
  let selectionBook = getBook(), selectionScope = '', selectionActive = getBook().data.activeId;
  list.setAttribute('aria-multiselectable', 'true');
  const noteRows = () => [...list.querySelectorAll<HTMLButtonElement>('[data-note-id]')];
  function renderSelection() {
    for (const row of noteRows()) row.setAttribute('aria-selected', String(selectedNotes.has(row.dataset.noteId!)));
  }
  function extendSelection(id: string, event: Pick<MouseEvent, 'shiftKey' | 'metaKey' | 'ctrlKey'>) {
    const ids = noteRows().map(row => row.dataset.noteId!);
    if (event.shiftKey) {
      const anchor = ids.indexOf(selectionAnchor ?? ''), end = ids.indexOf(id);
      if (anchor < 0) selectionAnchor = id;
      const start = anchor < 0 ? end : anchor;
      const range = ids.slice(Math.min(start, end), Math.max(start, end) + 1);
      selectedNotes = new Set(event.metaKey || event.ctrlKey ? [...selectedNotes, ...range] : range);
    } else {
      if (selectedNotes.has(id)) selectedNotes.delete(id); else selectedNotes.add(id);
      selectionAnchor = id;
    }
    renderSelection();
    noteRows().find(row => row.dataset.noteId === id)?.focus({ preventScroll: true });
  }
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
  const editFolders = mobile ? document.createElement('button') : undefined;
  const editingFolders = () => folderPane.hasAttribute('data-editing-folders');
  $('#sidebar-brand-icon').innerHTML = brandIcon;
  $('#new-folder-button').innerHTML = mobile ? folderPlusIcon : '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>';
  const compose = $('#new-note-button');
  if (mobile) compose.replaceChildren(createLucideIcon(document, SquarePen, ''));
  else compose.innerHTML = newNoteIcon;
  const today = $<HTMLButtonElement>('#today-note-button');
  const todayGlyph = today.querySelector('text')!;
  let todayTimer: ReturnType<typeof setTimeout>;
  let listDay = '';
  let searchTimer: ReturnType<typeof setTimeout> | undefined, lastSearchRefresh = 0;
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
  const titlebarTools = toggle.closest('.desktop-window-tools');
  toggle.innerHTML = mobile ? backIcon : sidebarIcon;
  if (editFolders) {
    footer.classList.replace('sidebar-footer', 'folder-toolbar-actions');
    account.classList.add('icon-button');
    settings.hidden = true;
    footer.prepend(account);
    folderToolbar.prepend(footer);
    $('.titlebar').prepend(toggle);
    toggle.title = toggle.ariaLabel = translatedLabel('返回笔记列表');
    toggle.setAttribute('aria-controls', 'notes-pane');
    toggle.removeAttribute('aria-expanded'); toggle.removeAttribute('aria-pressed');
    foldersBack.hidden = false; foldersBack.innerHTML = backIcon;
    editFolders.type = 'button'; editFolders.id = 'edit-folders'; editFolders.className = 'icon-button';
    editFolders.textContent = translatedLabel('menu.edit'); editFolders.ariaLabel = translatedLabel('编辑文件夹');
    editFolders.setAttribute('aria-controls', 'folder-list');
    editFolders.onclick = async () => { if (await folderMenu.finishRename()) setFolderEditing(!editingFolders()); };
    folderToolbar.append($('#new-folder-button'), editFolders);
    $('.notes-navigation-title').replaceWith($('.notes-summary'));
    const bottom = document.createElement('div'); bottom.className = 'mobile-library-tools';
    const searchField = document.createElement('label'); searchField.className = 'mobile-library-search';
    searchField.append(createLucideIcon(document, Search, ''), search);
    // 和 Pinto 一样，搜索时收起顶栏，右侧的今日笔记和新建换成一个关闭按钮。
    const cancelSearch = document.createElement('button');
    cancelSearch.type = 'button'; cancelSearch.className = 'icon-button mobile-search-cancel'; cancelSearch.ariaLabel = translatedLabel('取消搜索');
    cancelSearch.append(createLucideIcon(document, X, ''));
    cancelSearch.onclick = () => endMobileSearch(true);
    bottom.append(searchField, today, compose, cancelSearch); $('#library-sidebar').append(bottom);
    document.querySelector<HTMLElement>('#library-search-button')?.setAttribute('hidden', '');
    search.addEventListener('focus', () => {
      if (!shell.hasAttribute('data-mobile-search')) { searchFrom = page; shell.toggleAttribute('data-mobile-search', true); }
      if (page !== 'folders') return;
      folderId = null; trash = false; list.scrollTop = 0;
      refresh(); navigate('notes');
    });
  }
  const contextMenu = createNoteMenu(getBook, { save: actions.save, share: actions.share, error: actions.error, status: actions.status, mobile, activate: actions.select, selection: () => [...selectedNotes], folderOrder: () => folderOrder, openNoteWindow: actions.openNoteWindow, noteWindow: actions.noteWindow, revealTitle() {
    if (documentColumn.inert) navigate('note');
  }, async change(mutate, reload) {
    if (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') { actions.error('笔记库正在处理其他操作，请完成后重试'); return; }
    const focusedNote = document.activeElement?.closest<HTMLElement>('.note-row')?.dataset.noteId;
    document.body.dataset.libraryBusy = 'true'; shell.inert = true;
    try {
      try { await mutate(); }
      finally {
        if (reload) actions.reload();
        if (!actions.noteWindow) {
          const notes = notesIn(folderId);
          if (!notes.some(note => note.id === getBook().data.activeId)) await actions.select(notes[0]?.id ?? null);
        }
        refresh();
      }
    } catch (error) { actions.error(error instanceof Error ? error.message : String(error)); }
    finally {
      document.body.dataset.libraryBusy = 'false'; shell.inert = false;
      if (focusedNote) {
        const row = noteRows().find(row => row.dataset.noteId === focusedNote) ?? list.querySelector<HTMLElement>('[aria-selected=true]');
        row?.focus({ preventScroll: true });
      }
    }
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
    } finally { document.body.dataset.libraryBusy = 'false'; shell.inert = false; }
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
    let restoreFooterFocus: HTMLElement | undefined;
    const wideList = innerWidth > 900 && layout.columns === 'list';
    const showServices = innerWidth > 900 && layout.columns !== 'note';
    if (desktopWeb) {
      const servicesOwner = wideList ? notesPane : folderPane;
      if (services.parentElement !== servicesOwner) {
        servicesOwner.append(services);
        if (showServices && active instanceof HTMLElement && services.contains(active)) restoreFooterFocus = active;
      }
      shell.toggleAttribute('data-web-sidebar-services', showServices);
      const owner = showServices ? services : footer;
      if (logout.parentElement !== owner) {
        if (showServices) owner.append(logout);
        else owner.prepend(logout);
        if (active === logout && layout.columns !== 'note') restoreFooterFocus = logout;
        else if (active === account && layout.columns === 'list') restoreFooterFocus = settings;
        else if (active === settings && showServices) restoreFooterFocus = account;
      }
    }
    resizing.update();
    const toolbarOwner = wideList ? notesToolbar : folderPane;
    const restoreToolFocus = folderToolbar.parentElement !== toolbarOwner && folderToolbar.contains(active) && (wideList || layout.columns === 'all');
    if (folderToolbar.parentElement !== toolbarOwner) toolbarOwner.prepend(folderToolbar);
    const parent = titlebarTools ?? (externalWindow ? shell : innerWidth <= 900
      ? layout.columns === 'note' ? $('.titlebar') : $('.notes-navigation')
      : layout.columns === 'note' ? $('.titlebar') : folderToolbar);
    if (toggle.parentElement !== parent) {
      parent.prepend(toggle);
    }
    if (externalWindow) {
      toggle.ariaLabel = toggle.title = translatedLabel('切换笔记列表');
      toggle.setAttribute('aria-expanded', String(layout.columns === 'list'));
      toggle.setAttribute('aria-controls', 'notes-pane');
      toggle.removeAttribute('aria-pressed');
    } else if (innerWidth > 900) {
      const showFolders = layout.columns === 'all';
      toggle.ariaLabel = toggle.title = translatedLabel(showFolders ? '隐藏文件夹栏' : layout.columns === 'list' ? '显示文件夹栏' : '显示笔记列表');
      toggle.setAttribute('aria-expanded', String(showFolders));
      toggle.setAttribute('aria-controls', layout.columns === 'note' ? 'library-sidebar' : 'folder-pane');
      toggle.removeAttribute('aria-pressed');
    } else {
      toggle.ariaLabel = toggle.title = translatedLabel('切换笔记列表');
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
    if (restoreFooterFocus) restoreFooterFocus.focus({ preventScroll: true });
    else if (restoreToolFocus && active instanceof HTMLElement) active.focus({ preventScroll: true });
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
  let searchFrom: MobilePage | undefined;
  function endMobileSearch(goBack: boolean) {
    if (!shell.hasAttribute('data-mobile-search')) return false;
    const from = searchFrom;
    searchFrom = undefined;
    shell.removeAttribute('data-mobile-search');
    search.value = ''; search.blur(); delete notesPane.dataset.searching;
    refresh();
    if (goBack && from === 'folders' && page === 'notes') navigate('folders');
    return true;
  }
  function renderMobile(next: MobilePage, animate = false) {
    const previous = page;
    if (next === 'folders') endMobileSearch(false);
    const active = document.activeElement;
    page = next;
    if (page !== 'folders' && editingFolders()) setFolderEditing(false);
    const wide = Boolean(tablet?.matches);
    shell.toggleAttribute('data-mobile-motion', animate);
    shell.toggleAttribute('data-mobile-wide', wide);
    shell.dataset.mobilePage = page;
    folderPane.inert = page !== 'folders';
    notesPane.inert = page === 'folders' || !wide && page === 'note';
    documentColumn.inert = !wide && page !== 'note';
    toggle.hidden = wide;
    if (active === search && page === 'notes') return;
    if (page !== previous || documentColumn.inert && documentColumn.contains(active) || notesPane.inert && notesPane.contains(active)) {
      if (page === 'folders') folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]')?.focus({ preventScroll: true });
      else if (page === 'notes' || wide) {
        const selected = page === 'note' || previous === 'note' ? list.querySelector<HTMLElement>('[aria-selected=true]') : null;
        const target = selected ?? foldersBack;
        target.focus({ preventScroll: true });
      } else toggle.focus({ preventScroll: true });
    }
  }
  function setFolderEditing(editing: boolean) {
    if (!editFolders) return;
    folderPane.toggleAttribute('data-editing-folders', editing);
    editFolders.replaceChildren(editing ? createLucideIcon(document, Check, '') : '编辑');
    editFolders.ariaLabel = editing ? translatedLabel('完成编辑') : translatedLabel('编辑文件夹');
    refresh();
  }
  const routeState = (next: MobilePage) => ({ qingyuNavigation: { page: next, folderId, trash } satisfies MobileRoute });
  function navigate(next: MobilePage) {
    if (page === next) return;
    if (pageDepth[next] < pageDepth[page]) { history.go(pageDepth[next] - pageDepth[page]); return; }
    if (page === 'folders' && next === 'note') history.pushState(routeState('notes'), '');
    history.pushState(routeState(next), '');
    renderMobile(next, true);
  }
  const changeLayout = (command: SidebarCommand) => {
    if (externalWindow && command === 'folders') return;
    applyLayout(nextSidebarLayout(layout, externalWindow ? 'notes' : command));
  };
  const resizing = setupSidebarResize(shell, () => layout, applyLayout, actions.error, externalWindow, () => autoHidden ? 'all' : layout.columns);
  if (!mobile && innerWidth > 900 && !actions.noteWindow) layout = { ...layout, columns: resizing.columns };
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
    icon.className = 'sidebar-icon'; icon.ariaHidden = 'true';
    if (mobile) icon.append(createLucideIcon(document, isTrash ? Trash2 : id === null ? Inbox : FolderIcon, ''));
    else icon.innerHTML = id === null ? allIcon : folderIcon;
    const text = document.createElement('span'); text.className = 'folder-name'; text.textContent = name;
    const total = document.createElement('span'); total.className = 'folder-count'; total.textContent = !mobile && count === 0 ? '' : String(count);
    button.append(icon, text, total);
    if (editingFolders()) {
      if (!id) button.disabled = true;
      else {
        button.ariaLabel = formatLabel('编辑文件夹 {name}', { name });
        button.setAttribute('aria-haspopup', 'menu');
        const more = document.createElement('span'); more.className = 'folder-edit-more'; more.ariaHidden = 'true';
        more.innerHTML = '<svg viewBox="0 0 24 24"><use href="#toolbar-ellipsis"/></svg>';
        button.append(more);
      }
    }
    button.onclick = async event => {
      if (id && editingFolders()) {
        await openFolderMenu(button, { anchor: button, keyboard: event.detail === 0 });
        return;
      }
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
  const thumbnailCleanups: (() => void)[] = [];
  function refresh() {
    clearTimeout(searchTimer); searchTimer = undefined; lastSearchRefresh = performance.now();
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
    if (editFolders) editFolders.hidden = getBook().external;
    if (treeRoot !== getBook().root) {
      treeRoot = getBook().root;
      expanded = new Set(readFolderIds(expansionKey())); folderOrder = readFolderIds(folderOrderKey());
    }
    if (folderId && !getBook().data.folders.some(folder => folder.id === folderId)) { folderId = null; trash = false; }
    const previous = folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]');
    if (folderId && previous?.dataset.folderId === folderId && previous.dataset.folderPath !== getBook().data.folders.find(folder => folder.id === folderId)?.path) revealAncestors(folderId);
    const focusedNote = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-note-id]')?.dataset.noteId;
    // 正在侧栏里改文件夹名时不重建列表，否则输入框会被换掉；改完会再刷新一次。
    const renaming = Boolean(folders.querySelector('.renaming:not([data-committing]) .rename-folder'));
    if (!renaming) folders.replaceChildren(folderRow(null, uiText('allNotes'), getBook().data.notes.filter(note => !note.trashed).length));
    const counts = folderNoteCounts(getBook().data);
    if (!renaming) for (const { folder, depth, hasChildren } of folderRows(getBook().data.folders, expanded, folderOrder)) {
      const entry = document.createElement('div'); entry.className = 'folder-entry';
      const row = folderRow(folder.id, folder.name, counts.get(folder.id) ?? 0);
      row.style.setProperty('--folder-depth', String(depth)); entry.style.setProperty('--folder-depth', String(depth));
      entry.append(row);
      if (hasChildren) {
        row.setAttribute('aria-expanded', String(expanded.has(folder.id)));
        const disclosure = document.createElement('button'); disclosure.type = 'button'; disclosure.className = 'folder-disclosure';
        disclosure.ariaLabel = formatLabel(expanded.has(folder.id) ? '折叠文件夹 {name}' : '展开文件夹 {name}', { name: folder.name }); disclosure.setAttribute('aria-expanded', String(expanded.has(folder.id)));
        disclosure.innerHTML = '<svg viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m4 2 4 4-4 4"/></svg>';
        disclosure.onclick = () => { void toggleFolder(folder.id); };
        entry.append(disclosure);
      }
      if (!mobile && !editingFolders() && !getBook().external) {
        // Pinto 的 RowMenuButton：鼠标移到行上时，计数的位置换成「⋯」，打开和右键相同的菜单。
        const more = document.createElement('button'); more.type = 'button'; more.className = 'folder-row-more';
        more.tabIndex = -1; more.ariaHidden = 'true'; more.title = translatedLabel('文件夹选项');
        more.innerHTML = '<svg viewBox="0 0 17 17" aria-hidden="true"><circle cx="8.5" cy="8.5" r="7.6" fill="none" stroke="currentColor" stroke-width="1.1"/><circle cx="5.1" cy="8.5" r="1.05" fill="currentColor"/><circle cx="8.5" cy="8.5" r="1.05" fill="currentColor"/><circle cx="11.9" cy="8.5" r="1.05" fill="currentColor"/></svg>';
        more.onclick = event => openFolderMenu(row, { anchor: more, keyboard: event.detail === 0 });
        entry.append(more);
      }
      folders.append(entry);
    }
    if (!renaming) {
      const trashRow = folderRow(null, uiText('trash'), getBook().data.notes.filter(note => note.trashed).length, true);
      trashRow.dataset.trash = 'true'; folders.append(trashRow);
    }
    const notes = notesIn(folderId);
    const scope = JSON.stringify([getBook().root, folderId, trash, search.value]);
    if (selectionBook !== getBook() || selectionScope !== scope || selectionActive !== getBook().data.activeId) {
      selectedNotes = new Set([getBook().data.activeId]);
      selectionAnchor = getBook().data.activeId;
    }
    selectionBook = getBook(); selectionScope = scope; selectionActive = getBook().data.activeId;
    const visibleIds = new Set(notes.map(note => note.id));
    selectedNotes = new Set([...selectedNotes].filter(id => visibleIds.has(id)));
    if (selectionAnchor && !visibleIds.has(selectionAnchor)) selectionAnchor = null;
    const name = trash ? uiText('trash') : getBook().data.folders.find(folder => folder.id === folderId)?.name ?? uiText('allNotes');
    $('#notes-heading').textContent = name;
    const locale = document.documentElement.lang || 'zh-CN';
    $('#notes-count').textContent = uiText('notesCount').replace('{count}', notes.length.toLocaleString(locale));
    for (const cleanup of thumbnailCleanups.splice(0)) cleanup();
    list.replaceChildren();
    list.toggleAttribute('data-show-folders', !folderId);
    const archived = getBook().trashedFolders;
    // 移到笔记库废纸篓的文件夹：App 移动端和网页客户端都在这里恢复（桌面 App 用的是系统废纸篓）
    if ((mobile || getBook().server) && trash && archived.length) {
      const group = document.createElement('section'), heading = document.createElement('h3');
      heading.className = 'notes-year'; heading.textContent = uiText('folders');
      group.append(heading);
      for (const name of archived) {
        if (search.value && !name.toLocaleLowerCase().includes(search.value.trim().toLocaleLowerCase())) continue;
        const row = document.createElement('div'), title = document.createElement('span'), restore = document.createElement('button');
        row.className = 'trash-folder-row';
        title.textContent = name;
        restore.type = 'button'; restore.className = 'button quiet-button button-touch';
        restore.textContent = translatedLabel('恢复文件夹'); restore.ariaLabel = formatLabel('恢复文件夹 {name}', { name });
        restore.onclick = () => {
          void changeFolder(() => getBook().folderAction('restore-folder', null, name))
            .catch(reason => actions.error(reason instanceof Error ? reason.message : String(reason)));
        };
        row.append(createLucideIcon(document, FolderIcon, 'sidebar-icon'), title, restore);
        group.append(row);
      }
      if (group.childElementCount > 1) list.append(group);
    }
    const date = noteListDate(now, locale);
    const fullDate = new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' });
    const hideSummary = localStorage.getItem(hideSummaryKey) === 'true';
    for (const { label, notes: grouped } of groupNotes(notes, { ...order, grouped: groupsByDate(groupingKey()) }, now, locale)) {
      let previous: number | undefined;
      const group = document.createElement('section'); group.role = 'group'; group.setAttribute('aria-label', label);
      if (label) {
        const heading = document.createElement('h3'); heading.className = 'notes-year'; heading.textContent = label;
        group.append(heading);
      }
      for (const note of grouped) {
        const row = document.createElement('button');
        row.className = 'note-row'; row.role = 'option'; row.dataset.noteId = note.id;
        row.setAttribute('aria-selected', String(selectedNotes.has(note.id)));
        const title = document.createElement('span'); title.className = 'note-row-title'; title.textContent = noteTitle(note) + (note.conflictOf ? '（冲突副本）' : '');
        const info = document.createElement('span'); info.className = 'note-row-info';
        const time = document.createElement('time'), timestamp = order.key === 'createdAt' ? note.createdAt : note.updatedAt;
        time.dateTime = new Date(timestamp).toISOString(); time.textContent = date(timestamp, previous);
        time.title = fullDate.format(timestamp); time.setAttribute('aria-label', time.title);
        if (!mobile && label && order.key !== 'title') previous = timestamp;
        // 参照 Pinto 列表：已分享的笔记在日期前放一个弱化的链接图标。
        const stamp = document.createElement('span'); stamp.className = 'note-row-date';
        if (isShared(note)) {
          stamp.append(createLucideIcon(document, LinkIcon, 'note-row-shared'));
          row.dataset.shared = ''; row.setAttribute('aria-description', '已分享');
        }
        stamp.append(time);
        // 和 Pinto 一样分三行：标题独占一行，第二行是时间和摘要，第三行是所在文件夹。
        // 桌面上时间和摘要之间只留空隙，手机上保留「·」。
        row.append(title); info.append(stamp);
        if (!hideSummary) {
          if (mobile) {
            const separator = document.createElement('span'); separator.ariaHidden = 'true'; separator.textContent = '·'; info.append(separator);
          }
          const summary = document.createElement('span'); summary.className = 'note-row-summary'; summary.textContent = noteSummary(note);
          info.append(summary);
        }
        row.append(info);
        // 已经在这个文件夹里浏览时，文件夹那一行就不用再显示了。
        if (mobile || folderId !== note.folderId) {
          const location = document.createElement('span'); location.className = 'note-row-folder';
          const icon = document.createElement('span'); icon.className = 'sidebar-icon'; icon.ariaHidden = 'true'; icon.innerHTML = folderIcon;
          const folderLabel = document.createElement('span'); folderLabel.textContent = getBook().data.folders.find(folder => folder.id === note.folderId)!.name;
          location.append(icon, folderLabel); row.append(location);
        }
        const picture = actions.thumbnail?.(note);
        if (picture) {
          const image = document.createElement('img'); image.className = 'note-row-thumbnail';
          image.alt = ''; image.loading = 'lazy'; image.decoding = 'async'; image.draggable = false;
          // 图片读不到时只去掉缩略图，行本身照常显示。
          image.onerror = () => { image.remove(); delete row.dataset.thumbnail; };
          row.dataset.thumbnail = ''; row.dataset.picture = picture; row.append(image);
          const cleanup = actions.mountThumbnail?.(note, image, row, picture);
          if (cleanup) thumbnailCleanups.push(cleanup);
          else image.src = picture;
        }
        row.onclick = async event => {
          if (event.shiftKey || event.metaKey || event.ctrlKey) {
            event.preventDefault(); extendSelection(note.id, event); return;
          }
          // 先把高亮画出来再换编辑器内容，点下去马上有反馈；切换失败再退回原来的选中。
          const previous = selectedNotes, previousAnchor = selectionAnchor, before = getBook().data;
          selectedNotes = new Set([note.id]); selectionAnchor = note.id; renderSelection();
          await new Promise<void>(resolve => requestAnimationFrame(() => setTimeout(resolve, 0)));
          if (!await actions.select(note.id)) {
            if (selectedNotes.size === 1 && selectedNotes.has(note.id)) { selectedNotes = previous; selectionAnchor = previousAnchor; renderSelection(); }
            return;
          }
          selectedNotes = new Set([note.id]); selectionAnchor = note.id;
          const after = getBook().data;
          // 只换了当前笔记时列表内容没变，不用整列重建（几百行加缩略图）。
          if (after.notes === before.notes && after.folders === before.folders && displayedLibrary === before) {
            displayedLibrary = after; selectionActive = after.activeId; renderSelection();
            const location = document.querySelector('#note-location');
            if (location) location.textContent = after.folders.find(folder => folder.id === getBook().current?.folderId)?.name ?? '';
          } else refresh();
          if (!mobile || tablet?.matches) list.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(note.id)}"]`)?.focus({ preventScroll: true });
          if (mobile) { if (page !== 'folders') navigate('note'); return; }
          if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
        };
        row.oncontextmenu = event => {
          event.preventDefault();
          void openNoteRowMenu(note.id, event.clientX, event.clientY, { preview: touchMenus.has(event) });
        };
        group.append(row);
      }
      list.append(group);
    }
    if (!notes.length) {
      const empty = document.createElement('p'); empty.className = 'notes-empty';
      empty.textContent = uiText(search.value.trim() ? 'noMatch' : trash ? 'emptyTrash' : 'noNotes'); list.append(empty);
    }
    if (focusedNote) list.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(focusedNote)}"]`)?.focus({ preventScroll: true });
    if (mobile) { list.scrollTop = listTop; folders.scrollTop = foldersTop; }
  }
  const newNoteFolder = (target = folderId) => target ?? getBook().data.folders[0].id;
  // 和 Pinto 一样：新笔记建好就打开，光标在正文里，标题就是正文第一行。
  async function newNote(title = '', targetFolder = folderId) {
    if (!await folderMenu.finishRename()) return;
    if (!title && actions.blankNote?.()) {
      if (mobile) navigate('note');
      actions.focusEditor?.();
      return;
    }
    if (!await actions.create(newNoteFolder(targetFolder), title)) return;
    folderId = targetFolder; trash = false; search.value = '';
    if (mobile) {
      refresh(); navigate('note');
      actions.focusEditor?.();
    } else {
      refresh(); list.scrollTop = 0;
      if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
    }
  }
  async function newFolder(parentId = 'default') {
    if (getBook().external) { actions.error('临时文件模式不管理文件夹，请返回笔记库操作'); return; }
    if (!await folderMenu.finishRename()) return;
    if (!await actions.save()) return;
    newFolderParent = parentId;
    if (!mobile) {
      // 和 Pinto 一样：先建一个「新建文件夹」，选中它，并在侧栏里直接改名（名称已全选）。
      const { folders: all } = getBook().data;
      const parent = all.find(folder => folder.id === parentId);
      const prefix = [parent?.path ?? (parentId === 'default' ? '' : parent?.name)].filter(Boolean).map(path => `${path}/`).join('');
      const taken = new Set(all.map(folder => (folder.path ?? folder.name).toLocaleLowerCase()));
      let name = '新建文件夹';
      for (let count = 2; taken.has((prefix + name).toLocaleLowerCase()); count++) name = `新建文件夹 ${count}`;
      let created = '';
      try {
        await changeFolder(async () => { created = await getBook().createFolder(name, parentId); });
      } catch (reason) { actions.error(reason instanceof Error ? reason.message : String(reason)); return; }
      if (!created) return;
      revealAncestors(created);
      if (await selectFolder(created)) folderMenu.rename(created);
      return;
    }
    $('#folder-dialog-title').textContent = parentId === 'default' ? translatedLabel('app.newMarkdownFolder') : translatedLabel('新建子文件夹');
    folderError.textContent = ''; folderName.value = ''; createFolder.disabled = true; dialog.showModal(); folderName.focus();
  }
  $('#folder-form').onsubmit = async event => {
    event.preventDefault();
    if (!await actions.save()) return;
    try {
      await changeFolder(async () => { await getBook().createFolder(folderName.value, newFolderParent); });
      dialog.close();
    } catch (error) {
      folderError.textContent = error instanceof DOMException ? translatedLabel('保存失败，文件夹尚未创建') : error instanceof Error ? error.message : '文件夹创建失败';
      folderName.focus();
    }
  };
  $('#cancel-folder').onclick = () => dialog.close();
  $('#new-folder-button').onclick = () => { void newFolder(); };
  const listMore = $<HTMLButtonElement>('#list-more-button');
  listMore.onclick = event => {
    const keyForGrouping = groupingKey();
    const currentGrouping = groupsByDate(keyForGrouping);
    const update = (patch: Partial<NoteOrder>) => {
      const next = { ...order, ...patch };
      localStorage.setItem(orderKey, JSON.stringify(next)); order = next;
      refresh(); list.scrollTop = 0;
    };
    void showMenu([
      { text: '排序方式', icon: createLucideIcon(document, ArrowUpDown, ''), items: [
        ...([['updatedAt', '修改日期'], ['createdAt', '创建日期'], ['title', '标题']] as const)
          .map(([key, text]) => ({ text, checked: order.key === key, run: () => update({ key }) })),
        { text: '' },
        ...[false, true].map(reverse => ({ text: (order.key === 'title' ? ['升序', '降序'] : ['最新在前', '最早在前'])[Number(reverse)], checked: order.reverse === reverse, run: () => update({ reverse }) })),
      ] },
      { text: '按日期分组', icon: createLucideIcon(document, CalendarDays, ''), enabled: order.key !== 'title', items: [true, false].map(grouped =>
        ({ text: grouped ? '开启' : '关闭', checked: currentGrouping === grouped, run: () => {
          localStorage.setItem(keyForGrouping, String(grouped));
          refresh();
        } })) },
      { text: uiText('hideNoteSummaries'), checked: localStorage.getItem(hideSummaryKey) === 'true', run: () => {
        localStorage.setItem(hideSummaryKey, String(localStorage.getItem(hideSummaryKey) !== 'true')); refresh();
      } },
      ...(!mobile && !getBook().external ? [{ text: '' }, { text: '新建文件夹', icon: createLucideIcon(document, FolderPlus, ''), run: () => newFolder() }] : []),
    ], { anchor: listMore, keyboard: event.detail === 0, label: '列表选项', onError: actions.error });
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
  const edge = document.createElement('glass-progressive-blur');
  edge.className = 'notes-scroll-edge';
  edge.setAttribute('edge', 'top'); edge.setAttribute('blur', '20'); edge.setAttribute('layers', '6');
  const wash = document.createElement('div'); wash.className = 'notes-scroll-wash'; wash.ariaHidden = 'true';
  notesToolbar.before(edge, wash);
  if (!mobile) {
    notesToolbar.prepend($('.notes-summary'));
    const scrollTimers = new WeakMap<Element, ReturnType<typeof setTimeout>>();
    shell.addEventListener('scroll', event => {
      const pane = event.target;
      if (!(pane instanceof HTMLElement) || !pane.matches('#folder-list, #notes-list, #document-column .cm-scroller')) return;
      pane.toggleAttribute('data-scrolling', true);
      clearTimeout(scrollTimers.get(pane));
      scrollTimers.set(pane, setTimeout(() => pane.removeAttribute('data-scrolling'), 700));
    }, { capture: true, passive: true });
    let frame = 0;
    const markStuck = () => {
      frame = 0;
      const top = list.getBoundingClientRect().top + parseFloat(getComputedStyle(list).paddingTop);
      // 所有分组标题吸在同一处：到顶的标题里最后一个是当前标题，前面的被它盖住。
      // 还没滚动时不算吸顶，标题只是普通的一行。
      const headings = [...list.querySelectorAll<HTMLElement>('.notes-year')];
      const atTop = headings.filter(heading => list.scrollTop > 0
        && heading.getBoundingClientRect().top <= top + (parseFloat(getComputedStyle(heading).top) || 0) + .5);
      const current = atTop.at(-1);
      for (const heading of headings) {
        heading.toggleAttribute('data-stuck', heading === current);
        heading.toggleAttribute('data-covered', heading !== current && atTop.includes(heading));
      }
      const pinned = current?.getBoundingClientRect();
      // 和 Pinto 一样，标题栏和吸顶的分组标题共用一片渐进模糊，延伸到细线下方 14px。
      if (pinned) notesPane.style.setProperty('--notes-edge', `${Math.round(pinned.bottom - notesPane.getBoundingClientRect().top + 14)}px`);
      else notesPane.style.removeProperty('--notes-edge');
    };
    list.addEventListener('scroll', () => { frame ||= requestAnimationFrame(markStuck); }, { passive: true });
    new MutationObserver(() => { frame ||= requestAnimationFrame(markStuck); }).observe(list, { childList: true });
  }
  toggle.onclick = event => {
    if (shell.inert) return;
    if (mobile) return navigate('notes');
    changeLayout(externalWindow || innerWidth <= 900 ? 'cycle' : layout.columns === 'note' ? 'notes' : 'folders');
    if (externalWindow && event.detail > 0) toggle.blur();
  };
  foldersBack.onclick = () => navigate('folders');
  toggle.onpointerdown = event => event.preventDefault();
  $('#sidebar-backdrop').onclick = () => changeLayout('notes');
  // 搜索每输入一个字都要重建列表。空闲时第一下立即刷新，连续输入时最多每 100ms 刷新一次，最后一下一定会刷新。
  search.oninput = event => {
    if ((event as InputEvent).isComposing) return;
    const wait = lastSearchRefresh + 100 - performance.now();
    if (wait <= 0) refresh();
    else { clearTimeout(searchTimer); searchTimer = setTimeout(refresh, wait); }
  };
  // compositionend also covers IMEs that do not emit a final input event.
  search.addEventListener('compositionend', refresh);
  // Keep WebKit from blurring/saving and replacing the clicked row before its click arrives.
  $('#library-sidebar').addEventListener('pointerdown', event => {
    if (event.pointerType !== 'touch' && !(event.target as HTMLElement).closest('input') && (event.target as HTMLElement).closest('button')) event.preventDefault();
  });
  const touchMenus = new WeakSet<Event>();
  async function openNoteRowMenu(id: string, x: number, y: number, surface: { preview?: boolean; keyboard?: boolean }) {
    if (document.body.dataset.libraryBusy === 'true' || !await actions.save()) return;
    if (!selectedNotes.has(id)) {
      selectedNotes = new Set([id]); selectionAnchor = id; renderSelection();
    }
    await contextMenu(id, x, y, surface);
  }
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
        row.dispatchEvent(new PointerEvent('contextmenu', { bubbles: true, cancelable: true, clientX: x, clientY: y, pointerType: 'touch' }));
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
      if (touch) { touchMenus.add(event); held = true; }
    }, true);
    document.addEventListener('click', event => {
      if (held && event.detail) { held = false; event.preventDefault(); event.stopImmediatePropagation(); }
    }, true);
  }
  $('#folder-pane').oncontextmenu = async event => {
    if (getBook().external) { event.preventDefault(); return; }
    if ((event.target as HTMLElement).closest('input')) return;
    event.preventDefault();
    if ((event.target as HTMLElement).closest('.sidebar-brand, .folder-toolbar')) return;
    const row = (event.target as HTMLElement).closest<HTMLElement>('.folder-row') ?? (event.target as HTMLElement).closest('.folder-entry')?.querySelector<HTMLElement>('.folder-row');
    await openFolderMenu(row, { at: { x: event.clientX, y: event.clientY } }, touchMenus.has(event));
  };
  async function openFolderMenu(row: HTMLElement | null | undefined, position: Pick<MenuOptions, 'anchor' | 'at' | 'keyboard'>, preview = false) {
    if (getBook().external || editingFolders() && !row?.dataset.folderId) return;
    if (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') return;
    const id = row?.dataset.folderId || null;
    if (!await folderMenu.finishRename() || !await actions.save()) return;
    const target = getBook().data.folders.find(folder => folder.id === id);
    const type = row?.dataset.trash === 'true' ? 'trash' : !target ? 'all' : target.id === 'default' || target.path === '' ? 'root' : 'folder';
    await folderMenu.open(id, type, Boolean(row), position, editingFolders(), preview);
  }
  $('#notes-pane').addEventListener('contextmenu', event => {
    if (!(event.target as HTMLElement).closest('input')) event.preventDefault();
  });
  document.addEventListener('contextmenu', event => { if (document.body.dataset.libraryBusy === 'true') event.preventDefault(); }, true);
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
      event.preventDefault();
      const box = row.getBoundingClientRect();
      void openFolderMenu(row, { at: { x: box.left + 20, y: box.top + 20 }, keyboard: true });
      return;
    }
    const command = event.metaKey || event.ctrlKey;
    if (event.key === 'F6' && event.shiftKey || command && getBook().root && (event.altKey && event.key.toLowerCase() === 'r' || event.key.toLowerCase() === 'j' || event.shiftKey && event.key === 'Backspace')) {
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
    if (event.isComposing) return;
    if ((event.key === 'F10' && event.shiftKey) || event.key === 'ContextMenu') {
      const row = document.activeElement?.closest<HTMLElement>('[data-note-id]');
      if (row) {
        event.preventDefault(); const box = row.getBoundingClientRect();
        void openNoteRowMenu(row.dataset.noteId!, box.left + 20, box.top + 20, { keyboard: true });
      }
      return;
    }
    const rows = [...list.querySelectorAll<HTMLButtonElement>('[data-note-id]')];
    const index = rows.findIndex(row => row === document.activeElement);
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || !rows.length) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    if (event.shiftKey) {
      extendSelection(rows[next].dataset.noteId!, event);
      rows[next].scrollIntoView?.({ block: 'nearest' });
    } else if (mobile && !tablet?.matches) rows[next].focus(); else rows[next].click();
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
  /**
   * 保存一篇笔记后只更新它那一行。列表顺序、分组、缩略图都没变时就地改标题、时间和摘要，
   * 否则整列刷新。打字时每次自动保存都会走这里，不能每次都把几百行重建一遍。
   */
  function noteSaved(id: string) {
    const book = getBook(), note = book.data.notes.find(entry => entry.id === id);
    const row = list.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(id)}"]`);
    if (!note || !row || actions.noteWindow || search.value.trim()) { refresh(); return; }
    const now = new Date(), locale = document.documentElement.lang || 'zh-CN';
    if (now.toDateString() !== listDay) { refresh(); return; }
    const groups = groupNotes(notesIn(folderId), { ...order, grouped: groupsByDate(groupingKey()) }, now, locale);
    const expected = groups.flatMap(({ label, notes }) => notes.map(entry => `${label}\n${entry.id}`));
    const shown = noteRows().map(entry => `${entry.closest('section')?.getAttribute('aria-label') ?? ''}\n${entry.dataset.noteId}`);
    const picture = actions.thumbnail?.(note) ?? '';
    if (expected.join('\0') !== shown.join('\0') || (row.dataset.picture ?? '') !== picture) {
      refresh(); return;
    }
    if (displayedLibrary !== book.data) { displayedLibrary = book.data; actions.libraryChanged?.(); }
    const title = row.querySelector('.note-row-title');
    if (title) title.textContent = noteTitle(note) + (note.conflictOf ? '（冲突副本）' : '');
    const summary = row.querySelector('.note-row-summary');
    if (summary) summary.textContent = noteSummary(note);
    // 下一行的时间要和这一行比是不是同一天，所以一起重算。
    const group = groups.find(({ notes }) => notes.some(entry => entry.id === id))!;
    const index = group.notes.findIndex(entry => entry.id === id);
    const stampOf = (entry: Note) => order.key === 'createdAt' ? entry.createdAt : entry.updatedAt;
    const date = noteListDate(now, locale), full = new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' });
    for (const position of [index, index + 1]) {
      const entry = group.notes[position];
      const time = entry && list.querySelector<HTMLTimeElement>(`[data-note-id="${CSS.escape(entry.id)}"] time`);
      if (!time) continue;
      const before = !mobile && group.label && order.key !== 'title' && position > 0 ? stampOf(group.notes[position - 1]) : undefined;
      const timestamp = stampOf(entry);
      time.dateTime = new Date(timestamp).toISOString(); time.textContent = date(timestamp, before);
      time.title = full.format(timestamp); time.setAttribute('aria-label', time.title);
    }
  }
  refresh();
  return {
    noteSaved,
    updateDraftTitle(note: Note) {
      const title = list.querySelector(`[data-note-id="${CSS.escape(note.id)}"] .note-row-title`);
      if (title) title.textContent = noteTitle(note) + (note.conflictOf ? '（冲突副本）' : '');
    },
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
    showCurrentMenu(anchor: HTMLButtonElement, keyboard: boolean) { const note = getBook().current; if (note) return contextMenu(note.id, 0, 0, { anchor, keyboard }); },
    showEditorMenu(id: string, event: MouseEvent, editorItems: MenuItem[]) {
      if (getBook().current?.id === id && !getBook().loadError && document.body.dataset.libraryBusy !== 'true') {
        return contextMenu(id, event.clientX, event.clientY, { editorItems, restoreFocus: actions.focusEditor });
      }
    },
    moveCurrentNote() { const note = getBook().current; if (note) return contextMenu(note.id, 0, 0, 'move'); },
    revealCurrent() { folderId = null; trash = Boolean(getBook().current?.trashed); search.value = ''; refresh(); },
    /** 安卓返回键：搜索中先退出搜索。 */
    cancelSearch: () => mobile && page !== 'note' && endMobileSearch(true),
  };
}
