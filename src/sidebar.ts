import './sidebar.css';
import brandIcon from '../public/brand/qingyu-mark.svg?raw';
import allIcon from '../public/miaoyan/icons/allNotes.svg?raw';
import folderIcon from '../public/miaoyan/icons/folder.svg?raw';
import addFolderIcon from '../public/miaoyan/icons/newFolder.svg?raw';
import newNoteIcon from '../public/miaoyan/icons/newNote.svg?raw';
import sidebarIcon from '../public/miaoyan/icons/sidebar.svg?raw';
import { groupNotes, noteTitle, visibleNotes, type Notebook } from './notebook';
import { noteText } from './document-text';
import { nextSidebarLayout, type SidebarCommand, type SidebarLayout } from './sidebar-layout';
import { createNoteMenu } from './note-menu';
import { createFolderMenu } from './folder-menu';
import { folderRows } from './folder-tree';
import { setupSidebarResize } from './sidebar-resize';

export function createSidebar(getBook: () => Notebook, actions: {
  select(id: string | null): Promise<boolean>;
  create(folderId: string, title?: string): Promise<boolean>;
  save(): Promise<boolean>;
  reload(): void;
  error(message: string): void;
}) {
  const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
  const shell = $('#app-shell'), list = $('#notes-list'), folders = $('#folder-list');
  const search = $<HTMLInputElement>('#note-search'), dialog = $<HTMLDialogElement>('#folder-dialog');
  const folderName = $<HTMLInputElement>('#folder-name'), folderError = $('#folder-error');
  const date = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'numeric', day: 'numeric' });
  let folderId: string | null = null;
  let trash = Boolean(getBook().current?.trashed);
  let newFolderParent = 'default';
  let treeRoot = getBook().root;
  const expansionKey = () => `miaoyan-editor.folder-tree.v1:${getBook().root ?? 'web'}`;
  function readExpansion(): Set<string> {
    try { const saved: unknown = JSON.parse(localStorage.getItem(expansionKey()) ?? '[]'); return new Set(Array.isArray(saved) ? saved.filter((id): id is string => typeof id === 'string') : []); }
    catch { return new Set(); }
  }
  let expanded = readExpansion();
  function saveExpansion() { try { localStorage.setItem(expansionKey(), JSON.stringify([...expanded])); } catch { actions.error('文件夹展开状态未能保存'); } }
  let layout: SidebarLayout = { columns: innerWidth > 900 ? 'all' : 'note', unfolding: false };
  $('#sidebar-brand-icon').innerHTML = brandIcon;
  $('#new-folder-button').innerHTML = addFolderIcon;
  $('#new-note-button').innerHTML = newNoteIcon;
  const toggle = $('#sidebar-button');
  toggle.innerHTML = sidebarIcon;
  const contextMenu = createNoteMenu(getBook, { save: actions.save, error: actions.error, async change(mutate, reload) {
    try {
      const focused = list.contains(document.activeElement);
      await mutate();
      if (reload) actions.reload();
      const notes = visibleNotes(getBook().data, folderId, search.value, trash);
      if (!notes.some(note => note.id === getBook().data.activeId)) await actions.select(notes[0]?.id ?? null);
      refresh();
      if (focused) list.querySelector<HTMLElement>('[aria-selected=true]')?.focus({ preventScroll: true });
    } catch (error) { actions.error(error instanceof Error ? error.message : String(error)); }
  } });
  const folderMenu = createFolderMenu(getBook, { create: newFolder, error: actions.error, change: changeFolder });
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
      const notes = visibleNotes(getBook().data, folderId, search.value, trash);
      if (!notes.some(note => note.id === getBook().data.activeId)) await actions.select(notes[0]?.id ?? null);
      refresh();
    } finally { document.body.dataset.libraryBusy = 'false'; shell.inert = document.body.dataset.syncBusy === 'true'; }
  }

  function applyLayout(next: SidebarLayout) {
    const restoringList = layout.columns === 'note' && next.columns !== 'note';
    const active = document.activeElement;
    const hiddenFocus = next.columns === 'note' ? $('#library-sidebar').contains(active) : next.columns === 'list' && $('#folder-pane').contains(active);
    layout = next;
    shell.dataset.sidebar = layout.columns;
    resizing.update();
    const parent = innerWidth <= 900
      ? layout.columns === 'note' ? $('.titlebar') : $('.notes-navigation')
      : $('.actions');
    if (toggle.parentElement !== parent) {
      parent.prepend(toggle);
    }
    toggle.setAttribute('aria-expanded', String(layout.columns !== 'note'));
    // Original MiaoYan accents the control when the folders column is hidden.
    toggle.setAttribute('aria-pressed', String(layout.columns !== 'all'));
    const overlay = layout.columns !== 'note' && innerWidth <= 900;
    $('#workspace').inert = $('#empty-note').inert = overlay;
    for (const button of document.querySelectorAll<HTMLButtonElement>('.actions button')) if (button !== toggle) button.inert = overlay;
    if (restoringList) {
      const selected = list.querySelector<HTMLElement>('[aria-selected="true"]');
      selected?.scrollIntoView({ block: 'nearest' });
      if (selected) {
        const header = selected.parentElement?.querySelector('.notes-year');
        const overlap = list.getBoundingClientRect().top + (header?.getBoundingClientRect().height ?? 0) - selected.getBoundingClientRect().top;
        if (overlap > 0) list.scrollTop -= overlap;
      }
    }
    if (hiddenFocus || active === toggle) toggle.focus({ preventScroll: true });
  }
  const changeLayout = (command: SidebarCommand) => applyLayout(nextSidebarLayout(layout, command));
  const resizing = setupSidebarResize(shell, () => layout, applyLayout, actions.error, getBook().external);
  if (innerWidth > 900) layout = { ...layout, columns: resizing.columns };
  async function selectFolder(id: string | null, isTrash = false) {
    if (!await folderMenu.finishRename()) return false;
    const notes = visibleNotes(getBook().data, id, '', isTrash);
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
      if (!await selectFolder(id, isTrash)) return;
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
    $('#new-folder-button').hidden = getBook().external;
    if (treeRoot !== getBook().root) { treeRoot = getBook().root; expanded = readExpansion(); }
    if (folderId && !getBook().data.folders.some(folder => folder.id === folderId)) { folderId = null; trash = false; }
    const previous = folders.querySelector<HTMLElement>('.folder-row[aria-pressed=true]');
    if (folderId && previous?.dataset.folderId === folderId && previous.dataset.folderPath !== getBook().data.folders.find(folder => folder.id === folderId)?.path) revealAncestors(folderId);
    const focusedNote = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>('[data-note-id]')?.dataset.noteId;
    folders.replaceChildren(folderRow(null, '全部', getBook().data.notes.filter(note => !note.trashed).length));
    for (const { folder, depth, hasChildren } of folderRows(getBook().data.folders, expanded)) {
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
    trashRow.dataset.trash = 'true'; if (getBook().data.notes.some(note => note.trashed)) folders.append(trashRow);
    const notes = visibleNotes(getBook().data, folderId, search.value, trash);
    const name = trash ? '废纸篓' : getBook().data.folders.find(folder => folder.id === folderId)?.name ?? '全部';
    $('#notes-heading').textContent = name;
    $('#notes-count').textContent = `${notes.length} 篇笔记${search.value.trim() ? ' · 搜索结果' : ''}`;
    list.replaceChildren();
    for (const { label, notes: grouped } of groupNotes(notes)) {
      const group = document.createElement('section'); group.role = 'group'; group.setAttribute('aria-label', label);
      const heading = document.createElement('h3'); heading.className = 'notes-year'; heading.textContent = label;
      group.append(heading);
      for (const note of grouped) {
        const text = noteText(note.source), row = document.createElement('button');
        row.className = 'note-row'; row.role = 'option'; row.dataset.noteId = note.id;
        row.setAttribute('aria-selected', String(note.id === getBook().data.activeId));
        const title = document.createElement('span'); title.className = 'note-row-title'; title.textContent = noteTitle(note) + (note.conflictOf ? '（冲突副本）' : '');
        const info = document.createElement('span'); info.className = 'note-row-info';
        const time = document.createElement('time'); time.dateTime = new Date(note.updatedAt).toISOString(); time.textContent = date.format(note.updatedAt);
        const summary = document.createElement('span'); summary.className = 'note-row-summary'; summary.textContent = text.summary;
        info.append(time, summary);
        const location = document.createElement('span'); location.className = 'note-row-folder';
        const icon = document.createElement('span'); icon.className = 'sidebar-icon'; icon.ariaHidden = 'true'; icon.innerHTML = folderIcon;
        const label = document.createElement('span'); label.textContent = getBook().data.folders.find(folder => folder.id === note.folderId)!.name;
        location.append(icon, label); row.append(title, info, location);
        row.onclick = async () => {
          if (!await actions.select(note.id)) return;
          refresh();
          list.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(note.id)}"]`)?.focus({ preventScroll: true });
          if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
        };
        row.oncontextmenu = async event => {
          event.preventDefault();
          if (!await actions.select(note.id)) return;
          refresh();
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
  }
  async function newNote(title = '') {
    if (!await actions.create(folderId ?? getBook().data.folders[0].id, title)) return;
    trash = false; search.value = ''; refresh(); list.scrollTop = 0;
    if (innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
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
  $('#new-note-button').onclick = $('#new-empty-note').onclick = () => { void newNote(); };
  search.addEventListener('keydown', event => { if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); void newNote(search.value); } });
  toggle.onclick = () => changeLayout('cycle');
  toggle.onpointerdown = event => event.preventDefault();
  $('#sidebar-backdrop').onclick = () => changeLayout('notes');
  search.oninput = event => { if (!(event as InputEvent).isComposing) refresh(); };
  // compositionend also covers IMEs that do not emit a final input event.
  search.addEventListener('compositionend', refresh);
  // Keep WebKit from blurring/saving and replacing the clicked row before its click arrives.
  $('#library-sidebar').addEventListener('pointerdown', event => {
    if (!(event.target as HTMLElement).closest('input') && (event.target as HTMLElement).closest('button')) event.preventDefault();
  });
  $('#folder-pane').oncontextmenu = async event => {
    if (getBook().external) { event.preventDefault(); return; }
    if ((event.target as HTMLElement).closest('input')) return;
    event.preventDefault();
    if ((event.target as HTMLElement).closest('.sidebar-brand, .folder-toolbar') || document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') return;
    const row = (event.target as HTMLElement).closest<HTMLElement>('.folder-row') ?? (event.target as HTMLElement).closest('.folder-entry')?.querySelector<HTMLElement>('.folder-row');
    if (row) {
      const id = row.dataset.folderId || null, isTrash = row.dataset.trash === 'true';
      if (!await selectFolder(id, isTrash)) return;
    } else if (!await actions.save()) return;
    const selected = getBook().data.folders.find(folder => folder.id === folderId);
    const type = trash ? 'trash' : !selected ? 'all' : selected.id === 'default' || selected.path === '' ? 'root' : 'folder';
    await folderMenu.open(folderId, type, Boolean(row), event.clientX, event.clientY);
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
      const id = row.dataset.folderId, item = folderRows(getBook().data.folders, expanded).find(item => item.folder.id === id);
      if (event.key === 'ArrowLeft') {
        if (id && expanded.has(id)) void toggleFolder(id);
        else if (item?.parentId) folders.querySelector<HTMLButtonElement>(`.folder-row[data-folder-id="${CSS.escape(item.parentId)}"]`)?.click();
      } else if (event.key === 'ArrowRight') {
        if (item?.hasChildren && !expanded.has(item.folder.id)) void toggleFolder(item.folder.id);
      } else {
        const rows = [...folders.querySelectorAll<HTMLButtonElement>('.folder-row')], index = rows.indexOf(row as HTMLButtonElement);
        rows[event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1 : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))]?.click();
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
    rows[next].click();
  };
  document.addEventListener('keydown', event => {
    if (event.key === 'Escape' && !dialog.open && innerWidth <= 900 && layout.columns !== 'note') changeLayout('notes');
    if (event.isComposing || (!event.metaKey && !event.ctrlKey) || document.querySelector('dialog[open]')) return;
    if (event.key.toLowerCase() === 'n') { event.preventDefault(); if (event.shiftKey) newFolder(); else newNote(); }
    if (event.key === '1' || event.key === '2') { event.preventDefault(); changeLayout(event.key === '1' ? 'folders' : 'notes'); }
    if (event.key.toLowerCase() === 'f' && event.shiftKey) { event.preventDefault(); if (layout.columns === 'note') changeLayout('notes'); search.focus(); search.select(); }
  });
  window.addEventListener('resize', () => applyLayout(layout));
  applyLayout(layout); refresh();
  return { refresh, revealCurrent() { folderId = null; trash = Boolean(getBook().current?.trashed); search.value = ''; refresh(); } };
}
