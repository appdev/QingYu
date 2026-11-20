import './style.css';
import { mountGlassButton, mountGlassToolbar } from './glass-controls';
import 'glassy-liquid/components/progressive-blur';
import { confirmLinkedNote, showNoteLinkPicker, showShare } from './reference-panels';
import { openQuickOpen, type QuickCommand } from './quick-open';
import starter from './starter.md?raw';
import { firstLocalImage, markdownBody, plainMarkdownLine } from './document-text';
import { editorItemMove, moveEditorItem, type Format } from './format';
import { inlineFormatIcons, openEditorFormatPanel } from './editor-format-panel';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { Calendar, FileDown, FileText, FolderInput, FolderPlus, Image, Link as LinkIcon, Notebook as NotebookIcon, Pin, PinOff, Settings, SquarePen } from 'lucide';
import { createRichEditor, type RichEditor } from './rich-editor';
import { webLinkPreview, webLinkThumbnail } from './web-link-preview';
import { isBlankNote, isShared, Notebook, noteTitle, type Note, type NoteShare } from './notebook';
import { copyShareLink } from './mock-web-share';
import { createSidebar } from './sidebar';
import { changeNoteTextSize, noteTextSizeShortcut, preferences, settingsShortcut, native, mobile } from './preferences';
import { connectPreferences, hostSync } from './settings-bridge';
import { connectEditorHistoryMenu, connectMobileEditorHistory, refreshEditorHistory } from './editor-history-menu';
import { accountSyncNotices, createSyncSession, type SyncStatus } from './sync-session';
import { syncPasswordRequired } from './sync-password';
import { openSettings } from './open-settings';
import { sidebarAccount } from './client-account';
import { dailyNoteChoices } from './daily-note';
import type { SettingsSection } from './settings-view';
import { connectCloudAutoSync, startAutoSync } from './auto-sync';
import { createServerSyncClient } from './server-sync-client';
import { keepEditorFocusForTouchButtons, keepPanelTargetVisible, keepMobileViewportAtTop, revealCaret } from './editor-viewport';
import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { showWindowWhenPainted } from './window-ready';
import { assetBase, attachmentMarkdown, attachmentPlaceholder, attachmentUrl, openAttachment } from './attachments';
import { downloadFromMain, hostAssetDownloads } from './asset-fetch';
import { mountPendingImage } from './asset-download-view';
import { MAX_ASSET_BYTES } from './sync-files';
import { requireWebSession, watchWebSession } from './web-session';
import { isServerLibrary, offerServerDrafts } from './server-library';
import { dismissPopupMenu, popupMenuOpen, showMenu } from './popup-menu';
import { describeNoteLink, noteHeadingOffset, noteLinkChoices, noteLinkHeadingChoices, noteLinkMarkdown, resolveNoteLink } from './note-links';
import { createNoteRelationsFooter } from './editor-note-relations';
import { hostMenuBar } from './menu-bar-host';
import { isLocalAsset } from './server-assets';
import type { PictureRange, PictureRequest } from './note-picture';
import { translatedLabel, bindLabels, localizeStatus, localizeSyncError, uiText, workflowText } from './ui-language';

const localizeLabels = bindLabels(document);
localizeLabels();
await requireWebSession();

const $ = <T extends HTMLElement>(selector: string) => document.querySelector<T>(selector)!;
const workspace = $('#workspace');
// 对照 Pinto ShareLinkChrome：已分享的笔记在正文右上角显示「已分享」，点按复制链接。
const shareMarker = document.createElement('button');
shareMarker.type = 'button'; shareMarker.id = 'note-share-marker'; shareMarker.hidden = true;
shareMarker.title = translatedLabel('点按复制此笔记的分享链接。'); shareMarker.ariaLabel = translatedLabel('已分享，复制链接');
shareMarker.append(createLucideIcon(document, LinkIcon, ''), '已分享');
shareMarker.onclick = () => { const url = notebook.current?.share?.url; if (url) void copyShareLink(url); };
$('.titlebar').append(shareMarker);
document.documentElement.dataset.platform = native && /Mac/.test(navigator.platform) ? 'macos' : 'other';
const saveStatus = $('#save-status');
function setStatus(message: string, error = false) {
  saveStatus.textContent = localizeStatus(message);
  document.body.dataset.statusError = String(error);
}
const windowLabel = native ? getCurrentWindow().label : 'main';
const serverLibrary = isServerLibrary();
let browserNoteId = serverLibrary && !mobile ? new URLSearchParams(location.hash.slice(1)).get('note') : null;
const documentWindow = !mobile && (windowLabel.startsWith('document-') || Boolean(browserNoteId));
const noteWindow = documentWindow && (windowLabel.startsWith('document-note-') || Boolean(browserNoteId));
if (noteWindow) {
  $('#app-shell').dataset.noteWindow = '';
  $('#library-sidebar').hidden = $('#sidebar-button').hidden = $('#new-empty-note').hidden = true;
  const message = $('#empty-note p');
  message.dataset.i18n = 'missingNote';
  message.textContent = uiText('missingNote');
}
const canAddAttachments = native || serverLibrary;
const notebook = new Notebook({ getItem: key => documentWindow || serverLibrary ? null : localStorage.getItem(key), setItem: (key, value) => {
  if (documentWindow || serverLibrary) throw new Error('当前笔记不能转存到浏览器笔记库');
  localStorage.setItem(key, value);
} }, documentWindow || serverLibrary ? '' : starter);
if ('__TAURI_INTERNALS__' in window) {
  setStatus(documentWindow ? '正在打开文档…' : '正在打开本地笔记文件夹…');
  try {
    if (noteWindow) await notebook.attachNoteWindow();
    else if (documentWindow) await notebook.attachExternal();
    else await notebook.openFiles();
  }
  catch (error) { notebook.loadError = true; setStatus(error instanceof Error ? error.message : String(error), true); }
}
else if (serverLibrary) {
  setStatus('正在打开服务器笔记库…');
  try {
    await notebook.openServer();
    if (browserNoteId) await notebook.select(browserNoteId);
  }
  catch (error) { notebook.loadError = true; setStatus(error instanceof Error ? error.message : String(error), true); }
}
let sidebar: ReturnType<typeof createSidebar> | undefined;
let showingNote = Boolean(notebook.current) && !(noteWindow && notebook.loadError);
let displayedNoteId = showingNote ? notebook.current.id : undefined;
let syncApply: Promise<void> | undefined;
let draftDuringSync: { previous: Note; applied?: Note } | undefined;
let linkLibrary = notebook.data;
const availableNoteLinks = new Map<string, boolean>();
function noteLinkExists(target: string) {
  if (linkLibrary !== notebook.data) {
    linkLibrary = notebook.data;
    availableNoteLinks.clear();
  }
  let exists = availableNoteLinks.get(target);
  if (exists === undefined) {
    exists = Boolean(resolveNoteLink(linkLibrary, target, notebook.current).note);
    availableNoteLinks.set(target, exists);
  }
  return exists;
}
$('#sync-button').onclick = async () => {
  try { await showSettings('sync'); }
  catch { setStatus('无法打开同步设置，本机笔记仍保留', true); }
};
notebook.onSyncApply = () => {
  const previous = notebook.current;
  let release!: () => void;
  syncApply = new Promise(resolve => { release = resolve; });
  return applied => {
    try { if (applied) refreshAfterSync(previous); }
    finally { syncApply = undefined; release(); }
  };
};
function refreshAfterSync(previous: Note | undefined) {
  if (previous && displayedNoteId === previous.id) {
    const current = notebook.current;
    if (previous.id !== current?.id || previous.source !== current.source || previous.readOnly !== current.readOnly || previous.trashed !== current.trashed) {
      if (richEditor.composing || sourceForSave() !== editorSource(previous.source)) {
        draftDuringSync = { previous, applied: notebook.data.notes.find(note => note.id === previous.id) };
      } else {
        showCurrentNote(Boolean(current));
      }
    } else {
      refreshTitle();
      updateShareMarker();
      richEditor.refreshNoteLinks();
    }
  } else if (!previous && !displayedNoteId && notebook.current) {
    showCurrentNote(true);
  }
  sidebar?.refresh();
}
const syncSession = createSyncSession(notebook, () => setStatus('笔记库同步完成'), () => {
  if (notebook.loadError) throw new Error('笔记目录尚未读取成功，请先恢复目录访问或更改笔记位置');
  if (document.body.dataset.libraryBusy === 'true') throw new Error('笔记位置正在更改，请稍后同步');
  return saveDraft(true);
}, mobile);
if (native && !documentWindow) void hostAssetDownloads(notebook, syncSession.fetchAsset).catch(() => undefined);
let thumbnailRefresh: number | undefined;
function downloadAttachment(entry: { root: string; path: string }) {
  if (notebook.root !== entry.root) return Promise.reject(new Error('笔记库已切换，请重新下载附件'));
  const download = documentWindow ? downloadFromMain(windowLabel, entry.root, entry.path) : syncSession.fetchAsset(entry.path);
  // 列表缩略图不自己下载，正文里的图下完后刷新列表，缩略图才会从占位换成图片。
  // 一篇笔记里的多张图会接连下完，合并成一次刷新。
  return download.then(hash => {
    if (hash && sidebar && thumbnailRefresh === undefined) {
      thumbnailRefresh = window.setTimeout(() => { thumbnailRefresh = undefined; sidebar?.refresh(); }, 300);
    }
    return hash;
  });
}
async function ensureAttachment(noteId: string, source: string) {
  const entry = await attachmentPlaceholder(notebook, noteId, source);
  if (entry) return downloadAttachment(entry);
}
function mountNoteAssetImage(noteId: string, image: HTMLImageElement, frame: HTMLElement, url: string, source: string, thumbnail = false) {
  if (!native || notebook.external || notebook.data.sync?.version !== 2 || !isLocalAsset(source)) return;
  return mountPendingImage(image, frame, url, attachmentPlaceholder(notebook, noteId, source), downloadAttachment, thumbnail);
}
let syncBusy = false;
$('#sync-button').hidden = documentWindow || !native;
function renderSyncState(status: SyncStatus) {
  syncBusy = status.busy;
  const button = $('#sync-button');
  // 未登录、账号未就绪不算同步出错，按钮不显示红点。
  const failed = status.error && !accountSyncNotices.has(status.message);
  button.dataset.state = status.busy ? 'syncing' : failed ? 'error' : status.lastSyncedAt ? 'synced' : 'idle';
  const message = status.passwordRequired ? syncPasswordRequired : status.message || '存储与同步';
  const label = status.busy ? workflowText('正在同步…') : failed ? localizeSyncError(message) : localizeStatus(message);
  button.title = label + '\n' + workflowText('点击查看同步设置');
  button.setAttribute('aria-label', label);
  document.body.dataset.syncBusy = String(status.busy);
  if (status.busy) setStatus(status.message || '正在同步，请稍候…');
  else if (status.passwordRequired) setStatus(syncPasswordRequired);
  showSyncState(status, failed);
}
let lastSyncState: SyncStatus;
syncSession.subscribe(status => { lastSyncState = status; renderSyncState(status); });
// 账号行用转圈和红点表示同步状态，空闲时可悬停查看失败原因或上次同步时间。
function showSyncState(status: { busy: boolean; message: string; lastSyncedAt?: number }, failed: boolean) {
  const account = document.querySelector<HTMLElement>('#sidebar-account');
  const indicator = account?.querySelector<HTMLElement>('.sidebar-account-spinner');
  if (!account || !indicator) return;
  indicator.dataset.state = status.busy ? 'syncing' : failed ? 'error' : 'idle';
  if (status.busy) { account.removeAttribute('title'); return; }
  let tip = '';
  if (failed) tip = workflowText('同步失败：{message}').replace('{message}', () => localizeStatus(status.message));
  else if (status.lastSyncedAt) {
    const at = new Date(status.lastSyncedAt);
    const today = at.toDateString() === new Date().toDateString();
    const locale = document.documentElement.lang;
    const day = today ? uiText('menu.today') : at.toLocaleDateString(locale, { month: 'numeric', day: 'numeric' });
    const time = at.toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
    tip = workflowText('上次同步：{date}').replace('{date}', day + ' ' + time);
  }
  if (tip) account.title = tip;
  else account.removeAttribute('title');
}
document.addEventListener('keydown', event => { if (document.body.dataset.libraryBusy === 'true' && !(event.target instanceof Element && event.target.closest('.settings-inline')) && !settingsShortcut(event)) { event.preventDefault(); event.stopImmediatePropagation(); } }, true);
function showSettings(section?: SettingsSection) {
  return openSettings(native ? syncSession : undefined, section, () => dailyNoteChoices(notebook.data));
}
$('#settings-button').onclick = () => { void showSettings().catch(() => setStatus('无法打开设置', true)); };
document.addEventListener('keydown', event => {
  if (settingsShortcut(event) && (!native || mobile)) { event.preventDefault(); void showSettings(); }
}, true);

const formatItems: [Format, string, string?][] = [
  ['bold', '粗体', '⌘B'], ['italic', '斜体', '⌘I'], ['underline', '下划线', '⌘U'],
  ['strike', '删除线', '⌘⇧X'], ['highlight', '高亮', '⌘⇧E'],
  ['title', '标题', '⌘⇧T'], ['heading', '二级标题', '⌘⇧H'], ['subheading', '三级标题', '⌘⇧J'], ['body', '正文', '⌘⇧B'],
  ['code', '等宽', '⌘⇧M'], ['bullet', '• 项目符号列表', '⌘⇧7'], ['list', '– 短划线列表', '⌘⇧8'], ['ordered', '1. 编号列表', '⌘⇧9'],
  ['quote', '块引用', "⌘'"], ['callout', '标注'], ['codeblock', '代码块'], ['divider', '分隔线'],
  ['task', '任务清单', '⌘⇧L'], ['link', '链接', '⌘K'],
];
// 与 Pinto「格式」菜单一致的快捷键，按物理键位匹配，⇧7 这类组合在各键盘布局下都能触发。
const formatShortcuts: Record<string, Format | 'attach'> = {
  KeyB: 'bold', KeyI: 'italic', KeyU: 'underline', 'Shift-KeyX': 'strike', 'Shift-KeyE': 'highlight', 'Shift-KeyM': 'code',
  'Shift-KeyT': 'title', 'Shift-KeyH': 'heading', 'Shift-KeyJ': 'subheading', 'Shift-KeyB': 'body', 'Shift-KeyL': 'task',
  'Shift-Digit7': 'bullet', 'Shift-Digit8': 'list', 'Shift-Digit9': 'ordered', Quote: 'quote',
  'Alt-KeyT': 'table', 'Shift-KeyD': 'date', 'Shift-KeyA': 'attach',
  ...(mobile ? { KeyK: 'link' } : {}),
};
const menu = $('#format-menu');
const mobileTools = $('#mobile-tools');
const mobileSuggestions = document.createElement('div');
mobileSuggestions.className = 'mobile-suggestions';
if (mobile) mobileTools.append(mobileSuggestions);
const desktop = !mobile;
let closeFormatPanel: (() => void) | undefined;
let refreshFormatPanel: (() => void) | undefined;
let formatCell: ReturnType<RichEditor['captureTableFormat']> | undefined;
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let draftWrites = Promise.resolve(true);

function applyFormat(action: Format, keepMenu = false) {
  if (!currentWritingEditor()) return;
  richEditor.format(action);
  if (!keepMenu) closeMenu();
}
const noteRelations = createNoteRelationsFooter({
  context: () => ({ library: notebook.data, note: notebook.external || notebook.loadError ? undefined : notebook.current }),
  report: message => setStatus(message, true),
  async open(id, line) {
    if (!await selectNote(id)) return;
    sidebar?.revealCurrent();
    if (notebook.current?.id !== id) return;
    richEditor.scrollToLine(line);
  },
  async write(id, before, after, target) {
    const root = notebook.root;
    if (!await saveDraft(true) || root !== notebook.root || notebook.current?.id !== target) return false;
    try {
      await notebook.save(after, id, before);
      sidebar?.refresh();
      return true;
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error), true);
      return false;
    }
  },
});

const richEditor = createRichEditor($('#rich-editor'), () => {
  refreshTitle();
  updateCompose();
  try { notebook.stageDraft(sourceForSave()); }
  catch { setStatus('本地草稿备份失败，请保持页面打开并等待服务器保存', true); }
  if (document.body.dataset.statusError !== 'true') setStatus('正在保留草稿…');
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => { void saveDraft(); }, 500);
}, {
  sortCheckedItems: () => preferences.value.sortCheckedItems,
  relations: noteRelations,
  suggestionsHost: mobile ? mobileSuggestions : undefined,
  attachFile: !mobile && canAddAttachments ? () => attachFromKeyboard() : undefined,
  selectionChanged: () => refreshFormatPanel?.(),
  async openAsset(id, path) { await ensureAttachment(id, path); await openAttachment(notebook, id, path); },
  mountAssetImage: mountNoteAssetImage,
  async pendingAssetSize(id, path) { return (await attachmentPlaceholder(notebook, id, path))?.size; },
  async deletePendingAsset(id, path) {
    const entry = await attachmentPlaceholder(notebook, id, path);
    if (!entry) return;
    if (!window.confirm('下次同步时也会删除远端附件。要继续吗？')) return;
    await notebook.removeAssetPlaceholder(entry.root, entry.path, entry.hash);
    setStatus('已删除本地占位，下次同步时会删除远端附件');
  },
  openNoteLink(target) { void followNoteLink(target); },
  noteLinkExists,
  describeNoteLink(target) { return describeNoteLink(notebook.data, target, notebook.current); },
  loadLinkPreview: { page: webLinkPreview, thumbnail: webLinkThumbnail },
  sharePicture(source, selection) {
    const request = pictureRequest(notebook.current, source, selection);
    if (request) void import('./note-picture').then(({ showNotePicture }) => showNotePicture(request, focusEditor)).catch(() => setStatus('无法打开图片预览', true));
  },
  suggestNoteLinks(query, draft) {
    const { data, current } = notebook;
    const titles = () => noteLinkChoices(data, current?.id ?? '', query);
    return query.includes('#') ? noteLinkHeadingChoices(data, current, query, draft).then(choices => choices.length ? choices : titles())
      : titles();
  },
  onError: message => setStatus(message, true),
});
const { view } = richEditor;

function sourceForSave() {
  return view.state.doc.toString();
}
const editorSource = (source: string) => source.replace(/\r\n?/g, '\n');

function refreshTitle(force = false) {
  const note = displayedNoteId === notebook.current?.id ? notebook.current : draftDuringSync?.previous;
  if (force || !$('#rename-note')) $('#note-title').textContent = showingNote && note ? noteTitle({ ...note, source: view.state.doc.toString() }) : uiText('noSelection');
  $('#word-count').textContent = showingNote ? uiText('characterCount').replace('{count}', view.state.doc.length.toLocaleString(document.documentElement.lang)) : '';
}
function saveDraft(includeTitle = false) {
  const write = draftWrites.then(() => persistDraft(includeTitle));
  draftWrites = write.catch(() => false);
  return write;
}
async function openBrowserNote(id: string) {
  const tab = window.open('about:blank', '_blank');
  if (!tab) throw new Error('浏览器阻止了新标签页，请允许打开后重试');
  tab.opener = null;
  try {
    if (await saveDraft(true)) {
      tab.location.replace(new URL(`/#note=${encodeURIComponent(id)}`, location.origin).href);
      return;
    }
  } catch (error) { tab.close(); throw error; }
  tab.close();
}

async function persistDraft(includeTitle: boolean): Promise<boolean> {
  if (syncApply) await syncApply;
  if (documentWindow && notebook.loadError) return true;
  if (!showingNote) return true;
  if (!includeTitle && document.body.dataset.libraryBusy === 'true') {
    clearTimeout(saveTimer); saveTimer = setTimeout(() => { void saveDraft(); }, 500); return false;
  }
  if (richEditor.composing) return false;
  clearTimeout(saveTimer);
  saveTimer = undefined;
  const source = sourceForSave();
  const recovered = draftDuringSync?.previous.id === displayedNoteId ? draftDuringSync : undefined;
  const note = notebook.data.notes.find(entry => entry.id === displayedNoteId) ?? recovered?.previous;
  if (!note) return true;
  const titleInput = includeTitle ? document.querySelector<HTMLInputElement>('#rename-note') : null;
  if (titleInput?.dataset.composing === 'true') return false;
  const titleValue = titleInput?.value;
  const stored = note.source;
  // CodeMirror normalizes line endings internally; viewing a CRLF draft is not an edit.
  const changed = editorSource(stored) !== source;
  try {
    if (recovered) {
      if (source !== editorSource(recovered.previous.source)) await notebook.saveDraftAfterSync(recovered.previous, source, recovered.applied);
      else if (displayedNoteId === recovered.previous.id) showCurrentNote(Boolean(notebook.current));
      if (draftDuringSync === recovered) draftDuringSync = undefined;
    } else await notebook.save(changed ? source : stored, note.id, syncBusy && changed && !note.trashed ? stored : undefined);
    if (titleInput?.isConnected && titleInput.dataset.noteId) {
      if (titleValue?.trim()) await notebook.renameNote(titleInput.dataset.noteId, titleValue);
      if (titleInput.value !== titleValue) return persistDraft(includeTitle);
      refreshTitle(true);
      sidebar?.refresh();
    }
    if (note.id === notebook.data.activeId) {
      if (sourceForSave() !== source) return persistDraft(includeTitle);
      setStatus(note.readOnly ? '文件不可读或内容损坏，仅可预览，原文件未修改' : notebook.server ? '笔记已保存到服务器' : notebook.root ? '笔记已保存到本地文件' : '草稿已保存到此浏览器');
    }
    if (changed) sidebar?.refresh();
    return true;
  } catch (error) {
    if (!recovered && draftDuringSync?.previous.id === displayedNoteId && displayedNoteId === note.id) return persistDraft(includeTitle);
    setStatus(error instanceof Error ? error.message : typeof error === 'string' ? error : '笔记保存失败，暂不能切换；请复制原文备份', true);
    return false;
  }
}

const visitHistory: string[] = [];
let visitIndex = -1, movingInHistory = false;
const noteBack = $<HTMLButtonElement>('#note-back');
const noteForward = $<HTMLButtonElement>('#note-forward');

function historyDestination(step: -1 | 1) {
  const available = new Set(notebook.data.notes.filter(note => !note.trashed).map(note => note.id));
  for (let index = visitIndex + step; index >= 0 && index < visitHistory.length; index += step) {
    if (available.has(visitHistory[index])) return index;
  }
  return -1;
}
function updateHistoryButtons() {
  noteBack.disabled = movingInHistory || historyDestination(-1) < 0;
  noteForward.disabled = movingInHistory || historyDestination(1) < 0;
}
async function stepHistory(step: -1 | 1) {
  const index = historyDestination(step);
  if (movingInHistory || index < 0) return;
  movingInHistory = true; updateHistoryButtons();
  try {
    if (await selectNote(visitHistory[index], index)) sidebar?.revealCurrent();
  } finally { movingInHistory = false; updateHistoryButtons(); }
}
noteBack.onclick = () => { void stepHistory(-1); };
noteForward.onclick = () => { void stepHistory(1); };

function showCurrentNote(selected: boolean, fromHistory?: number) {
  closeFormatPanel?.();
  closeMenu();
  clearTimeout(saveTimer);
  saveTimer = undefined;
  richEditor.deactivate();
  showingNote = selected;
  displayedNoteId = selected ? notebook.current.id : undefined;
  workspace.hidden = !selected;
  refreshEditorHistory();
  $('#empty-note').hidden = selected;
  for (const kind of ['format', 'checklist', 'table', 'attach', 'note-more', 'share']) $<HTMLButtonElement>(`#${kind}-button`).disabled = !selected || Boolean(notebook.current?.readOnly && ['format', 'checklist', 'table', 'attach'].includes(kind)) || notebook.loadError;
  if (selected) {
    const note = notebook.current;
    if (browserNoteId && browserNoteId !== note.id) {
      browserNoteId = note.id;
      history.replaceState(history.state, '', `#note=${encodeURIComponent(note.id)}`);
    }
    if (fromHistory !== undefined) visitIndex = fromHistory;
    else if (!note.trashed && visitHistory[visitIndex] !== note.id) {
      visitHistory.splice(visitIndex + 1, visitHistory.length, note.id);
      if (visitHistory.length > 101) visitHistory.shift();
      visitIndex = visitHistory.length - 1;
    }
    richEditor.activate(note.source, note.id, notebook.server || native ? note.id : undefined, assetBase(notebook),
      notebook.loadError || Boolean(note.readOnly));
  }
  updateHistoryButtons();
  refreshTitle();
  updateShareMarker();
  updateCompose();
}
function updateShareMarker() {
  const note = showingNote && !notebook.loadError ? notebook.current : undefined;
  shareMarker.hidden = !note || !isShared(note) || !note.share?.url;
}

function blankNote() {
  return showingNote && !notebook.loadError && isBlankNote(notebook.current, sourceForSave());
}
// 对应 Pinto 的 ListCompose：空白新笔记开着时桌面上的新建按钮变灰，再建只会多一篇空白笔记。
function updateCompose() {
  if (mobile) return;
  const compose = $<HTMLButtonElement>('#new-note-button'), off = blankNote();
  compose.disabled = off;
  compose.title = off ? uiText('blankNote') : `${uiText('newNote')}（⌘N）`;
}

async function selectNote(id: string | null, fromHistory?: number) {
  if (movingInHistory && fromHistory === undefined) return false;
  if (!await saveDraft(true)) return false;
  if (id === notebook.data.activeId && showingNote) {
    if (fromHistory !== undefined) visitIndex = fromHistory;
    return true;
  }
  try {
    if (id && !await notebook.select(id)) return false;
    showCurrentNote(id !== null, fromHistory);
    return true;
  } catch { setStatus('无法切换笔记，当前内容仍保留在编辑区', true); return false; }
}

function focusEditor() {
  richEditor.focus();
}

async function followNoteLink(target: string) {
  const { note, heading } = resolveNoteLink(notebook.data, target, notebook.current);
  if (!note) {
    const segments = target.split('#')[0].split('/');
    const title = segments.pop()?.trim().replace(/\.(?:md|markdown)$/i, '');
    const current = notebook.current;
    const folderId = segments.length ? notebook.data.folders.find(folder => (folder.path || folder.name) === segments.join('/'))?.id : current?.folderId;
    if (!title || !current || !folderId || current.readOnly || notebook.external) { setStatus('找不到链接指向的笔记', true); return; }
    confirmLinkedNote(title, async () => {
      if (notebook.current?.id === current.id && await createNote(folderId, title)) sidebar?.revealCurrent();
    });
    return;
  }
  if (!await selectNote(note.id)) return;
  sidebar?.revealCurrent();
  if (!heading) return;
  const doc = view.state.doc;
  const position = await noteHeadingOffset(doc.toString(), heading);
  if (position === undefined || doc !== view.state.doc || !showingNote || notebook.current?.id !== note.id) return;
  richEditor.scrollToLine(doc.lineAt(position).number);
}

async function createNote(folderId: string, title = '') {
  if (!await saveDraft(true)) return false;
  try {
    await notebook.createNote(folderId, title);
    showCurrentNote(true);
    focusEditor();
    return true;
  } catch (error) { setStatus(error instanceof Error ? error.message : String(error), true); return false; }
}
let openingToday = false;
async function openTodayNote() {
  if (openingToday || notebook.loadError || notebook.external || document.body.dataset.libraryBusy === 'true' ||
    document.querySelector('dialog[open]')) return;
  openingToday = true;
  try {
    if (!await saveDraft(true)) return;
    const previous = notebook.current;
    const created = await notebook.openToday(preferences.value);
    if (!showingNote || previous?.id !== notebook.current?.id || previous.source !== notebook.current.source) showCurrentNote(true);
    sidebar?.openCurrent();
    view.dispatch({ selection: { anchor: view.state.doc.length }, scrollIntoView: true });
    richEditor.focus();
    setStatus(created ? '已创建今日笔记' : '已打开今日笔记');
  } catch (error) { setStatus(error instanceof Error ? error.message : String(error), true); }
  finally { openingToday = false; }
}

function closeMenu() {
  menu.hidden = true;
  formatCell = undefined;
  if (!closeFormatPanel) $('#format-button').setAttribute('aria-expanded', 'false');
}
function bindChecklistTool(button: HTMLElement) {
  button.addEventListener('pointerdown', event => {
    if (event.button === 0 && document.activeElement?.closest('.cm-markra-table')) event.preventDefault();
  });
  button.onclick = () => applyFormat('task');
}
bindChecklistTool($('#checklist-button'));
function pictureRequest(note: Note, source: string, selection?: PictureRange): PictureRequest | undefined {
  if (!note || note.trashed || note.readOnly) return;
  const root = notebook.root;
  return { title: noteTitle({ ...note, source }), source, selection,
    prepareImage: href => {
      if (notebook.root !== root) throw new Error('笔记库已切换，请重新打开图片预览');
      return ensureAttachment(note.id, href);
    },
    image: async href => {
      if (notebook.root !== root) return;
      if (!isLocalAsset(href) || href.startsWith('/') && !/^\/(?:i|files)\//u.test(href)) return;
      const path = decodeURIComponent(href.split(/[?#]/u)[0]);
      const type = path.toLowerCase().match(/\.(png|jpe?g|webp|gif|bmp)$/u)?.[1];
      if (!type) return;
      let blob: Blob;
      if (native) {
        const bytes = await invoke<ArrayBuffer>('note_picture_asset', { noteId: note.id, path });
        blob = new Blob([bytes], { type: `image/${type === 'jpg' ? 'jpeg' : type}` });
      } else {
        if (!notebook.server) return;
        const response = await fetch(`/api/web/assets/${encodeURIComponent(note.id)}?${new URLSearchParams({ path })}`);
        if (!response.ok) return;
        blob = await response.blob();
      }
      return new Promise<string | undefined>((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : undefined);
        reader.onerror = () => reject(reader.error); reader.readAsDataURL(blob);
      });
    },
  };
}
function shareNote(anchor: HTMLElement, note: Note, source = note.source, selection?: PictureRange) {
  const text = markdownBody(source).body.split('\n').map(plainMarkdownLine).join('\n');
  const picture = pictureRequest(note, source, selection);
  // 发布或停止后的状态写进这篇笔记的元数据，列表随之刷新分享标记。
  const book = notebook, id = note.id;
  const shareStore = { get: () => book.data.notes.find(other => other.id === id)?.share,
    set: (share: NoteShare) => book.setShare(id, share).then(() => { if (notebook === book) { sidebar?.refresh(); updateShareMarker(); } }) };
  showShare(anchor, picture?.title ?? noteTitle({ ...note, source }), text, message => setStatus(message, true), picture, JSON.stringify([notebook.root, note.id]), shareStore);
}
$('#share-button').onclick = event => {
  shareNote(event.currentTarget as HTMLElement, notebook.current, sourceForSave(), richEditor.pictureSelection());
};

const tableButton = $('#table-button');
function bindTableTool(button: HTMLElement) {
  const capture = () => richEditor.captureTableMenu(button);
  let open: ReturnType<typeof capture>;
  // Preserve the editing cell before WebKit transfers keyboard focus to the toolbar.
  button.addEventListener('pointerdown', event => {
    open = capture();
    if (open) event.preventDefault();
  });
  button.addEventListener('pointercancel', () => { open = undefined; });
  button.onclick = () => {
    const panel = open ?? capture();
    open = undefined;
    if (panel) panel();
    else applyFormat('table');
  };
}
bindTableTool(tableButton);
$('#note-more-button').onclick = event => {
  const box = $('#note-more-button').getBoundingClientRect();
  void sidebar?.showCurrentMenu(Math.max(8, box.right - 250), box.bottom + 6, event.detail === 0);
};
const untickButton = document.createElement('button');
untickButton.textContent = translatedLabel('取消勾选全部任务');
untickButton.role = 'menuitem';
untickButton.dataset.format = 'untick';
untickButton.onclick = () => {
  closeMenu();
  if (!currentWritingEditor()) return;
  richEditor.untickChecklist();
};
function currentWritingEditor() {
  return showingNote && !view.state.readOnly && !richEditor.composing ? view : null;
}
const moveItemButtons = ([true, false] as const).map(up => {
  const button = document.createElement('button');
  button.type = 'button';
  button.role = 'menuitem';
  button.textContent = up ? translatedLabel('上移项目') : translatedLabel('下移项目');
  button.title = up ? translatedLabel('上移项目') + ' ⌃⌘↑' : translatedLabel('下移项目') + ' ⌃⌘↓';
  button.onclick = () => {
    const editor = currentWritingEditor();
    closeMenu();
    if (editor) moveEditorItem(editor, up);
  };
  return button;
});
function showMobileFormat(trigger: HTMLButtonElement) {
  if (closeFormatPanel || !currentWritingEditor()) return;
  const cell = richEditor.captureTableFormat();
  const input = cell ? cell.element()?.closest('table') ?? undefined : richEditor.input;
  const scroller = richEditor.element;
  const noteId = notebook.current?.id;
  const events = new AbortController();
  const restore = () => {
    if (notebook.current?.id !== noteId) return;
    if (!cell?.restore()) focusEditor();
  };
  trigger.setAttribute('aria-controls', 'editor-format-panel');
  let layout: ReturnType<typeof keepPanelTargetVisible>;
  const { dialog, close, refresh, useInput } = openEditorFormatPanel(trigger, () => cell ? {
    active: new Set<Format>(['body', ...cell.active()]), canIndent: false, canOutdent: false,
    inCode: false, isTitle: false, inTable: true, canFormatCell: cell.canFormat(),
  } : richEditor.formatPanelState(), action => {
    if (notebook.current?.id !== noteId || notebook.current?.readOnly) return;
    if (cell) {
      cell.apply(action);
      const grid = cell.element()?.closest('table');
      if (grid) { useInput(grid); cell.restore(); }
    } else richEditor.applyFormatPanel(action);
    layout.refresh();
  }, shouldRestore => {
    events.abort();
    layout.stop();
    closeFormatPanel = undefined;
    refreshFormatPanel = undefined;
    if (shouldRestore) restore();
    updateViewport();
  }, input);
  closeFormatPanel = close;
  layout = keepPanelTargetVisible(workspace, scroller, dialog, cell ? cell.bounds : richEditor.formatTargetBounds);
  refreshFormatPanel = () => { refresh(); layout.refresh(); };
  cell?.restore();
  document.addEventListener('pointerdown', event => {
    const target = event.target as Element;
    if (dialog.contains(target)) return;
    if (cell ? !cell.element()?.contains(target) : !scroller.contains(target) || target.closest('.cm-markra-table')) close();
  }, { capture: true, signal: events.signal });
  document.addEventListener('keydown', event => {
    if (event.key !== 'Escape' || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    close(true);
  }, { capture: true, signal: events.signal });
}
const formatButton = $<HTMLButtonElement>('#format-button');
formatButton.ariaHasPopup = mobile ? 'dialog' : 'menu';
menu.tabIndex = -1;
const formatChoices = () => [...menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled):not([hidden])')];
formatButton.onclick = event => {
  if (mobile) { showMobileFormat(formatButton); return; }
  if (!menu.hidden) { closeMenu(); return; }
  formatCell = richEditor.captureTableFormat();
  const canFormatCell = formatCell?.canFormat();
  const activeFormat = formatCell ? undefined : richEditor.paragraphFormat();
  for (const button of menu.querySelectorAll<HTMLButtonElement>('button[data-format]')) {
    const action = button.dataset.format as Format;
    button.disabled = Boolean(formatCell && (!canFormatCell || (action !== 'code' && !inlineFormatIcons[action])));
    if (button.role === 'menuitemradio') button.ariaChecked = String(action === activeFormat);
  }
  untickButton.hidden = Boolean(formatCell) || !richEditor.canUntickChecklist;
  const editor = currentWritingEditor();
  moveItemButtons.forEach((button, index) => {
    button.disabled = Boolean(formatCell) || !editor || !editorItemMove(editor, index === 0);
    button.hidden = document.documentElement.dataset.platform === 'macos' || (!native && button.disabled);
  });
  menu.hidden = false;
  const bounds = formatButton.getBoundingClientRect();
  menu.style.left = `${Math.min(innerWidth - menu.offsetWidth - 8, Math.max(8, bounds.left + bounds.width / 2 - menu.offsetWidth / 2))}px`;
  formatButton.ariaExpanded = 'true';
  if (!event.detail) (formatChoices()[0] ?? menu).focus();
};
formatButton.onkeydown = event => {
  if (mobile || event.key !== 'ArrowDown') return;
  event.preventDefault();
  if (menu.hidden) formatButton.click();
  else (formatChoices()[0] ?? menu).focus();
};
menu.onkeydown = event => {
  const { key } = event;
  if (key === 'Escape' || key === 'Tab') {
    event.preventDefault();
    event.stopPropagation();
    closeMenu();
    formatButton.focus({ preventScroll: true });
    return;
  }
  const choices = formatChoices();
  if (!choices.length) return;
  const index = choices.indexOf(document.activeElement as HTMLButtonElement);
  if (key === 'Home') choices[0].focus();
  else if (key === 'End') choices.at(-1)?.focus();
  else if (key === 'ArrowDown' || key === 'ArrowRight') choices[(index + 1) % choices.length].focus();
  else if (key === 'ArrowUp' || key === 'ArrowLeft') choices[(index + choices.length - 1) % choices.length].focus();
  else return;
  event.preventDefault();
};
const inlineFormats = document.createElement('div'); inlineFormats.className = 'format-inline';
menu.append(inlineFormats);
for (const [action, label, shortcut] of formatItems) {
  if (action !== 'task' && action !== 'link') {
    const button = document.createElement('button');
    const icon = inlineFormatIcons[action];
    button.role = ['title', 'heading', 'subheading', 'body', 'bullet', 'list', 'ordered', 'quote', 'callout'].includes(action) ? 'menuitemradio' : 'menuitem';
    button.ariaLabel = label;
    button.title = [label, shortcut].filter(Boolean).join(' ');
    button.append(icon ? createLucideIcon(document, icon, 'format-inline-icon') : label);
    button.onclick = event => {
      const keyboard = !event.detail && document.activeElement === button;
      if (formatCell) {
        formatCell.apply(action);
        if (!icon || !keyboard) formatCell.restore();
        if (!icon) closeMenu();
      } else applyFormat(action, Boolean(icon));
      if (icon && keyboard && !menu.hidden) button.focus();
    };
    button.dataset.format = action;
    (icon ? inlineFormats : menu).append(button);
    if (action === 'ordered') menu.append(untickButton);
    if (action === 'quote') menu.append(...moveItemButtons);
  }
}
const quickFormat = formatButton.cloneNode(true) as HTMLButtonElement;
quickFormat.removeAttribute('id');
quickFormat.setAttribute('aria-controls', 'editor-format-panel');
quickFormat.setAttribute('aria-haspopup', 'dialog');
quickFormat.onclick = () => showMobileFormat(quickFormat);
const quickChecklist = $('#checklist-button').cloneNode(true) as HTMLButtonElement;
quickChecklist.removeAttribute('id');
bindChecklistTool(quickChecklist);
const quickTable = tableButton.cloneNode(true) as HTMLButtonElement;
quickTable.removeAttribute('id');
quickTable.className = 'mobile-table-button';
quickTable.title = quickTable.ariaLabel = translatedLabel('表格');
quickTable.querySelector('svg')!.insertAdjacentHTML('beforeend', `
  <g class="mobile-table-badge">
    <rect x="11.7" y="12.4" width="11.7" height="8" rx="4" fill="var(--toolbar-button-fill)" stroke="var(--toolbar-button-fill)" stroke-width="2"/>
    <g fill="currentColor" stroke="none">
      <circle cx="14.65" cy="16.4" r="1"/><circle cx="17.55" cy="16.4" r="1"/><circle cx="20.45" cy="16.4" r="1"/>
    </g>
  </g>`);
bindTableTool(quickTable);
const quickLink = document.createElement('button');
quickLink.type = 'button';
quickLink.hidden = true;
quickLink.ariaLabel = translatedLabel('添加链接');
quickLink.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
  <path d="m10 8 3.4-3.4a4.2 4.2 0 0 1 5.9 5.9l-3.4 3.4a4.2 4.2 0 0 1-5.9 0m4 2-3.4 3.4a4.2 4.2 0 0 1-5.9-5.9L8.1 10a4.2 4.2 0 0 1 5.9 0"/>
</svg>`;
quickLink.onpointerdown = event => event.preventDefault();
quickLink.onclick = () => {
  if (document.activeElement?.closest('.cm-markra-table')) applyFormat('link');
};
const mobileToolsGroup = document.createElement('div');
mobileToolsGroup.className = 'mobile-tools-group';
mobileToolsGroup.append(quickFormat, quickChecklist, quickLink, quickTable);
mobileTools.append(mobileToolsGroup);
const attachmentInput = document.createElement('input');
attachmentInput.type = 'file'; attachmentInput.multiple = true; attachmentInput.hidden = true;
document.body.append(attachmentInput);
const attachButton = $<HTMLButtonElement>('#attach-button');
attachButton.setAttribute('aria-haspopup', 'menu');
const attachMenuKey = canAddAttachments ? 'attach' : 'insertLink';
if (!canAddAttachments) {
  attachButton.dataset.i18nTitle = attachButton.dataset.i18nAriaLabel = attachMenuKey;
  attachButton.title = attachButton.ariaLabel = uiText(attachMenuKey);
  attachButton.replaceChildren(createLucideIcon(document, LinkIcon, ''));
}
function openAttachmentMenu(anchor: HTMLButtonElement, keyboard: boolean) {
  const icon = (node: Parameters<typeof createLucideIcon>[1]) => createLucideIcon(document, node, '');
  const insertInTable = richEditor.captureTableInsertion();
  const choose = (accept: string) => {
    richEditor.captureSelection();
    attachmentInput.accept = accept;
    attachmentInput.click();
  };
  const linkNote = () => {
    const id = notebook.current?.id;
    if (!id || !currentWritingEditor()) return;
    const original = view.state.doc;
    richEditor.captureSelection();
    showNoteLinkPicker(anchor, query => noteLinkChoices(notebook.data, id, query), target => {
      if (notebook.current?.id !== id || !currentWritingEditor() || view.state.doc !== original) return;
      if (insertInTable) {
        if (!insertInTable(selected => noteLinkMarkdown(target, selected))) setStatus('单元格已变化，请重新选择链接位置', true);
      } else richEditor.insert(selected => noteLinkMarkdown(target, selected));
    }, focusEditor);
  };
  void showMenu([
    ...(canAddAttachments ? [
      { text: '选择照片或视频…', icon: icon(Image), needsUserActivation: true, run: () => choose('image/*,video/*') },
      { text: '添加文件…', icon: icon(FileText), needsUserActivation: true, run: () => choose('') },
      { text: '' },
    ] : []),
    { text: '链接到笔记…', icon: icon(NotebookIcon), run: linkNote },
    { text: '添加链接', icon: icon(LinkIcon), run: () => applyFormat('link') },
  ], { anchor, keyboard, label: uiText(attachMenuKey), onError: message => setStatus(message, true), restoreFocus: focusEditor });
}
function attachFromKeyboard() {
  if (!currentWritingEditor()) return;
  if (!canAddAttachments) return;
  richEditor.captureSelection();
  attachmentInput.accept = '';
  attachmentInput.click();
}
attachButton.onclick = event => openAttachmentMenu(attachButton, event.detail === 0);
const quickAttach = attachButton.cloneNode(true) as HTMLButtonElement;
quickAttach.removeAttribute('id');
quickAttach.onclick = event => openAttachmentMenu(quickAttach, event.detail === 0);
mobileToolsGroup.append(quickAttach);
const dismissKeyboard = document.createElement('button');
dismissKeyboard.type = 'button';
dismissKeyboard.className = 'dismiss-keyboard';
dismissKeyboard.ariaLabel = translatedLabel('隐藏键盘');
dismissKeyboard.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="3.5" width="19" height="12" rx="2"/><path d="M6 7h.1M10 7h.1m4 0h.1m4 0h.1M6 11h.1m3.9 0h4m4 0h.1m-8.6 7.5 2.5 2.5 2.5-2.5"/></svg>';
dismissKeyboard.onclick = () => (document.activeElement as HTMLElement | null)?.blur();
mobileTools.append(dismissKeyboard);
async function insertAttachments(files: File[]) {
  if (!files.length || !currentWritingEditor()) return;
  if (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') { setStatus('正在处理其他操作，请稍后添加附件', true); return; }
  const id = notebook.current.id;
  richEditor.captureSelection();
  try {
    const maxImportBytes = 256 * 1024 * 1024;
    if (files.some(file => file.size > MAX_ASSET_BYTES) || files.reduce((size, file) => size + file.size, 0) > maxImportBytes) throw new Error('单附件不能超过 64 MiB，每次最多 256 MiB');
    if (!await saveDraft(true)) return;
    document.body.dataset.libraryBusy = 'true';
    $('#app-shell').inert = true;
    for (const file of files) {
      const attachment = await notebook.importAttachment(id, { name: file.name, bytes: Array.from(new Uint8Array(await file.arrayBuffer())) });
      if (notebook.current?.id !== id) throw new Error('当前笔记已切换，附件文件已保留，请重新插入');
      const markdown = attachmentMarkdown(attachment) + '\n\n';
      $('#app-shell').inert = false;
      richEditor.insert(markdown);
      $('#app-shell').inert = true;
      if (!await saveDraft(true)) throw new Error('附件已保留，但笔记尚未保存，请保持当前窗口打开后重试');
    }
    setStatus('图片与附件已保存');
  } catch (error) { setStatus(error instanceof Error ? error.message : String(error), true); }
  finally { document.body.dataset.libraryBusy = 'false'; $('#app-shell').inert = false; }
}
attachmentInput.onchange = () => { const files = Array.from(attachmentInput.files ?? []); attachmentInput.value = ''; void insertAttachments(files); };
workspace.addEventListener('dragover', event => { if (event.dataTransfer?.types.includes('Files')) event.preventDefault(); });
workspace.addEventListener('drop', event => { if (!event.dataTransfer?.files.length) return; event.preventDefault(); event.stopImmediatePropagation(); void insertAttachments(Array.from(event.dataTransfer.files)); }, true);
workspace.addEventListener('paste', event => { if (!event.clipboardData?.files.length) return; event.preventDefault(); event.stopImmediatePropagation(); void insertAttachments(Array.from(event.clipboardData.files)); }, true);
// Toolbar presses must not dismiss the keyboard or move the editor selection.
mountGlassToolbar($('.actions'));
if (!mobile) mountGlassToolbar($('.notes-navigation'));
if (mobile) mountGlassButton($<HTMLButtonElement>('#new-note-button'));
for (const bar of [$('.actions'), menu, mobileTools]) bar.addEventListener('pointerdown', event => {
  if ((event.target as HTMLElement).closest('button')) event.preventDefault();
});
if (mobile) keepEditorFocusForTouchButtons(mobileTools, button => !mobileSuggestions.contains(button));
document.addEventListener('pointerdown', event => {
  if (!menu.contains(event.target as Node) && !$('#format-button').contains(event.target as Node)) closeMenu();
});
if (mobile) window.addEventListener('popstate', () => { closeMenu(); closeFormatPanel?.(); });
// 安卓返回键先交给这里：有菜单、弹窗或搜索就只收起最上面那一层，返回 true 表示已处理；
// 设置自己写了浏览记录，交给 WebView 后退。MainActivity 收到 false 才走默认后退。
if (mobile) Object.assign(window, { qingyuBack() {
  if (popupMenuOpen()) { dismissPopupMenu(); return true; }
  if (history.state?.settings) return false;
  const dialog = [...document.querySelectorAll<HTMLDialogElement>('dialog[open]')].at(-1);
  if (dialog) {
    if (dialog.dispatchEvent(new Event('cancel', { cancelable: true }))) dialog.close();
    return true;
  }
  const popover = [...document.querySelectorAll<HTMLElement>(':popover-open')].at(-1);
  if (popover) { popover.hidePopover(); return true; }
  if (closeFormatPanel) { closeFormatPanel(); return true; }
  if (!menu.hidden) { closeMenu(); return true; }
  return Boolean(sidebar?.cancelSearch());
} });
function findInNote(direction?: 'next' | 'previous') {
  if (document.querySelector('dialog[open], .search-picker:popover-open') || richEditor.composing) return false;
  if (!showingNote) {
    if (direction) return false;
    $('#library-search-button').click();
    return true;
  }
  return richEditor.find(direction);
}
let quickOpen: ReturnType<typeof openQuickOpen> | undefined;
function toggleQuickOpen() {
  if (!desktop || documentWindow || !sidebar || notebook.external || notebook.loadError || $('#app-shell').inert ||
    document.querySelector('dialog[open], .search-picker:not(.quick-open):popover-open') || popupMenuOpen() ||
    richEditor.composing) return false;
  if (quickOpen) {
    if (quickOpen.panel.contains(document.activeElement)) quickOpen.close();
    else quickOpen.focus();
    return true;
  }
  const previous = document.activeElement;
  const currentId = notebook.current?.id;
  const currentEditor = currentWritingEditor();
  const previousDoc = currentEditor?.state.doc;
  const cell = previous?.closest('.cm-markra-table') ? richEditor.captureTableFormat() : undefined;
  const root = notebook.root;
  closeMenu();
  quickOpen = openQuickOpen({
    library: notebook.data,
    commands() {
      const rows: QuickCommand[] = [
        { title: '新建笔记', icon: SquarePen, shortcut: '⌘N', keywords: ['new note', 'create'], run: () => { const compose = $<HTMLButtonElement>('#new-note-button'); if (compose.disabled) focusEditor(); else compose.click(); } },
        { title: '今日笔记', icon: Calendar, shortcut: '⌘T', keywords: ['today', 'daily'], run: () => { void openTodayNote(); } },
        { title: '新建文件夹', icon: FolderPlus, shortcut: '⇧⌘N', keywords: ['new folder'], run: () => $('#new-folder-button').click() },
      ];
      const note = notebook.current;
      if (showingNote && note && !note.trashed) rows.push({
        title: note.pinned ? '取消置顶' : '置顶笔记', icon: note.pinned ? PinOff : Pin,
        detail: noteTitle(note), keywords: ['pin', 'unpin'],
        run: () => { void notebook.togglePin(note.id).then(() => sidebar?.refresh()).catch(error => setStatus(String(error), true)); },
      });
      if (showingNote && note && !note.trashed && notebook.data.folders.some(folder => folder.id !== note.folderId)) rows.push({
        title: '移动笔记…', icon: FolderInput, detail: noteTitle(note), keywords: ['move', 'folder'], run: () => { void sidebar?.moveCurrentNote(); },
      });
      if (native) rows.push(
        { title: '导入文件…', icon: FileDown, keywords: ['import files'], run: () => { void emitTo('main', 'library-command', 'library-import'); } },
        { title: '导入文件夹…', icon: FileDown, keywords: ['import folder'], run: () => { void emitTo('main', 'library-command', 'library-import-folder'); } },
      );
      else if (serverLibrary) rows.push(
        { title: '导入文件…', icon: FileDown, keywords: ['import files'], run: () => chooseImport(false) },
        { title: '导入文件夹…', icon: FileDown, keywords: ['import folder'], run: () => chooseImport(true) },
      );
      return rows.concat({ title: uiText('menu.settings'), icon: Settings, shortcut: '⌘,', keywords: ['settings', 'preferences'], run: () => $('#settings-button').click() });
    },
    async openNote(note) {
      if (notebook.root !== root || !notebook.data.notes.some(item => item.id === note.id && !item.trashed) || !await selectNote(note.id)) return;
      sidebar?.revealCurrent();
      focusEditor();
    },
    showAll: query => sidebar?.searchAll(query),
    restore() {
      if (currentId === notebook.current?.id && previousDoc === currentEditor?.state.doc && cell?.restore()) return;
      if (previous instanceof HTMLElement && previous.isConnected && !previous.closest('[inert]')) previous.focus({ preventScroll: true });
    },
    closed() { quickOpen = undefined; },
  });
  return true;
}

/** 网页客户端的导入：浏览器只能在点击时打开选择窗口，选好后交给笔记库导入，之后和 App 一样显示当前笔记。 */
function chooseImport(directory: boolean) {
  const input = document.createElement('input');
  input.type = 'file'; input.multiple = true;
  if (directory) input.webkitdirectory = true; else input.accept = '.md,.markdown,.txt';
  input.onchange = () => {
    const files = [...input.files ?? []];
    if (!files.length) return;
    void notebook.importUploads(files).then(() => {
      showCurrentNote(Boolean(notebook.current)); sidebar?.revealCurrent(); setStatus('已导入笔记');
    }).catch(error => setStatus(error instanceof Error ? error.message : String(error), true));
  };
  input.click();
}

function commandK() {
  if (richEditor.composing || $('#app-shell').inert ||
    document.querySelector('dialog[open], .search-picker:not(.quick-open):popover-open') || popupMenuOpen()) return false;
  const target = document.activeElement;
  const editor = currentWritingEditor();
  if (editor && target && editor.contentDOM.contains(target)) {
    const cell = target.closest('.cm-markra-table');
    const selection = cell ? window.getSelection() : null;
    const picked = cell ? selection && !selection.isCollapsed && cell.contains(selection.anchorNode) && cell.contains(selection.focusNode)
      : target === editor.contentDOM && !editor.state.selection.main.empty;
    if (picked) { applyFormat('link'); return true; }
  }
  return toggleQuickOpen();
}

function foldSections(expanded: boolean) {
  if (!showingNote || richEditor.composing || document.querySelector('dialog[open], .search-picker:popover-open')) return false;
  void richEditor.foldHeadings(expanded);
  return true;
}
document.addEventListener('keydown', event => {
  if (desktop && document.documentElement.dataset.platform !== 'macos' && !event.isComposing && event.shiftKey && !event.altKey &&
    (/Mac/.test(navigator.platform) ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) && event.key.toLowerCase() === 'd') {
    const editor = currentWritingEditor();
    if (editor?.contentDOM === document.activeElement && !editor.compositionStarted) {
      event.preventDefault(); event.stopPropagation(); applyFormat('date'); return;
    }
  }
  if (desktop && !event.isComposing && !event.altKey && !event.shiftKey && !event.repeat &&
    (/Mac/.test(navigator.platform) ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey) &&
    event.key.toLowerCase() === 'k' && commandK()) {
    event.preventDefault(); event.stopPropagation(); return;
  }
  if (!event.isComposing && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey
      && event.key.toLowerCase() === 'f' && findInNote()) {
    event.preventDefault(); event.stopPropagation(); return;
  }
  if (desktop && !event.isComposing && event.altKey && event.shiftKey
    && (/Mac/.test(navigator.platform) ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && (event.key === 'ArrowLeft' || event.key === 'ArrowRight') && foldSections(event.key === 'ArrowRight')) {
    event.preventDefault(); event.stopPropagation(); return;
  }
  if (desktop && document.documentElement.dataset.platform !== 'macos' && !event.isComposing
    && event.ctrlKey && !event.shiftKey && (/Mac/.test(navigator.platform) ? event.metaKey && !event.altKey : event.altKey && !event.metaKey)
    && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
    const editor = currentWritingEditor();
    if (editor && editor.contentDOM === document.activeElement) {
      if (moveEditorItem(editor, event.key === 'ArrowUp')) closeMenu();
      event.preventDefault(); event.stopPropagation(); return;
    }
  }
  const backKey = event.key === '[' || event.code === 'BracketLeft' && !event.key;
  const forwardKey = event.key === ']' || event.code === 'BracketRight' && !event.key;
  if (desktop && !event.isComposing && (event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey
    && (backKey || forwardKey) && !document.querySelector('dialog[open], .search-picker:popover-open')) {
    const step = backKey ? -1 : 1;
    if (!(step < 0 ? noteBack : noteForward).disabled) {
      event.preventDefault(); event.stopPropagation();
      void stepHistory(step);
      return;
    }
  }
  if (!view.dom.contains(document.activeElement) || event.isComposing || (!event.metaKey && !event.ctrlKey)) return;
  const action = formatShortcuts[`${event.altKey ? 'Alt-' : ''}${event.shiftKey ? 'Shift-' : ''}${event.code}`];
  if (!action || action === 'attach' && (mobile || !canAddAttachments)) return;
  event.preventDefault(); event.stopPropagation();
  if (action === 'attach') attachFromKeyboard();
  else applyFormat(action);
}, true);
function resizeNoteText(action: 'bigger' | 'smaller' | 'actual') {
  try { changeNoteTextSize(action); }
  catch (error) { setStatus(error instanceof Error ? error.message : '无法保存字号', true); }
}
document.addEventListener('keydown', event => {
  const action = noteTextSizeShortcut(event);
  if (!desktop || !action || document.querySelector('dialog[open]')) return;
  event.preventDefault();
  resizeNoteText(action);
});
document.addEventListener('keydown', event => {
  if (event.isComposing) return;
  if (event.key === 'Escape') closeMenu();
  if (!event.metaKey && !event.ctrlKey) return;
  if (event.key.toLowerCase() === 's') { event.preventDefault(); clearTimeout(saveTimer); void saveDraft(true); }
});
function preserveOnLeave() {
  if (!showingNote) return true;
  if (!notebook.server) { void saveDraft(true); return true; }
  // Navigation can abort HTTP saves; the per-note draft is recovered after reopening.
  try { notebook.stageDraft(sourceForSave()); return true; }
  catch { setStatus('本地草稿备份失败，请保持页面打开并等待服务器保存', true); return false; }
}
window.addEventListener('pagehide', preserveOnLeave);
document.addEventListener('visibilitychange', () => { if (document.hidden) preserveOnLeave(); });

let nativeKeyboard = false;
function updateViewport() {
  const viewport = window.visualViewport;
  const height = viewport?.height ?? innerHeight;
  document.body.style.height = `${height}px`;
  if (mobile) keepMobileViewportAtTop();
  const editing = document.activeElement?.closest('#rich-editor [contenteditable="true"]');
  mobileTools.hidden = !mobile || !showingNote || Boolean(notebook.current?.readOnly) || !editing;
  const inTable = Boolean(editing?.matches('.cm-markra-table'));
  mobileSuggestions.hidden = inTable;
  quickLink.hidden = !inTable;
  quickAttach.hidden = inTable;
  mobileToolsGroup.toggleAttribute('data-in-table', inTable);
  view.requestMeasure();
  requestAnimationFrame(() => {
    if (mobile) {
      // Read the root layout before measuring WebKit's visual viewport after a keyboard resize.
      const layoutHeight = document.documentElement.clientHeight;
      document.body.classList.toggle('keyboard-open', nativeKeyboard || layoutHeight - (viewport?.height ?? layoutHeight) > 100);
    }
    if (!richEditor.composing && !closeFormatPanel) revealCaret(richEditor.element);
  });
}
window.visualViewport?.addEventListener('resize', updateViewport);
window.addEventListener('resize', updateViewport);
if (mobile) {
  window.addEventListener('scroll', keepMobileViewportAtTop);
  window.visualViewport?.addEventListener('scroll', keepMobileViewportAtTop);
  for (const event of ['focusin', 'focusout']) workspace.addEventListener(event, () => queueMicrotask(updateViewport));
}

window.addEventListener('native-keyboard', event => { nativeKeyboard = Boolean((event as CustomEvent).detail); updateViewport(); });
document.documentElement.toggleAttribute('data-web', !native);
if ((mobile || !native || document.documentElement.dataset.platform === 'macos') && !documentWindow) {
  if (native && !mobile) $('#app-shell').dataset.macSidebarFoot = '';
  const account = $<HTMLButtonElement>('#sidebar-account');
  if (!native && mobile) {
    account.title = account.ariaLabel = uiText('settings.title');
  }
  account.onclick = () => { void showSettings(!mobile && (native || serverLibrary) ? 'account' : undefined).catch(() => setStatus('无法打开设置', true)); };
  if (native || serverLibrary) {
    const refreshAccount = sidebarAccount(account);
    window.addEventListener('focus', refreshAccount);
    void refreshAccount();
  }
}
sidebar = createSidebar(() => notebook, {
  select: selectNote, create: createNote, openToday: openTodayNote, focusEditor, blankNote, save: () => saveDraft(true),
  share: shareNote,
  quickOpen: toggleQuickOpen,
  thumbnail(note) {
    const image = firstLocalImage(note.source);
    return image ? attachmentUrl(notebook, note.id, image) : undefined;
  },
  mountThumbnail(note, image, row, url) {
    if (!native || notebook.data.sync?.version !== 2) return;
    const source = firstLocalImage(note.source);
    if (source) return mountNoteAssetImage(note.id, image, row, url, source, true);
  },
  noteWindow,
  openNoteWindow: desktop && !documentWindow && !notebook.loadError
    ? native ? id => invoke('workspace_open_note', { noteId: id }) : serverLibrary ? openBrowserNote : undefined
    : undefined,
  libraryChanged: () => { richEditor.refreshNoteLinks(); updateShareMarker(); },
  reload: () => showCurrentNote(Boolean(notebook.current)), status: setStatus, error: message => setStatus(message, true),
}, mobile);
showCurrentNote(showingNote);
window.addEventListener('ui-language-change', () => {
  localizeLabels();
  if (lastSyncState) renderSyncState(lastSyncState);
  if (!native && mobile && !documentWindow) {
    const account = $('#sidebar-account');
    account.title = account.ariaLabel = uiText('settings.title');
  }
  sidebar?.refresh();
  richEditor.refreshLanguage();
  refreshTitle();
  updateCompose();
});
if (!notebook.loadError) {
  if (!documentWindow && preferences.value.todayOpenOnLaunch) await openTodayNote();
  else void saveDraft();
}
refreshTitle();
updateViewport();
if (native && desktop) showWindowWhenPainted();
watchWebSession(async expired => {
  if (expired && notebook.server) {
    if (richEditor.composing || document.querySelector<HTMLInputElement>('#rename-note')?.dataset.composing === 'true') return false;
    return preserveOnLeave();
  }
  if (syncBusy || document.body.dataset.libraryBusy === 'true') { setStatus('正在保存或同步，请稍后退出', true); return false; }
  return saveDraft(true);
}, message => setStatus(message, true));
if (serverLibrary && !notebook.loadError) {
  offerServerDrafts(notebook.data);
  const refresh = async () => {
    const current = notebook.current, id = notebook.data.activeId, source = sourceForSave();
    const ready = () => {
      return !document.body.inert && document.body.dataset.libraryBusy !== 'true'
      && !document.querySelector('#rename-note,.rename-folder,dialog[open],.note-context-menu')
      && !notebook.loadError && !saveTimer && !richEditor.composing
      && id === notebook.data.activeId && source === sourceForSave()
      && (!notebook.current || source.replace(/\r\n?/g, '\n') === notebook.current.source.replace(/\r\n?/g, '\n'));
    };
    try {
      if (await notebook.refreshServer(ready, browserNoteId ?? undefined)) {
        const next = notebook.current;
        if (browserNoteId && next?.id !== browserNoteId) {
          if (showingNote) showCurrentNote(false);
        } else if (browserNoteId && !showingNote || showingNote && (current?.id !== next?.id || current?.source !== next?.source || current?.readOnly !== next?.readOnly)) showCurrentNote(Boolean(next));
        else refreshTitle();
        sidebar?.refresh();
      }
    }
    catch (error) { setStatus(error instanceof Error ? error.message : '无法刷新服务器笔记', true); }
  };
  window.addEventListener('focus', refresh);
  setInterval(() => { if (!document.hidden) void refresh(); }, 15_000);
  // 部署端同步完成后立即读回笔记；和 App 同步后一样，没在显示笔记时打开当前笔记
  window.addEventListener('web-sync-finished', async () => {
    await refresh();
    if (!showingNote && !browserNoteId && notebook.current) { showCurrentNote(true); sidebar?.revealCurrent(); }
  });
  // 和 App 一样，删除、移动、编辑等本地改动后同步一次；定时同步在部署端
  const webSyncClient = createServerSyncClient();
  const webSync = startAutoSync(webSyncClient, () => document.body.inert || notebook.loadError || document.body.dataset.libraryBusy === 'true',
    message => setStatus(message, true), mobile, preferences, false);
  notebook.onLocalChange = webSync.changed;
  // 和 App 一样，登录轻羽账号后为这个账号开启云存储自动同步
  void connectCloudAutoSync(webSyncClient, preferences, () => webSync.sync(), message => setStatus(message, true)).catch(() => setStatus('无法读取账号同步状态', true));
  window.addEventListener('beforeunload', event => {
    if (showingNote && notebook.current && (richEditor.composing || saveTimer || sourceForSave().replace(/\r\n?/g, '\n') !== notebook.current.source.replace(/\r\n?/g, '\n'))) { event.preventDefault(); event.returnValue = ''; }
  });
}
if ('__TAURI_INTERNALS__' in window) {
  let autoSync: ReturnType<typeof startAutoSync> | undefined;
  if (!documentWindow) {
    const { setupFileLibrary } = await import('./file-library-ui');
    await setupFileLibrary(notebook, {
      save: () => saveDraft(true),
      openToday: openTodayNote,
      quickOpen: toggleQuickOpen,
      prepareExit: mobile ? undefined : () => invoke('workspace_prepare_exit'),
      cancelExit: mobile ? undefined : () => invoke('workspace_cancel_exit'),
      sync: async () => { await autoSync?.sync(); },
      refresh() { showCurrentNote(Boolean(notebook.current)); sidebar?.revealCurrent(); setStatus('已读取本地笔记文件夹'); }, error: message => setStatus(message, true),
    });
  }
  const { setupFileWatch } = await import('./file-watch');
  await setupFileWatch(notebook, {
    pending: () => Boolean(saveTimer || richEditor.composing || document.querySelector<HTMLInputElement>('#rename-note')?.dataset.composing === 'true'),
    buffer() { return showingNote && notebook.current ? { noteId: notebook.current.id, source: sourceForSave() } : undefined; },
    refresh(previous) {
      const current = notebook.current;
      if (previous?.id !== current?.id || previous?.source !== current?.source || previous?.readOnly !== current?.readOnly) {
        const selection = view.state.selection.main, top = view.scrollDOM.scrollTop;
        const before = view.state.doc.toString();
        showCurrentNote(Boolean(current));
        if (previous?.id === current?.id && current) {
          richEditor.markChanges(before);
          view.dispatch({ selection: { anchor: Math.min(selection.anchor, view.state.doc.length), head: Math.min(selection.head, view.state.doc.length) } });
          view.scrollDOM.scrollTop = top;
        } else refreshTitle(true);
      } else refreshTitle();
      if (current && previous?.id === current.id && previous.path !== current.path && !current.readOnly && sourceForSave().replace(/\r\n?/g, '\n') !== current.source.replace(/\r\n?/g, '\n')) {
        clearTimeout(saveTimer); saveTimer = setTimeout(() => { void saveDraft(); }, 500);
      }
      sidebar?.refresh();
      autoSync?.changed();
      setStatus('已载入外部文件变化');
    },
    backedUp(paths) { setStatus(notebook.external ? `文件已在外部变化，你的版本已备份到：${paths.join('、')}` : '文件已在外部变化，你的版本已备份到笔记目录的 .miaoyan-conflicts', true); },
    error: message => setStatus(message, true),
  }, windowLabel);
  if (!mobile) {
    const { setupWorkspace } = await import('./workspace');
    await setupWorkspace(windowLabel, {
      save: () => saveDraft(true),
      select: noteWindow ? undefined : async path => {
        const previous = notebook.current;
        await notebook.activateExternal(path);
        const current = notebook.current;
        if (previous?.id !== current?.id || previous?.source !== current?.source || previous?.readOnly !== current?.readOnly) showCurrentNote(Boolean(current));
        sidebar?.revealCurrent();
      },
      error: message => setStatus(message, true),
    });
  }
  if (!documentWindow) {
    await connectCloudAutoSync(syncSession, preferences, async () => { await autoSync?.sync(); }, message => setStatus(message, true)).catch(() => setStatus('无法监听账号同步状态', true));
    autoSync = startAutoSync(syncSession, () => document.body.inert || notebook.loadError || document.body.dataset.libraryBusy === 'true', message => setStatus(message, true), mobile, preferences);
    notebook.onLocalChange = autoSync.changed;
  }
}
window.addEventListener('preferences-applied', richEditor.requestMeasure);
window.addEventListener('settings-error', event => setStatus((event as CustomEvent<string>).detail, true));
if (native && desktop && !documentWindow) void listen<string>('note-text-size', ({ payload }) => {
  if (payload === 'bigger' || payload === 'smaller' || payload === 'actual') resizeNoteText(payload);
}, { target: 'main' });
if (native && desktop) void listen<string>('note-sections', ({ payload }) => {
  if (payload === 'collapse' || payload === 'expand') foldSections(payload === 'expand');
}, { target: windowLabel }).catch(console.warn);
if (native && desktop) void listen<string>('note-find', ({ payload }) => {
  if (payload === 'open') findInNote();
  else if (payload === 'previous' || payload === 'next') findInNote(payload);
}, { target: windowLabel }).catch(console.warn);
void connectPreferences().catch(() => setStatus('无法应用系统窗口设置', true));
if (mobile) connectMobileEditorHistory($<HTMLButtonElement>('#note-undo'), () => currentWritingEditor() ?? undefined);
else void connectEditorHistoryMenu({
  current: currentWritingEditor,
  apply(action) {
    if (['title', 'heading', 'subheading', 'body', 'bullet', 'list', 'ordered', 'quote', 'codeblock'].includes(action)
        && !document.activeElement?.closest('.cm-markra-table')) {
      richEditor.applyFormatPanel(action);
      closeMenu();
      focusEditor();
    } else applyFormat(action);
  },
}).catch(console.warn);
if (!documentWindow) void hostSync(syncSession).catch(() => setStatus('无法连接同步设置', true));
// 菜单栏面板的新建和打开都在这里做，和侧栏、编辑器走同一套保存流程。
if (native && desktop && !documentWindow) void hostMenuBar(() => notebook, {
  blocked() {
    if (notebook.loadError) return '笔记库还没打开';
    if (notebook.external) return '请回到笔记库后再试';
    if (document.body.dataset.libraryBusy === 'true') return '正在整理笔记库，请稍后再试';
  },
  async create(title, source) {
    if (!await saveDraft(true)) throw new Error('当前笔记还没保存好，请稍后再试');
    await notebook.createNote(notebook.data.folders[0].id, title);
    const id = notebook.data.activeId;
    await notebook.save(source, id);
    showCurrentNote(true);
    sidebar?.revealCurrent();
    return { id, title };
  },
  async open(id) {
    if (!await selectNote(id)) throw new Error('无法打开这篇笔记');
    sidebar?.revealCurrent();
  },
}).catch(console.warn);
