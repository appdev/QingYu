import { noteTitle, type Notebook, type Library } from './notebook';
import { markdownBody } from './document-text';
import { popupMenu, dismissPopupMenu as dismiss, popupMenuOpen, type MenuItem as Item } from './popup-menu';

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
  const title = noteTitle(note);
  const backlinks = [...new Set(library.notes.filter(other => !other.trashed && [...other.source.matchAll(/\[\[([^\]]+)\]\]/g)].some(match => match[1].trim() === title)).map(noteTitle))].sort();
  const date = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false });
  return { words, modified: date.format(note.updatedAt), created: date.format(note.createdAt), backlinks };
}

export function createNoteMenu(getBook: () => Notebook, actions: {
  save(): Promise<boolean>;
  change(mutate: () => Promise<void>, reload?: boolean): Promise<void>;
  error(message: string): void;
}) {
  let infoPanel: HTMLElement | undefined, nativeInfo = false;
  function dismissInfo() {
    infoPanel?.remove(); infoPanel = undefined;
    if (nativeInfo) { nativeInfo = false; void import('@tauri-apps/api/core').then(({ invoke }) => invoke('close_note_info')).catch(() => undefined); }
  }
  document.addEventListener('pointerdown', event => { if (!infoPanel?.contains(event.target as Node)) dismissInfo(); });
  window.addEventListener('resize', () => { dismiss(); dismissInfo(); });
  document.querySelector('#notes-list')?.addEventListener('scroll', () => { dismiss(); dismissInfo(); });
  document.addEventListener('keydown', event => { if (event.key === 'Escape' && infoPanel) { dismissInfo(); document.querySelector<HTMLElement>('.note-row[aria-selected=true]')?.focus(); } });
  document.addEventListener('keydown', async event => {
    if (event.isComposing || (!event.metaKey && !event.ctrlKey) || document.querySelector('dialog[open]') || popupMenuOpen()) return;
    const row = document.activeElement?.closest<HTMLElement>('.note-row');
    const note = getBook().data.notes.find(note => note.id === row?.dataset.noteId);
    if (!note) return;
    let run: (() => void | Promise<void>) | undefined;
    if (event.altKey && event.key.toLowerCase() === 'i') run = () => showInfo(note.id);
    if (!event.altKey && !note.trashed) {
      if (event.shiftKey && event.key.toLowerCase() === 'p') run = () => actions.change(() => getBook().togglePin(note.id));
      if (!event.shiftKey && event.key.toLowerCase() === 'd') run = () => actions.change(() => getBook().duplicateNote(note.id), true);
      if (!event.shiftKey && event.key.toLowerCase() === 'r') run = () => rename(note.id);
      if (event.key === 'Backspace' && !getBook().external) run = () => actions.change(() => getBook().trashNote(note.id));
    }
    if (run) { event.preventDefault(); if (await actions.save()) await run(); }
  });

  function rename(id: string) {
    const note = getBook().data.notes.find(note => note.id === id);
    if (!note) return;
    dismissInfo();
    const title = document.querySelector<HTMLElement>('#note-title')!;
    const input = document.createElement('input'); input.id = 'rename-note'; input.dataset.noteId = id; input.ariaLabel = '笔记名称'; input.value = noteTitle(note); input.maxLength = 500;
    if (/^[\da-f]{8}-[\da-f]{4}-[\da-f]{4}-[\da-f]{4}-[\da-f]{12}\.md$/i.test(note.filename ?? '')) input.value = '';
    input.placeholder = '无标题';
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
  document.querySelector('#note-title')?.addEventListener('dblclick', () => { if (getBook().current && !getBook().loadError) rename(getBook().current.id); });
  document.addEventListener('rename-current-note', () => { if (getBook().current && !getBook().loadError) rename(getBook().current.id); });

  async function showInfo(id: string) {
    if (!await actions.save()) return;
    const row = document.querySelector<HTMLElement>(`[data-note-id="${CSS.escape(id)}"]`);
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
    const panel = document.createElement('div'); panel.className = 'note-info'; panel.role = 'dialog'; panel.ariaLabel = '笔记简介'; panel.tabIndex = -1;
    for (const text of [`字数：${info.words}`, `修改：${info.modified}`, `创建：${info.created}`]) { const line = document.createElement('p'); line.textContent = text; panel.append(line); }
    const divider = document.createElement('hr'), backlinks = document.createElement('p'), list = document.createElement('div');
    backlinks.className = 'note-info-backlinks'; backlinks.textContent = `反向链接：${info.backlinks.length || '无'}`;
    list.className = 'note-info-links'; list.textContent = info.backlinks.join('\n'); panel.append(divider, backlinks, list); document.body.append(panel);
    const left = anchor.right + 18, flip = left + panel.offsetWidth > innerWidth - 8;
    panel.classList.toggle('opens-left', flip);
    panel.style.left = `${Math.max(8, flip ? anchor.left - panel.offsetWidth - 18 : left)}px`;
    panel.style.top = `${Math.max(8, Math.min(anchor.top + anchor.height / 2 - panel.offsetHeight / 2, innerHeight - panel.offsetHeight - 8))}px`;
    panel.style.setProperty('--arrow-top', `${Math.max(22, Math.min(anchor.top + anchor.height / 2 - parseFloat(panel.style.top), panel.offsetHeight - 22))}px`);
    infoPanel = panel; panel.focus();
  }
  return async (id: string, x: number, y: number) => {
    dismiss(); dismissInfo();
    const note = getBook().data.notes.find(note => note.id === id);
    if (!note) return;
    const change = async (run: () => Promise<void>, reload = false) => { if (await actions.save()) await actions.change(run, reload); };
    const trash: Item = { text: '废纸篓', run: () => change(() => getBook().trashNote(id)) };
    const items: Item[] = [
      { text: '显示简介', shortcut: '⌥⌘I', run: () => showInfo(id) }, { text: '' },
      ...(note.trashed ? [{ text: '恢复笔记', run: () => change(() => getBook().trashNote(id, false)) }] : [
        { text: note.pinned ? '取消置顶' : '置顶', shortcut: '⇧⌘P', run: () => change(() => getBook().togglePin(id)) },
        { text: '制作副本', shortcut: '⌘D', run: () => change(() => getBook().duplicateNote(id), true) },
        { text: '重命名', shortcut: '⌘R', run: () => rename(id) },
        ...(!getBook().external ? [{ text: '移到废纸篓', shortcut: '⌘⌫', run: trash.run }] : []),
      ]),
      { text: '导出', items: [{ text: 'Markdown', run: () => { void (async () => {
        if (!await actions.save()) return;
        const latest = getBook().data.notes.find(note => note.id === id);
        if (!latest) return;
        const filename = noteTitle(latest).replace(/[\\/:*?"<>|\x00-\x1f]/g, '_') + '.md';
        if ('__TAURI_INTERNALS__' in window) {
          try { const { invoke } = await import('@tauri-apps/api/core'); await invoke('export_markdown', { filename, source: latest.source }); }
          catch { actions.error('Markdown 导出失败，笔记原文仍保留'); }
          return;
        }
        const url = URL.createObjectURL(new Blob([latest.source], { type: 'text/markdown;charset=utf-8' }));
        const link = document.createElement('a'); link.href = url; link.download = filename;
        document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 30_000);
      })(); } }] },
      ...(!getBook().external ? [{ text: '移动', items: [
        ...(!note.trashed ? [trash, { text: '' }] : []),
        ...getBook().data.folders.filter(folder => note.trashed || folder.id !== note.folderId).map(folder => ({ text: folder.name, run: () => change(() => getBook().moveNote(id, folder.id)) })),
      ] }] : []),
    ];
    await popupMenu(items, x, y, actions.error, () => document.querySelector<HTMLElement>('.note-row[aria-selected=true]')?.focus(), '笔记操作');
  };
}
