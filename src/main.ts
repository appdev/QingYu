import './style.css';
import starter from './starter.md?raw';
import formatIcon from '../public/miaoyan/icons/icon_format.svg?raw';
import previewIcon from '../public/miaoyan/icons/icon_preview.svg?raw';
import splitIcon from '../public/miaoyan/icons/icon_editor_split.svg?raw';
import editIcon from '../public/miaoyan/icons/newNote.svg?raw';
import { Compartment, EditorState } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, drawSelection, keymap, type ViewUpdate, type DecorationSet } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { HighlightStyle, indentOnInput, syntaxHighlighting, syntaxTree } from '@codemirror/language';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { languages } from '@codemirror/language-data';
import { searchKeymap } from '@codemirror/search';
import { tags } from '@lezer/highlight';
import { markdownBody } from './document-text';
import { formatTransaction, replaceSource, toggleTask, type Format } from './format';
import type { RichEditor } from './rich-editor';
import { Notebook, noteTitle } from './notebook';
import { createSidebar } from './sidebar';
import { preferences, previewPreferences, settingsShortcut, native, mobile } from './preferences';
import { connectPreferences, hostSync } from './settings-bridge';
import { createSyncSession } from './sync-session';
import { openSettings } from './open-settings';
import { startAutoSync } from './auto-sync';
import { revealCaret } from './editor-viewport';

const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
const workspace = $('#workspace');
const frame = $<HTMLIFrameElement>('#preview');
const saveStatus = $('#save-status');
function setStatus(message: string, error = false) {
  saveStatus.textContent = message;
  document.body.dataset.statusError = String(error);
}
const libraryNotebook = new Notebook({ getItem: key => localStorage.getItem(key), setItem: (key, value) => localStorage.setItem(key, value) }, starter);
let notebook = libraryNotebook;
if ('__TAURI_INTERNALS__' in window) {
  setStatus('正在打开本地笔记文件夹…');
  try { await notebook.openFiles(); }
  catch (error) { notebook.loadError = true; setStatus(error instanceof Error ? error.message : String(error), true); }
}
const initial = notebook.current?.source ?? '';
let sidebar: ReturnType<typeof createSidebar> | undefined;
let showingNote = Boolean(notebook.current);
$('#sync-button').onclick = async () => {
  try { await openSettings(syncSession, 'sync'); }
  catch { setStatus('无法打开同步设置，本机笔记仍保留', true); }
};
const syncSession = createSyncSession(libraryNotebook, () => {
  if (notebook === libraryNotebook) { showCurrentNote(Boolean(notebook.current)); sidebar?.refresh(); }
  setStatus('笔记库同步完成');
}, () => {
  if (document.body.dataset.libraryBusy === 'true') throw new Error('笔记位置正在更改，请稍后同步');
  return notebook === libraryNotebook ? saveDraft(true) : Promise.resolve(true);
});
let syncBusy = false;
syncSession.subscribe(status => {
  syncBusy = status.busy;
  const button = $('#sync-button');
  button.dataset.state = status.busy ? 'syncing' : status.error ? 'error' : status.message ? 'synced' : 'idle';
  const label = `S3 同步：${status.busy ? '正在同步…' : status.message || '尚未同步'}`;
  button.title = `${label}\n点击查看同步设置`;
  button.setAttribute('aria-label', label);
  $('#app-shell').inert = status.busy || document.body.dataset.libraryBusy === 'true';
  document.body.dataset.syncBusy = String(status.busy);
  if (status.busy) setStatus(status.message || '正在同步，请稍候…');
});
document.addEventListener('keydown', event => { if ((syncBusy || document.body.dataset.libraryBusy === 'true') && !(event.target instanceof Element && event.target.closest('.settings-inline')) && !settingsShortcut(event)) { event.preventDefault(); event.stopImmediatePropagation(); } }, true);
$('#settings-button').onclick = () => { void openSettings(syncSession).catch(() => setStatus('无法打开设置', true)); };
document.addEventListener('keydown', event => {
  if (settingsShortcut(event) && (!native || mobile)) { event.preventDefault(); void openSettings(syncSession); }
}, true);

const formatItems: [Format, string, string, string][] = [
  ['heading', '标题', '⌘⇧H', 'H'], ['bold', '粗体', '⌘B', 'B'], ['italic', '斜体', '⌘I', 'I'],
  ['strike', '删除线', '⌘⇧X', 'S̶'], ['quote', '引用', '⌘⇧Q', '❯'], ['task', '任务列表', '⌘⇧T', '☑'],
  ['code', '行内代码', '⌘E', '</>'], ['link', '链接', '⌘K', '↗'], ['table', '表格', '', '▦'],
];
const formatBindings = formatItems.filter(item => item[2]).map(([action]) => ({
  key: ({ bold: 'Mod-b', italic: 'Mod-i', strike: 'Mod-Shift-x', heading: 'Mod-Shift-h', quote: 'Mod-Shift-q', task: 'Mod-Shift-t', code: 'Mod-e', link: 'Mod-k' } as Record<string, string>)[action],
  run: () => { applyFormat(action); return true; },
}));
const menu = $('#format-menu');
const mobileTools = $('#mobile-tools');
const desktop = !mobile;
const wrapping = new Compartment();
let mode: 'edit' | 'split' | 'preview' | 'rich' = 'edit';
let richEditor: RichEditor | undefined;
let richLoading: Promise<RichEditor> | undefined;
let ready = false;
let version = 0;
let renderedVersion = -1;
let sourceOffset = 0;
let renderTimer: ReturnType<typeof setTimeout>;
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let scrollTimer: ReturnType<typeof setTimeout>;
let ignoreScrollUntil = 0;
let pendingRender: { id: number; source: string } | null = null;

const highlight = HighlightStyle.define([
  { tag: [tags.heading1, tags.heading2, tags.heading3, tags.heading4, tags.heading5, tags.heading6], class: 'md-title' }, { tag: tags.emphasis, class: 'md-emphasis' },
  { tag: tags.strong, class: 'md-strong' }, { tag: tags.strikethrough, class: 'md-strike' },
  { tag: tags.link, class: 'md-link' }, { tag: tags.url, class: 'md-url' },
  { tag: tags.quote, class: 'md-quote' }, { tag: tags.monospace, class: 'md-code' },
  { tag: tags.keyword, class: 'md-keyword' }, { tag: tags.string, class: 'md-string' },
  { tag: tags.number, class: 'md-number' }, { tag: tags.comment, class: 'md-comment' },
]);

function decorations(view: EditorView): DecorationSet {
  const ranges: ReturnType<Decoration['range']>[] = [];
  const tree = syntaxTree(view.state);
  for (const { from, to } of view.visibleRanges) {
    tree.iterate({ from, to, enter(node) {
      if (node.name === 'TaskMarker') {
        ranges.push(Decoration.mark({ class: 'md-task' }).range(node.from, node.to));
      } else if (/^(HeaderMark|EmphasisMark|QuoteMark|ListMark|CodeMark|LinkMark|StrikethroughMark)$/.test(node.name) && !(node.name === 'CodeMark' && node.node.parent?.name === 'FencedCode')) {
        ranges.push(Decoration.mark({ class: node.name === 'ListMark' ? 'md-list-mark md-mark' : 'md-mark' }).range(node.from, node.to));
      }
    } });
  }
  return Decoration.set(ranges, true);
}

const markdownDecorations = ViewPlugin.fromClass(class {
  decorations: DecorationSet;
  constructor(view: EditorView) { this.decorations = decorations(view); }
  update(update: ViewUpdate) {
    if (update.docChanged || update.viewportChanged || syntaxTree(update.startState) !== syntaxTree(update.state)) this.decorations = decorations(update.view);
  }
}, { decorations: plugin => plugin.decorations });

function applyFormat(action: Format) {
  if (notebook.loadError || notebook.current?.readOnly) return;
  if (!showingNote) return;
  if (mode === 'rich') {
    if (!richEditor || richEditor.composing) return;
    richEditor.format(action);
    closeMenu();
    return;
  }
  if (view.compositionStarted) return;
  if (mode === 'preview') setMode('edit');
  view.dispatch(formatTransaction(view.state, action));
  view.focus();
  closeMenu();
}

function createNoteState(source: string) {
  return EditorState.create({
    doc: source,
    extensions: [
      history(), drawSelection(), indentOnInput(), wrapping.of(preferences.value.lineWrap ? EditorView.lineWrapping : []),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(highlight), markdownDecorations,
      EditorView.contentAttributes.of({ 'aria-label': 'Markdown 原文', spellcheck: 'false', autocapitalize: 'off' }),
      keymap.of([
        ...formatBindings,
        indentWithTab, ...defaultKeymap, ...historyKeymap, ...searchKeymap,
      ]),
      EditorView.updateListener.of(update => {
        if (!update.docChanged) return;
        version++;
        refreshTitle();
        if (document.body.dataset.statusError !== 'true') setStatus('正在保留草稿…');
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => { void saveDraft(); }, 500);
        scheduleRender();
      }),
      EditorView.editable.of(!notebook.loadError && !notebook.current?.readOnly),
      EditorState.readOnly.of(notebook.loadError || Boolean(notebook.current?.readOnly)),
      EditorView.domEventHandlers({
        compositionend() { setTimeout(() => { scheduleRender(0); saveDraft(); }, 30); },
        scroll() { syncPreview(); },
        focus() { updateViewport(); },
        blur() { saveDraft(); updateViewport(); },
      }),
    ],
  });
}
const view = new EditorView({ parent: $('#editor'), state: createNoteState(initial) });

function refreshTitle(force = false) {
  if (force || !$('#rename-note')) $('#note-title').textContent = showingNote ? noteTitle({ ...notebook.current, source: view.state.doc.toString() }) : '未选择笔记';
  $('#word-count').textContent = showingNote ? `${view.state.doc.length.toLocaleString('zh-CN')} 字符` : '';
}
async function saveDraft(includeTitle = false) {
  if (!showingNote) return true;
  if (!includeTitle && document.body.dataset.libraryBusy === 'true') {
    clearTimeout(saveTimer); saveTimer = setTimeout(() => { void saveDraft(); }, 500); return false;
  }
  if (view.compositionStarted || richEditor?.composing) return false;
  richEditor?.flush();
  clearTimeout(saveTimer);
  saveTimer = undefined;
  const text = view.state.doc.toString();
  const note = notebook.current;
  if (!note) return true;
  const titleInput = includeTitle ? document.querySelector<HTMLInputElement>('#rename-note') : null;
  if (titleInput?.dataset.composing === 'true') return false;
  const titleValue = titleInput?.value;
  const stored = note.source;
  // CodeMirror normalizes line endings internally; viewing a CRLF draft is not an edit.
  const changed = stored.replace(/\r\n?/g, '\n') !== text;
  try {
    await notebook.save(changed ? text : stored, note.id);
    if (titleInput?.isConnected && titleInput.dataset.noteId) {
      if (titleValue?.trim()) await notebook.renameNote(titleInput.dataset.noteId, titleValue);
      if (titleInput.value !== titleValue) return saveDraft(includeTitle);
      refreshTitle(true);
      sidebar?.refresh();
    }
    if (note.id === notebook.data.activeId && view.state.doc.toString() !== text) return saveDraft(includeTitle);
    if (note.id === notebook.data.activeId && view.state.doc.toString() === text) setStatus(note.readOnly ? '文件不可读或内容损坏，仅可预览，原文件未修改' : notebook.root ? '笔记已保存到本地文件' : '网页开发环境：已保留草稿');
    if (changed) sidebar?.refresh();
    return true;
  } catch (error) {
    setStatus(error instanceof Error ? error.message : typeof error === 'string' ? error : '笔记保存失败，暂不能切换；请复制原文备份', true);
    return false;
  }
}

function showCurrentNote(selected: boolean) {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  richEditor?.deactivate();
  showingNote = selected;
  workspace.hidden = !selected;
  $('#empty-note').hidden = selected;
  for (const kind of ['format', 'edit', 'rich', 'split', 'preview']) $<HTMLButtonElement>(`#${kind}-button`).disabled = !selected || Boolean(notebook.current?.readOnly && (kind === 'rich' || kind === 'format')) || notebook.loadError;
  if (selected) {
    // Each document gets its own history, so Undo cannot restore a different note's content.
    view.setState(createNoteState(notebook.current.source));
    version++;
    renderedVersion = -1;
    pendingRender = null;
    sourceOffset = 0;
    scheduleRender(0);
    if (mode === 'rich' && notebook.current.readOnly) { mode = 'edit'; workspace.dataset.mode = mode; }
    if (mode === 'rich') richEditor?.activate(notebook.current.source, notebook.current.id);
    view.requestMeasure();
  }
  refreshTitle();
}

async function selectNote(id: string | null) {
  if (!await saveDraft(true)) return false;
  if (id === notebook.data.activeId && showingNote) return true;
  try {
    if (id) await notebook.select(id);
    showCurrentNote(id !== null);
    return true;
  } catch { setStatus('无法切换笔记，当前内容仍保留在编辑区', true); return false; }
}

async function createNote(folderId: string, title = '') {
  if (!await saveDraft(true)) return false;
  try {
    await notebook.createNote(folderId, title);
    showCurrentNote(true);
    if (mode === 'preview') await setMode('edit');
    if (mode === 'rich') richEditor?.focus(); else if (mode === 'edit' || mode === 'split') view.focus();
    return true;
  } catch (error) { setStatus(error instanceof Error ? error.message : String(error), true); return false; }
}
function scheduleRender(delay = 150) {
  clearTimeout(renderTimer);
  // Let the system IME finish its composition without preview-triggered layout work.
  if (view.compositionStarted) return;
  renderTimer = setTimeout(() => {
    if (view.compositionStarted) return;
    const source = view.state.doc.toString();
    sourceOffset = markdownBody(source).offset;
    pendingRender = { id: version, source };
    if (mode === 'preview' || mode === 'split') sendRender();
  }, delay);
}
function sendRender() {
  if (ready && pendingRender && !view.compositionStarted) frame.contentWindow?.postMessage({ type: 'render', ...pendingRender }, location.origin);
}
function previewReady() {
  ready = true;
  frame.contentWindow?.postMessage({ type: 'preferences', ...previewPreferences() }, location.origin);
  if (mode === 'preview' || mode === 'split') sendRender();
}
async function setMode(next: typeof mode) {
  if (!showingNote) return;
  if (notebook.current?.readOnly && next === 'rich') return;
  if (view.compositionStarted || richEditor?.composing) return;
  richEditor?.deactivate();
  if (next === 'split' && matchMedia('(max-width: 640px)').matches) next = 'preview';
  mode = next;
  workspace.dataset.mode = mode;
  for (const kind of ['edit', 'split', 'preview', 'rich']) $(`#${kind}-button`).setAttribute('aria-pressed', String(kind === mode));
  if (next === 'rich') {
    closeMenu();
    $('#rich-pane').setAttribute('aria-busy', 'true');
    try {
      richLoading ??= import('./rich-editor').then(({ createRichEditor }) => createRichEditor($('#rich-editor'), source => {
        const transaction = replaceSource(view.state, source);
        if (transaction) view.dispatch(transaction);
      }));
      richEditor = await richLoading;
      if (mode === 'rich') { richEditor.activate(view.state.doc.toString(), notebook.current.id); richEditor.focus(); }
    } catch (error) {
      console.error(error);
      richLoading = undefined;
      $('#rich-editor').textContent = '正在加载所见即所得编辑器…';
      if (mode === 'rich') {
        void setMode('edit');
        setStatus('所见即所得加载失败，原文仍可编辑', true);
      }
    } finally { $('#rich-pane').setAttribute('aria-busy', 'false'); }
  } else if (next !== 'edit') {
    if (pendingRender && renderedVersion !== pendingRender.id) sendRender();
    syncPreview();
  }
  view.requestMeasure();
  updateViewport();
}
function closeMenu() { menu.hidden = true; $('#format-button').setAttribute('aria-expanded', 'false'); }
$('#format-button').innerHTML = formatIcon;
$('#edit-button').innerHTML = editIcon;
$('#split-button').innerHTML = splitIcon;
$('#preview-button').innerHTML = previewIcon;
$('#edit-button').onclick = () => { void setMode('edit'); if (mode === 'edit') view.focus(); };
$('#rich-button').onclick = () => { void setMode(mode === 'rich' ? 'edit' : 'rich'); };
$('#split-button').onclick = () => setMode(mode === 'split' ? 'edit' : 'split');
$('#preview-button').onclick = () => setMode(mode === 'preview' ? 'edit' : 'preview');
$('#format-button').onclick = () => {
  menu.hidden = !menu.hidden;
  if (desktop && !menu.hidden) menu.style.right = `${innerWidth - $('#format-button').getBoundingClientRect().right}px`;
  $('#format-button').setAttribute('aria-expanded', String(!menu.hidden));
};
for (const [action, label, shortcut, symbol] of formatItems) {
  const button = document.createElement('button');
  button.role = 'menuitem';
  button.innerHTML = `<span>${label}</span><kbd>${shortcut}</kbd>`;
  button.onclick = () => applyFormat(action);
  menu.appendChild(button);
  if (action !== 'strike' && action !== 'table') {
    const quick = document.createElement('button');
    quick.textContent = symbol;
    quick.setAttribute('aria-label', label);
    quick.onclick = () => applyFormat(action);
    mobileTools.appendChild(quick);
  }
}
// Toolbar presses must not dismiss the keyboard or move the editor selection.
for (const bar of [$('.actions'), menu, mobileTools]) bar.addEventListener('pointerdown', event => {
  if ((event.target as HTMLElement).closest('button')) event.preventDefault();
});
document.addEventListener('pointerdown', event => {
  if (!menu.contains(event.target as Node) && !$('#format-button').contains(event.target as Node)) closeMenu();
});
document.addEventListener('keydown', event => {
  if (mode !== 'rich' || !$('#rich-editor').contains(document.activeElement) || event.isComposing || event.altKey || (!event.metaKey && !event.ctrlKey)) return;
  const binding = formatBindings.find(item => item.key === `Mod-${event.shiftKey ? 'Shift-' : ''}${event.key.toLowerCase()}`);
  if (binding) { event.preventDefault(); event.stopPropagation(); binding.run(); }
}, true);
document.addEventListener('keydown', event => {
  if (event.isComposing) return;
  if (event.key === 'Escape') closeMenu();
  if (!event.metaKey && !event.ctrlKey) return;
  if (event.key === '3') { event.preventDefault(); setMode(mode === 'preview' ? 'edit' : 'preview'); }
  if (event.key === '\\') { event.preventDefault(); setMode(mode === 'split' ? 'edit' : 'split'); }
  if (event.key.toLowerCase() === 's') { event.preventDefault(); clearTimeout(saveTimer); void saveDraft(true); }
});
window.addEventListener('pagehide', () => { void saveDraft(true); });
document.addEventListener('visibilitychange', () => { if (document.hidden) void saveDraft(true); });

function syncPreview() {
  if (mode !== 'split' || performance.now() < ignoreScrollUntil) return;
  clearTimeout(scrollTimer);
  scrollTimer = setTimeout(() => {
    const position = view.lineBlockAtHeight(view.scrollDOM.scrollTop);
    const line = view.state.doc.lineAt(position.from).number - sourceOffset;
    frame.contentWindow?.postMessage({ type: 'scroll', line: Math.max(1, line) }, location.origin);
  }, 50);
}
window.addEventListener('message', ({ source, origin, data }) => {
  if (source !== frame.contentWindow || origin !== location.origin) return;
  if (data.type === 'preview-ready') previewReady();
  if (data.type === 'preview-rendered') { renderedVersion = data.id; syncPreview(); }
  if (data.type === 'preview-error') setStatus('部分预览未完成，原文已保留', true);
  if (data.type === 'task') {
    if (notebook.loadError || notebook.current?.readOnly) return;
    if (view.compositionStarted || renderedVersion !== version || !Number.isInteger(data.line)) return;
    const transaction = toggleTask(view.state, data.line + sourceOffset);
    if (transaction) { view.dispatch(transaction); scheduleRender(0); }
  }
  if (data.type === 'preview-scroll' && mode === 'split' && Number.isFinite(data.line)) {
    const number = Math.max(1, Math.min(view.state.doc.lines, Math.floor(data.line) + sourceOffset));
    const rect = view.lineBlockAt(view.state.doc.line(number).from);
    ignoreScrollUntil = performance.now() + 160;
    view.scrollDOM.scrollTop = rect.top;
  }
});
// The cached iframe can finish before this module installs its message listener.
frame.addEventListener('load', previewReady);
if (frame.contentDocument?.readyState === 'complete') previewReady();

const splitter = $('#splitter');
function resizeSplit(percent: number) {
  const value = Math.max(25, Math.min(75, percent));
  workspace.style.setProperty('--split', `${value}%`);
  splitter.setAttribute('aria-valuenow', String(Math.round(value)));
  view.requestMeasure();
}
splitter.onpointerdown = event => {
  splitter.setPointerCapture(event.pointerId);
  frame.style.pointerEvents = 'none';
  splitter.onpointermove = move => resizeSplit((move.clientX - workspace.getBoundingClientRect().left) / workspace.clientWidth * 100);
  splitter.onpointerup = splitter.onpointercancel = () => { splitter.onpointermove = null; frame.style.pointerEvents = ''; };
};
splitter.onkeydown = event => {
  if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
    event.preventDefault(); resizeSplit(Number(splitter.getAttribute('aria-valuenow')) + (event.key === 'ArrowLeft' ? -2 : 2));
  }
};
splitter.ondblclick = () => resizeSplit(50);

let nativeKeyboard = false;
function updateViewport() {
  const viewport = window.visualViewport;
  const height = viewport?.height ?? innerHeight;
  document.body.style.height = `${height}px`;
  const keyboard = nativeKeyboard || innerHeight - height > 100;
  document.body.classList.toggle('keyboard-open', keyboard);
  mobileTools.hidden = !(keyboard && mode !== 'preview' && matchMedia('(pointer: coarse)').matches);
  if (mode === 'split' && matchMedia('(max-width: 640px)').matches) setMode('edit');
  view.requestMeasure();
  if (mode === 'rich') requestAnimationFrame(() => { if (!richEditor?.composing) revealCaret(richEditor?.element); });
}
window.visualViewport?.addEventListener('resize', updateViewport);
window.addEventListener('resize', updateViewport);

window.addEventListener('native-keyboard', event => { nativeKeyboard = Boolean((event as CustomEvent).detail); updateViewport(); });
document.documentElement.dataset.platform = '__TAURI_INTERNALS__' in window && /Mac/.test(navigator.platform) ? 'macos' : 'other';
sidebar = createSidebar(() => notebook, { select: selectNote, create: createNote, save: () => saveDraft(true), reload: () => showCurrentNote(Boolean(notebook.current)), error: message => setStatus(message, true) });
showCurrentNote(showingNote);
if (!notebook.loadError) void saveDraft();
refreshTitle();
scheduleRender(0);
updateViewport();
if ('__TAURI_INTERNALS__' in window) {
  let autoSync: ReturnType<typeof startAutoSync> | undefined;
  let workspace: Awaited<ReturnType<typeof import('./workspace').setupWorkspace>> | undefined;
  const { setupFileLibrary } = await import('./file-library-ui');
  await setupFileLibrary(libraryNotebook, { save: () => saveDraft(true), sync: async () => { await autoSync?.sync(); }, async refresh() { await workspace?.restore(); showCurrentNote(Boolean(notebook.current)); sidebar?.revealCurrent(); }, error: message => setStatus(message, true) });
  const { setupFileWatch } = await import('./file-watch');
  const watch = (book: Notebook) => setupFileWatch(book, {
    pending: () => notebook === book && Boolean(saveTimer || view.compositionStarted || richEditor?.composing || document.querySelector<HTMLInputElement>('#rename-note')?.dataset.composing === 'true'),
    buffer() { if (notebook !== book) return undefined; richEditor?.flush(); return showingNote && notebook.current ? { noteId: notebook.current.id, source: view.state.doc.toString() } : undefined; },
    refresh(previous) {
      if (notebook !== book) return;
      const current = notebook.current;
      if (previous?.id !== current?.id || previous?.source !== current?.source || previous?.readOnly !== current?.readOnly) {
        const selection = view.state.selection.main, top = view.scrollDOM.scrollTop;
        const richPane = document.querySelector<HTMLElement>('.vditor-content');
        const richTop = richPane?.scrollTop;
        showCurrentNote(Boolean(current));
        if (previous?.id === current?.id && current) {
          view.dispatch({ selection: { anchor: Math.min(selection.anchor, view.state.doc.length), head: Math.min(selection.head, view.state.doc.length) } });
          view.scrollDOM.scrollTop = top;
          if (richPane && richTop !== undefined) richPane.scrollTop = richTop;
        } else refreshTitle(true);
      } else refreshTitle();
      if (current && previous?.id === current.id && previous.path !== current.path && !current.readOnly && view.state.doc.toString() !== current.source.replace(/\r\n?/g, '\n')) {
        clearTimeout(saveTimer); saveTimer = setTimeout(() => { void saveDraft(); }, 500);
      }
      sidebar?.refresh();
      setStatus('已载入外部文件变化');
    },
    backedUp(paths) { setStatus(book.external ? `文件已在外部变化，你的版本已备份到：${paths.join('、')}` : '文件已在外部变化，你的版本已备份到笔记目录的 .miaoyan-conflicts', true); },
    error: message => setStatus(message, true),
  });
  await watch(libraryNotebook);
  if (!mobile) {
    let stopExternalWatch: (() => void) | undefined;
    const { setupWorkspace } = await import('./workspace');
    workspace = await setupWorkspace(libraryNotebook, {
      current: () => notebook, save: () => saveDraft(true),
      async activate(book) {
        stopExternalWatch?.(); stopExternalWatch = undefined;
        notebook = book;
        showCurrentNote(Boolean(book.current)); sidebar?.revealCurrent();
        if (book.external) stopExternalWatch = await watch(book);
        setStatus(book.external ? `临时文件：${book.root}（保存回原目录，不参与 S3 同步）` : '已返回笔记库');
      },
      error: message => setStatus(message, true),
    });
  }
  autoSync = startAutoSync(syncSession, () => document.body.inert || libraryNotebook.loadError || document.body.dataset.libraryBusy === 'true', message => setStatus(message, true), mobile);
}
window.addEventListener('preferences-applied', () => {
  view.dispatch({ effects: wrapping.reconfigure(preferences.value.lineWrap ? EditorView.lineWrapping : []) });
  view.requestMeasure();
  if (ready) frame.contentWindow?.postMessage({ type: 'preferences', ...previewPreferences() }, location.origin);
});
window.addEventListener('settings-error', event => setStatus((event as CustomEvent<string>).detail, true));
void connectPreferences().catch(() => setStatus('无法应用系统窗口设置', true));
void hostSync(syncSession).catch(() => setStatus('无法连接同步设置', true));
void setMode(preferences.value.defaultMode);
