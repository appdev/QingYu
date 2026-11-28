import { ArrowUpRight, Copy, Image, ListTodo, Lock, Mic, MoreHorizontal, Paperclip, Pause, Pencil, Pin, PinOff, Play, Plus, Search, Trash2, X } from 'lucide';
import { askToConfirm } from './alert-dialog';
import { assetBase, attachmentMarkdown, attachmentUrl } from './attachments';
import { isLocalAsset } from './server-assets';
import { MAX_ASSET_BYTES } from './sync-files';
import { folderRows } from './folder-tree';
import { allMemos, findMemoFolder, groupMemosByDay, isInsideMemoFolder, memoNotes, memoTagCounts, memoTags, memoTagKey, renderMemo, toggleTask, type MemoFilter } from './memo';
import { showMoveFolderSheet } from './note-menu';
import { showMenu, type MenuItem, type MenuOptions } from './popup-menu';
import { NoteSaveConflict, type Note, type Notebook } from './notebook';
import { createRichEditor, type RichEditor } from './rich-editor';
import { translatedLabel, uiText } from './ui-language';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { usesGlisse } from './web-glass-mount';

type DraftAttachment = { file: File; image: boolean; preview?: string };
type AudioRecording = { release?: () => void; stop?: () => void; timer?: ReturnType<typeof setInterval> };

// 按麦克风音量驱动波形：音量上来时从容跟进、落下时缓缓回落，通过整条轨道的连续位移实现平滑滚动；
// 静音时保留极轻微的呼吸起伏，避免画面像卡住一样静止。系统要求减少动态效果时不启动。
function startWaveMeter(stream: MediaStream, wave: HTMLElement, host: HTMLElement) {
  const track = wave.firstElementChild as HTMLElement;
  const bars = [...track.children] as HTMLElement[];
  const reset = () => {
    track.style.transform = '';
    host.style.removeProperty('--memo-level');
    for (const bar of bars) { bar.style.transform = ''; bar.style.opacity = ''; }
  };
  reset();
  const Context = window.AudioContext ?? (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Context || matchMedia('(prefers-reduced-motion: reduce)').matches) return reset;
  let context: AudioContext;
  try { context = new Context(); } catch { return reset; }
  void context.resume().catch(() => {});
  const source = context.createMediaStreamSource(stream), analyser = context.createAnalyser();
  analyser.fftSize = 512; analyser.smoothingTimeConstant = 0.82;
  source.connect(analyser);
  const samples = new Uint8Array(analyser.fftSize), levels = bars.map(() => 0);
  const step = 80, pitch = 5;
  let level = 0, glow = 0, last = 0, frame = requestAnimationFrame(function draw(now) {
    frame = requestAnimationFrame(draw);
    analyser.getByteTimeDomainData(samples);
    let sum = 0;
    for (const value of samples) { const offset = (value - 128) / 128; sum += offset * offset; }
    const rms = Math.sqrt(sum / samples.length);
    const target = Math.pow(Math.min(1, Math.max(0, rms - 0.008) * 5.2), 0.72);
    level += (target - level) * (target > level ? 0.26 : 0.08);
    glow += (level - glow) * 0.12;
    if (!last) last = now;
    if (now - last >= step) {
      last = now - ((now - last) % step);
      const breath = 0.032 * (0.5 + 0.5 * Math.sin(now * 0.0026));
      levels.shift();
      levels.push(Math.max(level, breath));
      bars.forEach((bar, index) => {
        const value = levels[index]!;
        bar.style.transform = `scaleY(${(0.14 + value * 0.86).toFixed(3)})`;
        bar.style.opacity = (0.34 + value * 0.6).toFixed(2);
      });
      host.style.setProperty('--memo-level', glow.toFixed(3));
    }
    const phase = Math.min(1, Math.max(0, (now - last) / step));
    track.style.transform = `translate3d(${(-phase * pitch).toFixed(2)}px, 0, 0)`;
  });
  return () => { cancelAnimationFrame(frame); reset(); source.disconnect(); void context.close().catch(() => {}); };
}
const audioFormats = [
  ['audio/mp4;codecs=mp4a.40.2', 'm4a'], ['audio/webm;codecs=opus', 'webm'], ['audio/ogg;codecs=opus', 'ogg'], ['audio/mp4', 'm4a'],
] as const;
const audioTypes: Readonly<Record<string, string>> = {
  m4a: 'audio/mp4', mp3: 'audio/mpeg', wav: 'audio/wav', ogg: 'audio/ogg',
  opus: 'audio/ogg', aac: 'audio/aac', flac: 'audio/flac',
};
function audioType(path: string) {
  const filename = path.split(/[?#]/, 1)[0].split('/').at(-1)!.toLowerCase();
  const extension = filename.match(/\.([a-z0-9]+)$/u)?.[1] ?? '';
  if (Object.hasOwn(audioTypes, extension)) return audioTypes[extension];
  if (/^voice-[\dt-]+\.webm$/u.test(filename)) return 'audio/webm';
}
const taskPrefix = /^([ \t]*[-+*] \[[ xX]\] )/u;
const message = (error: unknown) => error instanceof Error ? error.message : String(error);
const icon = (glyph: Parameters<typeof createLucideIcon>[1]) => createLucideIcon(document, glyph, '');
const locale = () => document.documentElement.lang || 'zh-CN';
// 置顶组里的随记可能是很多天前写的，只显示时分看不出是哪天，所以带上日期；跨年再加年份。
function memoStamp(note: Note, language: string) {
  const date = new Date(note.createdAt);
  return new Intl.DateTimeFormat(language, {
    ...(note.pinned ? { month: 'short', day: 'numeric', ...(date.getFullYear() !== new Date().getFullYear() ? { year: 'numeric' } : {}) } as const : {}),
    hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
  }).format(date);
}
const filterName = (filter: MemoFilter) => filter.kind === 'tag' ? '#' + filter.tag : filter.kind === 'todo' ? uiText('memoTodo') : '';
const composerPrompt = (filter: MemoFilter) => filter.kind === 'tag'
  ? uiText('memoPlaceholderTag').replace('{tag}', () => filter.tag) : uiText('memoPlaceholder');

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = '', value = '') {
  const node = document.createElement(tag);
  node.className = className; node.textContent = value;
  return node;
}

function button(className: string, label: string, glyph?: Parameters<typeof icon>[0]) {
  const node = element('button', className, glyph ? '' : label);
  node.type = 'button';
  if (glyph) { node.append(icon(glyph)); node.title = node.ariaLabel = label; }
  return node;
}

function formatClock(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) return '00:00';
  const total = Math.floor(seconds);
  return [Math.floor(total / 60), total % 60].map(part => String(part).padStart(2, '0')).join(':');
}

// 用文件名生成稳定、错落自然的静态波形轮廓，让每条语音播放条都有辨识度且保持克制。
function waveformBars(seed: string, count = 28) {
  let hash = 2166136261;
  for (let i = 0; i < seed.length; i++) hash = Math.imul(hash ^ seed.charCodeAt(i), 16777619);
  return Array.from({ length: count }, (_, index) => {
    hash = Math.imul(hash ^ (index + 1), 16777619);
    const random = ((hash >>> 0) % 1000) / 1000;
    const envelope = Math.sin(((index + 0.5) / count) * Math.PI);
    return Math.min(0.94, Math.max(0.24, 0.24 + (0.26 + random * 0.5) * (0.45 + envelope * 0.55)));
  });
}

function createAudioPlayer(options: {
  source?: string;
  label: string;
  load?: () => Promise<string>;
  onPlay?: (audio: HTMLAudioElement) => void;
  onError?: (error: unknown) => void;
}) {
  const wrap = element('div', 'memo-audio');
  const audio = element('audio');
  audio.preload = options.source ? 'metadata' : 'none';
  audio.ariaLabel = options.label;
  if (options.source) audio.src = options.source;
  const toggle = button('memo-audio-toggle memo-play-audio', '');
  toggle.title = toggle.ariaLabel = uiText('memoPlayAudio');
  toggle.ariaPressed = 'false';
  const playGlyph = icon(Play), pauseGlyph = icon(Pause);
  playGlyph.classList.add('memo-audio-icon-play');
  pauseGlyph.classList.add('memo-audio-icon-pause');
  toggle.append(playGlyph, pauseGlyph);
  const track = element('div', 'memo-audio-track');
  track.role = 'slider';
  track.tabIndex = 0;
  track.ariaLabel = options.label;
  track.setAttribute('aria-valuemin', '0');
  track.setAttribute('aria-valuemax', '100');
  track.setAttribute('aria-valuenow', '0');
  const heights = waveformBars(options.label);
  const makeBars = (className: string) => {
    const layer = element('span', 'memo-audio-bars ' + className);
    layer.ariaHidden = 'true';
    layer.append(...heights.map(height => {
      const bar = element('i');
      bar.style.setProperty('--h', height.toFixed(3));
      return bar;
    }));
    return layer;
  };
  track.append(makeBars('memo-audio-bars-bg'), makeBars('memo-audio-bars-fg'));
  const time = element('span', 'memo-audio-time', '00:00');
  wrap.append(audio, toggle, track, time);

  let raf = 0, loading = false, seeking = false, loadedUrl: string | undefined;
  const stopLoop = () => { if (raf) { cancelAnimationFrame(raf); raf = 0; } };
  const renderState = () => {
    const duration = Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : 0;
    const current = Math.min(duration || audio.currentTime, Math.max(0, audio.currentTime));
    const ratio = duration > 0 ? Math.min(1, current / duration) : 0;
    track.style.setProperty('--progress', (ratio * 100).toFixed(2) + '%');
    track.setAttribute('aria-valuenow', String(Math.round(ratio * 100)));
    if (duration > 0) {
      time.textContent = current > 0 || !audio.paused ? `${formatClock(current)} / ${formatClock(duration)}` : formatClock(duration);
    } else {
      time.textContent = formatClock(current);
    }
  };
  const stepLoop = () => {
    renderState();
    if (!audio.paused && !audio.ended) raf = requestAnimationFrame(stepLoop);
    else raf = 0;
  };
  const seekByClientX = (clientX: number) => {
    const duration = audio.duration;
    if (!Number.isFinite(duration) || duration <= 0) return;
    const rect = track.getBoundingClientRect();
    if (!rect.width) return;
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    audio.currentTime = ratio * duration;
    renderState();
  };
  track.addEventListener('pointerdown', event => {
    if (event.button !== 0 || !Number.isFinite(audio.duration) || audio.duration <= 0) return;
    seeking = true;
    track.setPointerCapture?.(event.pointerId);
    seekByClientX(event.clientX);
  });
  track.addEventListener('pointermove', event => { if (seeking) seekByClientX(event.clientX); });
  const endSeek = () => { seeking = false; };
  track.addEventListener('pointerup', endSeek);
  track.addEventListener('pointercancel', endSeek);
  track.addEventListener('keydown', event => {
    if (!Number.isFinite(audio.duration) || audio.duration <= 0) return;
    const delta = event.key === 'ArrowRight' ? 5 : event.key === 'ArrowLeft' ? -5 : 0;
    if (!delta) return;
    event.preventDefault();
    audio.currentTime = Math.min(audio.duration, Math.max(0, audio.currentTime + delta));
    renderState();
  });
  audio.addEventListener('loadedmetadata', renderState);
  audio.addEventListener('durationchange', renderState);
  audio.addEventListener('timeupdate', renderState);
  audio.addEventListener('play', () => {
    options.onPlay?.(audio);
    wrap.toggleAttribute('data-playing', true);
    toggle.ariaPressed = 'true';
    stopLoop();
    raf = requestAnimationFrame(stepLoop);
  });
  const onStop = () => {
    stopLoop();
    wrap.removeAttribute('data-playing');
    toggle.ariaPressed = 'false';
    renderState();
  };
  audio.addEventListener('pause', onStop);
  audio.addEventListener('ended', () => {
    stopLoop();
    wrap.removeAttribute('data-playing');
    toggle.ariaPressed = 'false';
    audio.currentTime = 0;
    renderState();
  });
  toggle.onclick = async () => {
    if (loading) return;
    if (!audio.getAttribute('src') && options.load) {
      loading = true;
      toggle.disabled = true;
      wrap.toggleAttribute('data-loading', true);
      try {
        const url = await options.load();
        if (!wrap.isConnected) { URL.revokeObjectURL(url); return; }
        loadedUrl = url;
        audio.src = url;
        await audio.play().catch(() => undefined);
      } catch (error) {
        if (wrap.isConnected) options.onError?.(error);
      } finally {
        loading = false;
        toggle.disabled = false;
        wrap.removeAttribute('data-loading');
      }
      return;
    }
    if (audio.paused) await audio.play().catch(() => undefined);
    else audio.pause();
  };
  return {
    node: wrap,
    audio,
    cleanup() {
      stopLoop();
      if (!audio.paused) audio.pause();
      if (loadedUrl) { URL.revokeObjectURL(loadedUrl); loadedUrl = undefined; }
    },
  };
}

function bindEditorKeys(host: HTMLElement, editor: RichEditor, save: () => void, escape: () => void) {
  const composing = (event: KeyboardEvent) => event.isComposing || editor.view.compositionStarted;
  host.addEventListener('keydown', event => {
    if (composing(event) || event.key !== 'Enter' || !(event.metaKey || event.ctrlKey)) return;
    event.preventDefault(); event.stopPropagation(); save();
  }, true);
  host.addEventListener('keydown', event => {
    if (event.defaultPrevented || composing(event) || event.key !== 'Escape') return;
    event.preventDefault(); escape();
  });
}

export function createMemoView(getBook: () => Notebook, actions: {
  error(message: string): void;
  status?(message: string): void;
  changed(): void;
  openNote(id: string, folderId: string): Promise<void>;
  filterChanged(filter: MemoFilter): void;
  openLink(id: string, source: string): void | Promise<void>;
  pickAttachments?(images: boolean, remaining: number): Promise<File[]>;
  readAudio?(id: string, source: string): Promise<ArrayBuffer>;
  copyText(text: string): Promise<void>;
  mountImage?(id: string, image: HTMLImageElement, card: HTMLElement, url: string, source: string): (() => void) | undefined;
}) {
  const pane = document.querySelector<HTMLElement>('#memo-pane')!;
  const searchButton = document.querySelector<HTMLButtonElement>('#memo-search-button')!;
  searchButton.append(icon(Search));
  const header = element('div', 'memo-content memo-header');
  const scroller = element('div', 'memo-scroll');
  const body = element('div', 'memo-content');
  const searchBar = element('div', 'memo-search-bar');
  searchBar.hidden = true;
  const search = element('input', 'memo-search');
  search.type = 'search';
  const searchCount = element('span', 'memo-search-count');
  searchCount.setAttribute('role', 'status');
  searchBar.append(icon(Search), search, searchCount);
  const chip = button('memo-tag memo-filter', '');
  const form = element('form', 'memo-composer');
  const input = element('div', 'memo-input');
  const tagMenu = element('div', 'memo-tag-suggest');
  tagMenu.role = 'listbox'; tagMenu.hidden = true;
  const extras = element('div', 'memo-composer-tools');
  const add = button('button quiet-button memo-add', uiText('attach'), Plus);
  add.setAttribute('aria-haspopup', 'menu');
  const previews = element('div', 'memo-previews');
  const save = button('button primary-button memo-save', '');
  const saveLabel = element('span');
  const saveShortcut = element('span', 'memo-save-shortcut');
  const macShortcut = /Mac|iPad|iPhone/i.test(navigator.platform);
  saveShortcut.ariaHidden = 'true';
  saveShortcut.append(element('kbd', '', macShortcut ? '⌘' : 'Ctrl'), element('kbd', '', macShortcut ? '↵' : 'Enter'));
  save.setAttribute('aria-keyshortcuts', macShortcut ? 'Meta+Enter' : 'Control+Enter');
  save.append(saveLabel, saveShortcut);
  save.type = 'submit';
  const recordRow = element('div', 'memo-recording');
  const recordTime = element('span', 'memo-record-time');
  const stop = button('button secondary-button memo-record-stop', '');
  const stopIcon = element('span', 'memo-record-stop-icon');
  const stopLabel = element('span');
  stopIcon.ariaHidden = 'true';
  stop.append(stopIcon, stopLabel);
  const cancelRecording = button('button quiet-button', '');
  recordRow.hidden = true; recordRow.role = 'status';
  // 录音时左边一个缓慢呼吸的圆点，中间是随声音起伏的波形，新的一格从右边连续滑入、旧的往左淡出。
  const recordDot = element('span', 'memo-record-dot');
  const wave = element('span', 'memo-wave');
  const waveTrack = element('span', 'memo-wave-track');
  wave.ariaHidden = recordDot.ariaHidden = 'true';
  waveTrack.append(...Array.from({ length: 44 }, () => element('i')));
  wave.append(waveTrack);
  recordRow.append(recordDot, recordTime, wave, cancelRecording, stop);
  const picker = element('input');
  picker.type = 'file'; picker.multiple = true; picker.hidden = true;
  const quickFilters = element('div', 'memo-quick-filters');
  quickFilters.role = 'group'; quickFilters.hidden = true;
  const warning = element('p', 'memo-warning'); warning.setAttribute('role', 'status');
  const timeline = element('div', 'memo-timeline');
  const sentinel = element('div', 'memo-sentinel');
  sentinel.ariaHidden = 'true';
  const more = button('button quiet-button memo-more-results', uiText('memoExpand'));
  more.hidden = true;
  extras.append(add, save);
  form.append(input, tagMenu, previews, recordRow, extras, picker);
  header.append(searchBar, chip, form);
  body.append(quickFilters, warning, timeline, sentinel, more);
  scroller.append(body);
  // 手机上输入框平时收在底部输入栏里，点开后贴在键盘上方，单手就能记。平板和桌面仍放在顶部。
  const dock = button('memo-dock', '');
  const dockText = element('span', 'memo-dock-text');
  dock.append(dockText, icon(Plus));
  const sheetBackdrop = element('div', 'memo-sheet-backdrop');
  sheetBackdrop.ariaHidden = 'true';
  pane.append(header, scroller, sheetBackdrop, dock);
  const phone = () => Boolean(pane.closest('[data-mobile-page]:not([data-mobile-wide])'));
  let composing = false;
  // iOS 的键盘盖在网页上，要自己把面板抬到键盘上方；安卓会缩小 WebView，这里算出来是 0。
  function placeAboveKeyboard() {
    const viewport = window.visualViewport;
    const keyboard = viewport ? Math.max(0, document.documentElement.clientHeight - viewport.height - viewport.offsetTop) : 0;
    pane.style.setProperty('--memo-keyboard', keyboard + 'px');
    pane.toggleAttribute('data-keyboard', keyboard > 80);
  }
  function openSheet() {
    if (!visible) return;
    if (!phone()) { composer.focus(); return; }
    if (!composing) {
      composing = true; pane.toggleAttribute('data-composing', true);
      placeAboveKeyboard();
      window.visualViewport?.addEventListener('resize', placeAboveKeyboard);
      window.visualViewport?.addEventListener('scroll', placeAboveKeyboard);
    }
    composer.focus();
  }
  function closeSheet() {
    if (!composing) return false;
    composing = false; pane.removeAttribute('data-composing'); pane.removeAttribute('data-keyboard');
    window.visualViewport?.removeEventListener('resize', placeAboveKeyboard);
    window.visualViewport?.removeEventListener('scroll', placeAboveKeyboard);
    closeTagSuggest(); persistDrafts();
    if (form.contains(document.activeElement)) (document.activeElement as HTMLElement).blur();
    updateDock();
    return true;
  }
  function updateDock() {
    const draft = sourceOf(composer).trim().split('\n', 1)[0];
    dockText.textContent = draft ? uiText('memoDraft').replace('{text}', () => draft) : uiText('memoDock');
    dock.toggleAttribute('data-draft', Boolean(draft));
    dock.ariaLabel = draft ? dockText.textContent : composerPrompt(filter);
  }
  dock.onclick = openSheet;
  sheetBackdrop.onclick = () => { closeSheet(); };

  let filter: MemoFilter = allMemos, query = '', limit = 60;
  let visible = false, saving = false, sequence = 0, newMemo = '';
  let shownBook: Notebook | undefined, draftKey = '', attachments: DraftAttachment[] = [];
  let recording: AudioRecording | undefined;
  let pickerSequence = -1;
  let picking = false;
  type SavedText = { id: string; source: string; refs: string[] };
  let savedText: SavedText | undefined;
  const pendingAttachments = new Map<string, DraftAttachment[]>();
  const partialSaves = new Map<string, SavedText>();
  let draftTimer: ReturnType<typeof setTimeout> | undefined, searchTimer: ReturnType<typeof setTimeout> | undefined;
  let dayTimer: ReturnType<typeof setTimeout> | undefined;
  let edit: { id: string; backupKey: string; initial: string; node: HTMLFormElement; editor: RichEditor } | undefined;
  const editBackups = new Map<string, string>();
  const expanded = new Set<string>(), busy = new Set<string>();
  const composer = createRichEditor(input, () => { updateComposer(); queueDrafts(); updateTagSuggest(); }, {
    fieldLabel: () => composerPrompt(filter), sortCheckedItems: () => false, onError: actions.error,
  });
  const sourceOf = (editor: RichEditor) => editor.view.state.doc.toString();
  type Card = { note: Note; node: HTMLElement; cleanups: (() => void)[]; highlight?: string };
  const cards = new Map<string, Card>();
  let tagSuggest: { from: number; to: number; items: string[]; index: number } | undefined;
  let dragDepth = 0;
  const tagTrigger = /(?:^|[\s\u3000-\u303f\uff01-\uff0f\uff1a-\uff20\u2018-\u201f])#([\p{L}\p{N}_/-]{0,32})$/u;
  function closeTagSuggest() {
    if (!tagSuggest) return;
    tagSuggest = undefined; tagMenu.hidden = true; tagMenu.replaceChildren();
  }
  function renderTagSuggest() {
    if (!tagSuggest || !tagSuggest.items.length) { closeTagSuggest(); return; }
    const current = tagSuggest;
    tagMenu.replaceChildren(...current.items.map((tag, index) => {
      const option = element('button', 'memo-tag-option', '#' + tag);
      option.type = 'button'; option.role = 'option';
      option.setAttribute('aria-selected', String(index === current.index));
      option.onpointerdown = event => { event.preventDefault(); applyTagSuggest(tag); };
      return option;
    }));
    tagMenu.hidden = false;
  }
  function applyTagSuggest(tag: string) {
    if (!tagSuggest) return;
    const { from, to } = tagSuggest;
    const insert = '#' + tag + ' ';
    closeTagSuggest();
    composer.view.dispatch({ changes: { from, to, insert }, selection: { anchor: from + insert.length } });
    composer.focus();
  }
  function updateTagSuggest() {
    if (!visible || saving || !form.contains(document.activeElement)) { closeTagSuggest(); return; }
    const { doc, selection } = composer.view.state;
    if (!selection.main.empty) { closeTagSuggest(); return; }
    const head = selection.main.head, line = doc.lineAt(head);
    const match = line.text.slice(0, head - line.from).match(tagTrigger);
    if (!match) { closeTagSuggest(); return; }
    const partial = match[1], key = memoTagKey(partial);
    const matches = memoTagCounts(getBook().data)
      .map(([tag]) => tag)
      .filter(tag => !key || memoTagKey(tag).includes(key))
      .slice(0, 8);
    if (!matches.length || (matches.length === 1 && memoTagKey(matches[0]) === key)) { closeTagSuggest(); return; }
    const from = head - partial.length - 1;
    const index = tagSuggest && tagSuggest.from === from ? Math.min(tagSuggest.index, matches.length - 1) : 0;
    tagSuggest = { from, to: head, items: matches, index };
    renderTagSuggest();
  }
  let renderLocale = '', renderRoot: string | undefined, renderAssetRevision = -1;
  const observer = typeof IntersectionObserver === 'function' ? new IntersectionObserver(entries => {
    if (entries.some(entry => entry.isIntersecting)) showMore();
  }, { root: scroller, rootMargin: '0px 0px 600px' }) : undefined;
  function showMore() { if (visible && !sentinel.hidden) { limit += 60; renderTimeline(); } }
  more.onclick = showMore;

  function writeDraft(key: string, value?: string) {
    try {
      if (value === undefined) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    } catch (error) { actions.error(message(error)); }
  }
  function storeEditBackup(key: string, source?: string) {
    if (source === undefined) editBackups.delete(key);
    else editBackups.set(key, source);
    writeDraft(key, source);
  }
  function persistDrafts() {
    clearTimeout(draftTimer); draftTimer = undefined;
    if (draftKey) writeDraft(draftKey, sourceOf(composer) || undefined);
    if (edit) {
      const source = sourceOf(edit.editor);
      storeEditBackup(edit.backupKey, source === edit.initial ? undefined : source);
    }
  }
  function queueDrafts() {
    clearTimeout(draftTimer); draftTimer = setTimeout(persistDrafts, 500);
  }
  function clearSubmittedDraft(key: string, submitted: string) {
    if (key === draftKey) {
      if (sourceOf(composer) === submitted) {
        composer.activate('', crypto.randomUUID());
        updateComposer(); writeDraft(key);
      }
    } else {
      try { if (localStorage.getItem(key) === submitted) localStorage.removeItem(key); }
      catch (error) { actions.error(message(error)); }
    }
  }
  function loadDraft() {
    const book = getBook(), key = 'qingyu.memo.draft:' + (book.root ?? 'local');
    if (key === draftKey && book === shownBook) return;
    if (draftKey) persistDrafts();
    draftKey = key; shownBook = book; sequence++;
    finishRecording();
    let source = '';
    try { source = localStorage.getItem(key) ?? ''; } catch { /* No stored draft is available. */ }
    composer.activate(source, crypto.randomUUID());
    savedText = partialSaves.get(key); attachments = pendingAttachments.get(key) ?? [];
    closeEdit(); expanded.clear();
    clearCards();
  }
  function renderPreviews() {
    pausePreviews();
    previews.replaceChildren(...attachments.map(attachment => {
      const { file, image, preview } = attachment;
      const tile = element('div', image ? 'memo-preview' : 'memo-preview memo-preview-file');
      const remove = button(image && preview ? 'memo-remove-image' : 'button quiet-button memo-remove-attachment', translatedLabel('取消') + ' ' + file.name, X);
      remove.disabled = saving;
      remove.onclick = () => {
        attachments.splice(attachments.indexOf(attachment), 1);
        if (preview) URL.revokeObjectURL(preview);
        if (!attachments.length && !savedText?.refs.length) {
          partialSaves.delete(draftKey); savedText = undefined;
        }
        renderPreviews(); updateComposer();
      };
      if (image && preview) {
        const img = element('img'); img.src = preview; img.alt = file.name;
        tile.append(img);
      } else if (preview) {
        const player = createAudioPlayer({ source: preview, label: file.name, onPlay: pauseOtherAudio });
        tile.append(player.node);
      } else {
        const name = element('span', '', file.name); name.title = file.name;
        tile.append(icon(Paperclip), name);
      }
      tile.append(remove); return tile;
    }));
  }
  function pauseOtherAudio(active?: HTMLAudioElement) {
    for (const audio of pane.querySelectorAll('audio')) if (audio !== active && !audio.paused) audio.pause();
  }
  function pausePreviews() {
    for (const audio of previews.querySelectorAll('audio')) if (!audio.paused) audio.pause();
  }
  function updateComposer() {
    const disabled = getBook().loadError;
    const source = sourceOf(composer);
    form.ariaBusy = String(saving);
    composer.setReadOnly(disabled || saving);
    add.disabled = disabled || Boolean(recording);
    save.disabled = disabled || Boolean(recording) || !(source.trim() || attachments.length || savedText?.refs.length);
    add.ariaDisabled = save.ariaDisabled = saving ? 'true' : null;
    saveLabel.textContent = uiText(savedText ? 'memoRetryAttachments' : 'memoSave');
    warning.hidden = !disabled; warning.textContent = uiText('memoLoadError');
    updateDock();
  }
  function clearCards() {
    for (const card of cards.values()) for (const cleanup of card.cleanups) cleanup();
    cards.clear();
  }
  function canAttach() { return Boolean(getBook().root || getBook().server); }
  function addAttachments(files: File[], inlineImages = true) {
    if (!visible || saving || recording || getBook().loadError || !files.length) return;
    if (!canAttach()) { actions.error(uiText('memoAttachmentsUnavailable')); return; }
    if (savedText) { actions.error(uiText('memoResolveAttachments')); return; }
    if (files.some(file => file.size > MAX_ASSET_BYTES) ||
      [...attachments.map(({ file }) => file), ...files].reduce((size, file) => size + file.size, 0) > 4 * MAX_ASSET_BYTES) {
      actions.error(uiText('memoAttachmentLimit')); return;
    }
    attachments.push(...files.map(file => {
      const image = inlineImages && file.type.startsWith('image/');
      return { file, image, preview: image || audioType(file.name) ? URL.createObjectURL(file) : undefined };
    }));
    pendingAttachments.set(draftKey, attachments);
    renderPreviews(); updateComposer();
  }
  function finishRecording() {
    const current = recording;
    if (!current) return;
    recording = undefined; clearInterval(current.timer); current.release?.();
    delete recordRow.dataset.state;
    recordRow.style.removeProperty('--memo-level');
    form.removeAttribute('data-recording');
    recordRow.hidden = true; updateComposer();
  }
  cancelRecording.onclick = () => { finishRecording(); add.focus(); };
  stop.onclick = () => {
    stop.disabled = true;
    recording?.stop?.();
  };
  async function recordAudio() {
    if (!visible || !canAttach() || saving || savedText || recording || getBook().loadError) return;
    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder !== 'function') {
      actions.error(uiText('memoRecordingUnavailable')); return;
    }
    const format = audioFormats.find(([type]) => MediaRecorder.isTypeSupported(type));
    if (!format) { actions.error(uiText('memoRecordingUnavailable')); return; }
    const [mimeType, extension] = format;
    const book = getBook(), root = book.root;
    const current: AudioRecording = {};
    recording = current;
    recordRow.dataset.state = 'preparing';
    form.toggleAttribute('data-recording', true);
    recordRow.hidden = false; recordRow.ariaLabel = uiText('memoRecording'); recordTime.textContent = '00:00';
    waveTrack.style.transform = '';
    for (const bar of waveTrack.children) { (bar as HTMLElement).style.transform = ''; (bar as HTMLElement).style.opacity = ''; }
    stop.disabled = true; updateComposer(); cancelRecording.focus();
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const stopWave = startWaveMeter(stream, wave, recordRow);
      const release = () => { stopWave(); stream.getTracks().forEach(track => track.stop()); };
      current.release = release;
      if (recording !== current) { release(); return; }
      if (book !== getBook() || root !== book.root) { finishRecording(); return; }
      const recorder = new MediaRecorder(stream, { mimeType });
      const chunks: Blob[] = [];
      let size = 0;
      const remaining = Math.min(MAX_ASSET_BYTES, 4 * MAX_ASSET_BYTES - attachments.reduce((total, item) => total + item.file.size, 0));
      current.stop = () => { if (recorder.state !== 'inactive') recorder.stop(); release(); };
      recorder.ondataavailable = ({ data }) => {
        if (recording !== current || !data.size) return;
        size += data.size;
        if (size > remaining) { finishRecording(); actions.error(uiText('memoAttachmentLimit')); return; }
        chunks.push(data);
      };
      recorder.onerror = () => {
        if (recording === current) { finishRecording(); actions.error(uiText('memoRecordingFailed')); }
      };
      recorder.onstop = () => {
        if (recording !== current) return;
        finishRecording();
        if (book !== getBook() || root !== book.root) return;
        if (!size) { actions.error(uiText('memoRecordingFailed')); return; }
        const stamp = new Date().toISOString().slice(0, -1).replace(/[:.]/g, '-');
        addAttachments([new File(chunks, `voice-${stamp}.${extension}`, { type: recorder.mimeType || mimeType })]);
        add.focus();
      };
      recorder.start(1000);
      recordRow.dataset.state = 'active';
      const started = performance.now();
      const tick = () => {
        const elapsed = Math.floor((performance.now() - started) / 1000);
        const time = [Math.floor(elapsed / 60), elapsed % 60].map(part => String(part).padStart(2, '0')).join(':');
        recordTime.textContent = time;
      };
      tick(); current.timer = setInterval(tick, 1000); stop.disabled = false; stop.focus();
    } catch (error) {
      if (recording !== current) return;
      finishRecording();
      actions.error(uiText(error instanceof DOMException && error.name === 'NotAllowedError' ? 'memoMicrophoneDenied' : 'memoRecordingFailed'));
    }
  }
  function insertTodo() {
    const { doc, selection } = composer.view.state;
    if (!taskPrefix.test(doc.lineAt(selection.main.head).text)) composer.format('task');
    else composer.focus();
  }
  add.onclick = () => {
    if (saving || recording || picking) return;
    const enabled = !savedText && canAttach();
    const pick = actions.pickAttachments;
    const choose = async (images: boolean) => {
      pickerSequence = sequence;
      if (!pick) { picker.accept = images ? 'image/*' : ''; picker.click(); return; }
      const operation = sequence;
      const remaining = 4 * MAX_ASSET_BYTES - attachments.reduce((total, item) => total + item.file.size, 0);
      picking = true;
      try {
        const files = await pick(images, remaining);
        if (sequence === operation && pickerSequence === operation) addAttachments(files, images);
      } finally { picking = false; }
    };
    void showMenu([
      { text: uiText('attach'), icon: icon(Paperclip), enabled, needsUserActivation: !pick, run: () => choose(false) },
      { text: uiText('memoPicture'), icon: icon(Image), enabled, needsUserActivation: !pick, run: () => choose(true) },
      { text: uiText('memoRecordAudio'), icon: icon(Mic), enabled, run: recordAudio },
      { text: uiText('memoTodo'), icon: icon(ListTodo), run: insertTodo },
    ], { anchor: add, label: uiText('attach'), onError: actions.error });
  };
  picker.onchange = () => {
    const images = picker.accept === 'image/*';
    if (pickerSequence === sequence) addAttachments(Array.from(picker.files ?? []).filter(file => !images || file.type.startsWith('image/')), images);
    picker.value = '';
  };
  form.addEventListener('dragenter', event => {
    if (!event.dataTransfer?.types.includes('Files')) return;
    dragDepth++;
    form.toggleAttribute('data-dragging', true);
  });
  form.addEventListener('dragleave', () => {
    if (dragDepth > 0 && --dragDepth === 0) form.removeAttribute('data-dragging');
  });
  form.addEventListener('dragover', event => { if (event.dataTransfer?.types.includes('Files')) event.preventDefault(); });
  form.addEventListener('drop', event => {
    dragDepth = 0; form.removeAttribute('data-dragging');
    if (event.dataTransfer?.files.length) {
      event.preventDefault(); event.stopPropagation(); addAttachments(Array.from(event.dataTransfer.files));
    }
  }, true);
  input.addEventListener('paste', event => {
    if (event.clipboardData?.files.length) {
      event.preventDefault(); event.stopPropagation(); addAttachments(Array.from(event.clipboardData.files));
    }
  }, true);
  form.addEventListener('focusout', () => {
    queueMicrotask(() => { if (!form.contains(document.activeElement)) closeTagSuggest(); });
  });
  input.addEventListener('focusin', () => {
    if (phone()) scroller.scrollTop = 0;
  });
  input.addEventListener('keydown', event => {
    if (!tagSuggest || event.isComposing || composer.view.compositionStarted) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); event.stopPropagation();
      const delta = event.key === 'ArrowDown' ? 1 : -1;
      tagSuggest.index = (tagSuggest.index + delta + tagSuggest.items.length) % tagSuggest.items.length;
      renderTagSuggest();
    } else if ((event.key === 'Enter' && !event.metaKey && !event.ctrlKey) || event.key === 'Tab') {
      event.preventDefault(); event.stopPropagation();
      applyTagSuggest(tagSuggest.items[tagSuggest.index]!);
    } else if (event.key === 'Escape') {
      event.preventDefault(); event.stopPropagation();
      closeTagSuggest();
    }
  }, true);
  bindEditorKeys(input, composer, () => { closeTagSuggest(); void saveMemo(); }, () => {
    if (input.contains(document.activeElement)) (document.activeElement as HTMLElement).blur();
  });
  form.onsubmit = event => { event.preventDefault(); void saveMemo(); };
  pane.addEventListener('contextmenu', event => {
    if (!event.defaultPrevented && !(event.target instanceof Element && event.target.closest('a, input, textarea'))) event.preventDefault();
  });
  window.addEventListener('pagehide', () => { persistDrafts(); finishRecording(); });
  document.addEventListener('visibilitychange', () => { if (document.hidden) persistDrafts(); });

  async function saveMemo() {
    if (saving || !visible || save.disabled || composer.view.compositionStarted) return;
    let savedAll = false;
    const book = getBook(), root = book.root, key = draftKey, operation = sequence;
    const loadChanged = () => getBook() !== book || book.root !== root || operation !== sequence;
    const queued = [...attachments], pending = attachments;
    const submitted = sourceOf(composer);
    let source = submitted;
    const reportedFilter = filter;
    let partial = savedText;
    if (reportedFilter.kind === 'tag' && !partial && !memoTags(source).some(tag => memoTagKey(tag) === memoTagKey(reportedFilter.tag))) {
      const tag = '#' + reportedFilter.tag;
      const withTag = [source, tag].filter(Boolean).join('\n\n');
      source = memoTags(withTag).some(value => memoTagKey(value) === memoTagKey(reportedFilter.tag)) ? withTag : tag + '\n\n' + source;
    }
    saving = true; updateComposer(); renderPreviews();
    try {
      if (!partial) {
        const id = await book.createMemo(source);
        partial = { id, source, refs: [] };
        clearSubmittedDraft(key, submitted);
        if (!loadChanged()) newMemo = id;
        if (queued.length) partialSaves.set(key, partial);
      }
      const { id } = partial;
      if (loadChanged()) return;
      let failures = 0;
      for (const draft of queued) {
        if (loadChanged()) return;
        try {
          const bytes = Array.from(new Uint8Array(await draft.file.arrayBuffer()));
          if (loadChanged()) return;
          const attachment = await book.importAttachment(id, {
            name: draft.file.name, bytes,
          });
          partial.refs.push(attachmentMarkdown({ ...attachment, image: draft.image && attachment.image }));
          pending.splice(pending.indexOf(draft), 1);
          if (draft.preview) URL.revokeObjectURL(draft.preview);
        } catch { failures++; }
      }
      if (loadChanged()) return;
      if (partial.refs.length) {
        const appended = [partial.source, partial.refs.join('\n\n')].filter(Boolean).join('\n\n');
        await book.save(appended, id, partial.source);
        partial.source = appended; partial.refs = [];
      }
      if (!failures) { partialSaves.delete(key); pendingAttachments.delete(key); savedAll = true; }
      if (failures) actions.error(uiText('memoAttachmentFailed').replace('{count}', String(failures)));
    } catch (error) { actions.error(message(error)); }
    finally {
      saving = false;
      savedText = partialSaves.get(draftKey);
      if (visible) {
        renderPreviews(); updateComposer();
        // 手机上记完一条就收起面板，让新卡片出现在眼前；还有附件没传上时留着面板重试。
        if (!loadChanged()) { scroller.scrollTop = 0; if (composing && savedAll) closeSheet(); else composer.focus(); }
      }
      actions.changed();
    }
  }

  function handleSearch() {
    query = search.value; limit = 60; renderTimeline();
  }
  function closeSearch() {
    if (searchBar.hidden) return false;
    clearTimeout(searchTimer); search.value = ''; searchBar.hidden = true;
    pane.removeAttribute('data-searching'); searchCount.textContent = '';
    searchButton.ariaExpanded = 'false'; handleSearch();
    return true;
  }
  function focusSearch() {
    searchBar.hidden = false; pane.toggleAttribute('data-searching', true);
    searchButton.ariaExpanded = 'true'; search.focus();
  }
  searchButton.onclick = () => { if (!closeSearch()) focusSearch(); };
  search.oninput = () => { clearTimeout(searchTimer); searchTimer = setTimeout(handleSearch, 120); };
  search.onkeydown = event => {
    if (event.key === 'Escape' && !event.isComposing) {
      event.preventDefault(); closeSearch(); searchButton.focus();
    }
  };
  function setFilter(next: MemoFilter) {
    if (filterName(filter) !== filterName(next)) { limit = 60; scroller.scrollTop = 0; }
    filter = next;
    actions.filterChanged(filter);
    localize(); renderTimeline();
  }
  chip.onclick = () => { setFilter(allMemos); composer.focus(); };

  function renderQuickFilters() {
    const data = getBook().data;
    const todos = memoNotes(data, { kind: 'todo' }, '').length;
    const tags = memoTagCounts(data);
    if (!todos && !tags.length) { quickFilters.hidden = true; quickFilters.replaceChildren(); return; }
    const pill = (next: MemoFilter, label: string) => {
      const item = button('memo-quick-pill', label);
      const active = filter.kind === next.kind && (next.kind !== 'tag' || (filter.kind === 'tag' && memoTagKey(filter.tag) === memoTagKey(next.tag)));
      item.setAttribute('aria-pressed', String(active));
      item.onclick = () => setFilter(next);
      return item;
    };
    const items: HTMLButtonElement[] = [pill(allMemos, uiText('memoAll'))];
    if (todos || filter.kind === 'todo') items.push(pill({ kind: 'todo' }, todos ? `${uiText('memoTodo')} ${todos}` : uiText('memoTodo')));
    const shown = tags.slice(0, 12);
    const activeTag = filter.kind === 'tag' ? filter.tag : undefined;
    if (activeTag && !shown.some(([tag]) => memoTagKey(tag) === memoTagKey(activeTag))) shown.push([activeTag, 0]);
    for (const [tag] of shown) items.push(pill({ kind: 'tag', tag }, '#' + tag));
    quickFilters.replaceChildren(...items);
    quickFilters.hidden = false;
  }
  function applyHighlight(card: Card, term: string) {
    const normalized = term.trim().toLocaleLowerCase();
    if (card.highlight === normalized) return;
    card.highlight = normalized;
    const bodyEl = card.node.querySelector<HTMLElement>('.memo-card-body');
    if (!bodyEl) return;
    for (const mark of bodyEl.querySelectorAll('mark.memo-highlight')) {
      mark.replaceWith(document.createTextNode(mark.textContent ?? ''));
    }
    bodyEl.normalize();
    for (const tag of bodyEl.querySelectorAll<HTMLButtonElement>('.memo-tag')) {
      tag.toggleAttribute('data-matched', Boolean(normalized && (tag.textContent ?? '').toLocaleLowerCase().includes(normalized)));
    }
    if (!normalized) return;
    const walker = document.createTreeWalker(bodyEl, NodeFilter.SHOW_TEXT);
    const nodes: Text[] = [];
    for (let current = walker.nextNode(); current; current = walker.nextNode()) {
      const parent = current.parentElement;
      if (!parent || parent.closest('button, code, pre, script, style, mark')) continue;
      if (current.nodeValue && current.nodeValue.toLocaleLowerCase().includes(normalized)) nodes.push(current as Text);
    }
    for (const textNode of nodes) {
      const raw = textNode.nodeValue!, lower = raw.toLocaleLowerCase();
      const frag = document.createDocumentFragment();
      let offset = 0;
      for (let index = lower.indexOf(normalized, offset); index !== -1; index = lower.indexOf(normalized, offset)) {
        if (index > offset) frag.append(raw.slice(offset, index));
        const mark = element('mark', 'memo-highlight', raw.slice(index, index + normalized.length));
        frag.append(mark);
        offset = index + normalized.length;
      }
      if (offset < raw.length) frag.append(raw.slice(offset));
      textNode.replaceWith(frag);
    }
  }

  function localize() {
    const name = filterName(filter);
    stopLabel.textContent = uiText('memoStopRecording');
    cancelRecording.textContent = translatedLabel('取消');
    pane.ariaLabel = uiText('memos');
    form.dataset.dropHint = uiText('memoDropHint');
    searchButton.title = searchButton.ariaLabel = search.placeholder = search.ariaLabel = uiText('memoSearch');
    if (composer.input.ariaLabel !== composerPrompt(filter)) composer.refreshLanguage();
    chip.textContent = name + ' ×'; chip.ariaLabel = uiText('memoClearFilter'); chip.hidden = !name;
    add.title = add.ariaLabel = uiText('attach');
    more.textContent = uiText('memoExpand');
  }
  function layoutCard(card: HTMLElement) {
    const copy = card.querySelector<HTMLElement>('.memo-card-body');
    const expand = card.querySelector<HTMLButtonElement>('.memo-expand');
    const id = card.dataset.memoId;
    if (!copy || !expand || !id) return;
    const mediaHeight = [...copy.querySelectorAll<HTMLElement>(':scope > .memo-image-grid')].reduce((height, media) => height + media.offsetHeight, 0);
    const tall = copy.scrollHeight - mediaHeight > 12 * 24 + 1;
    if (!tall) {
      expanded.delete(id);
      card.removeAttribute('data-clipped');
      expand.hidden = true;
      return;
    }
    const isExpanded = expanded.has(id);
    card.toggleAttribute('data-clipped', !isExpanded);
    expand.hidden = false;
    expand.textContent = uiText(isExpanded ? 'memoCollapse' : 'memoExpand');
    expand.ariaExpanded = String(isExpanded);
  }
  function renderTimeline() {
    if (!visible) return;
    const book = getBook(), language = locale();
    if (renderLocale !== language || renderRoot !== book.root || renderAssetRevision !== book.assetRevision) {
      clearCards(); renderLocale = language; renderRoot = book.root; renderAssetRevision = book.assetRevision;
    }
    const notes = memoNotes(book.data, filter, query);
    renderQuickFilters();
    searchCount.textContent = query.trim() ? uiText('memoSearchCount').replace('{count}', notes.length.toLocaleString(language)) : '';
    const selected = notes.slice(0, limit);
    if (edit && !selected.some(note => note.id === edit!.id)) {
      const current = book.data.notes.find(note => note.id === edit!.id && !note.trashed);
      if (current) selected.push(current);
    }
    const kept = new Set(selected.map(note => note.id));
    for (const [id, card] of cards) {
      if (kept.has(id)) continue;
      for (const cleanup of card.cleanups) cleanup();
      cards.delete(id);
    }
    const active = document.activeElement;
    const memoFocused = active instanceof HTMLElement ? active.closest<HTMLElement>('.memo-card')?.dataset.memoId : undefined;
    const control = active instanceof HTMLElement ? active.dataset.memoControl : undefined;
    const fragment = document.createDocumentFragment(), now = new Date();
    const time = new Intl.DateTimeFormat(language, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
    const full = new Intl.DateTimeFormat(language, { dateStyle: 'full', timeStyle: 'short' });
    for (const group of groupMemosByDay(selected, now, language)) {
      const section = element('section', 'memo-day');
      const heading = element('h2', 'memo-day-title', group.label);
      section.append(heading);
      for (const note of group.notes) {
        if (edit?.id === note.id) { section.append(edit.node); continue; }
        let card = cards.get(note.id);
        if (card?.note !== note) {
          for (const cleanup of card?.cleanups ?? []) cleanup();
          card = createCard(note, time, full);
          cards.set(note.id, card);
        }
        section.append(card.node);
      }
      fragment.append(section);
    }
    if (!selected.length) {
      const filtered = filter.kind !== 'all' || Boolean(query.trim());
      const empty = element('div', 'memo-empty');
      empty.append(element('p', '', uiText(filtered ? 'memoNoMatch' : 'memoEmpty')));
      if (filtered) {
        const clear = button('button quiet-button', uiText('memoClearFilter'));
        clear.onclick = () => { closeSearch(); setFilter(allMemos); composer.focus(); };
        empty.append(clear);
      } else {
        empty.append(element('small', '', uiText('memoEmptyHint')));
      }
      fragment.append(empty);
    }
    timeline.replaceChildren(fragment);
    if (memoFocused) {
      const focus = active instanceof HTMLElement && active.isConnected ? active
        : control ? cards.get(memoFocused)?.node.querySelector<HTMLElement>('[data-memo-control="' + control + '"]') : undefined;
      focus?.focus({ preventScroll: true });
    }
    for (const card of cards.values()) {
      if (!card.node.isConnected) continue;
      applyHighlight(card, query);
      layoutCard(card.node);
    }
    sentinel.hidden = notes.length <= limit;
    observer?.unobserve(sentinel);
    if (!sentinel.hidden) observer?.observe(sentinel);
    more.hidden = sentinel.hidden;
    newMemo = '';
    markStuck();
  }
  function markStuck() {
    const top = scroller.getBoundingClientRect().top;
    for (const title of timeline.querySelectorAll<HTMLElement>('.memo-day-title')) {
      title.toggleAttribute('data-stuck', scroller.scrollTop > 0 && Math.abs(title.getBoundingClientRect().top - top) < 1);
    }
  }
  let frame = 0;
  scroller.addEventListener('scroll', () => {
    if (!frame) frame = requestAnimationFrame(() => { frame = 0; markStuck(); });
  }, { passive: true });
  if (typeof ResizeObserver === 'function') new ResizeObserver(() => {
    for (const card of cards.values()) {
      if (!card.node.isConnected) continue;
      applyHighlight(card, query);
      layoutCard(card.node);
    }
    markStuck();
  }).observe(scroller);

  function createCard(note: Note, time: Intl.DateTimeFormat, full: Intl.DateTimeFormat): Card {
    const book = getBook(), node = element('article', 'memo-card'), cleanups: Card['cleanups'] = [];
    node.dataset.memoId = note.id;
    if (note.id === newMemo) node.classList.add('memo-card-new');
    const metadata = element('div', 'memo-meta');
    const created = element('time', '', note.pinned ? memoStamp(note, locale()) : time.format(note.createdAt));
    created.dateTime = new Date(note.createdAt).toISOString(); created.title = full.format(note.createdAt);
    metadata.append(created);
    if (note.updatedAt - note.createdAt > 60_000) metadata.append(' · ' + uiText('memoEdited'));
    if (note.conflictOf) metadata.append(element('span', 'memo-conflict', uiText('memoConflict')));
    const markers = element('span', 'memo-markers');
    if (note.readOnly) { const lock = icon(Lock); lock.setAttribute('aria-label', uiText('memoReadOnly')); markers.append(lock); }
    if (note.pinned) { const pin = icon(Pin); pin.setAttribute('aria-label', uiText('pinned')); markers.append(pin); }
    const menu = button('memo-menu', uiText('more'), MoreHorizontal);
    menu.dataset.memoControl = 'menu'; menu.setAttribute('aria-haspopup', 'menu');
    menu.onclick = () => openMenu(note, node, menu, { anchor: menu });
    markers.append(menu); metadata.append(markers);
    const contents = element('div', 'memo-card-body');
    contents.innerHTML = renderMemo(note);
    for (const link of contents.querySelectorAll<HTMLAnchorElement>('a[href]')) {
      const target = link.getAttribute('href')!;
      const local = isLocalAsset(target);
      if (!/^(https?:|mailto:|tel:)/iu.test(target) && !local) continue;
      link.target = '_blank'; link.rel = 'noopener noreferrer';
      link.onclick = event => { event.preventDefault(); void Promise.resolve(actions.openLink(note.id, target)).catch(report); };
      const type = local && audioType(target);
      if (!type) continue;
      const label = link.textContent || uiText('memoRecordAudio');
      const mountAudio = (player: ReturnType<typeof createAudioPlayer>) => {
        if (label && !/^voice-[\dt-]+\.[a-z0-9]+$/iu.test(label)) player.node.title = label;
        player.node.oncontextmenu = event => {
          event.preventDefault(); event.stopPropagation();
          openMenu(note, node, menu, { at: { x: event.clientX, y: event.clientY } }, (event as PointerEvent).pointerType === 'touch', [
            { text: uiText('memoOpenAudio'), icon: icon(ArrowUpRight), run: () => { void Promise.resolve(actions.openLink(note.id, target)).catch(report); } },
            { text: '' },
          ]);
        };
        cleanups.push(player.cleanup);
        const parent = link.parentElement;
        if (parent?.tagName === 'P' && parent.childNodes.length === 1) parent.replaceWith(player.node);
        else link.replaceWith(player.node);
      };
      const readAudio = actions.readAudio;
      if (!readAudio) {
        const url = attachmentUrl(book, note.id, target);
        if (url) mountAudio(createAudioPlayer({ source: url, label, onPlay: pauseOtherAudio }));
        continue;
      }
      mountAudio(createAudioPlayer({
        label,
        onPlay: pauseOtherAudio,
        onError: report,
        load: async () => {
          const bytes = await readAudio(note.id, target);
          return URL.createObjectURL(new Blob([bytes], { type }));
        },
      }));
    }
    for (const tag of contents.querySelectorAll<HTMLButtonElement>('.memo-tag')) {
      tag.onclick = () => setFilter({ kind: 'tag', tag: tag.dataset.tag! });
    }
    [...contents.querySelectorAll<HTMLInputElement>('.task-list-item-checkbox')].forEach((task, index) => {
      task.dataset.memoControl = 'task-' + index; task.setAttribute('aria-label', uiText('memoTodo') + ' ' + (index + 1));
      task.disabled = Boolean(note.readOnly);
      task.onchange = () => {
        const checked = !task.checked;
        task.disabled = true;
        void mutate(note, current => current.source === note.source && !current.readOnly
          ? book.save(toggleTask(note.source, index), note.id, note.source)
          : Promise.reject(new Error(uiText('memoChanged'))))
          .then(success => { if (!success) task.checked = checked; })
          .finally(() => { task.disabled = Boolean(note.readOnly); });
      };
    });
    const images = [...contents.querySelectorAll('img')];
    for (const image of images) {
      const source = image.getAttribute('src') ?? '';
      const local = isLocalAsset(source), url = local ? attachmentUrl(book, note.id, source) : source;
      image.loading = 'lazy'; image.decoding = 'async'; image.referrerPolicy = 'no-referrer';
      if (local) {
        image.removeAttribute('src');
        if (url) {
          const cleanup = actions.mountImage?.(note.id, image, node, url, source);
          if (cleanup) cleanups.push(cleanup); else image.src = url;
        }
      }
      if (url && (local || /^https?:/iu.test(source))) {
        const open = element('a', 'memo-image-link');
        open.href = url; open.target = '_blank'; open.rel = 'noopener noreferrer';
        open.ariaLabel = image.alt || uiText('memoPicture');
        if (local) open.onclick = event => { event.preventDefault(); void Promise.resolve(actions.openLink(note.id, source)).catch(report); };
        image.replaceWith(open); open.append(image);
      }
    }
    // Group consecutive image-only paragraphs without reordering the note.
    let grid: HTMLElement | undefined;
    for (const block of [...contents.children]) {
      const media = [...block.children];
      if (block.tagName !== 'P' || !media.length || !media.every(item => item.matches('img, .memo-image-link')) || block.textContent?.trim()) {
        grid = undefined; continue;
      }
      if (!grid) { grid = element('div', 'memo-image-grid'); block.before(grid); }
      grid.append(...media); block.remove();
    }
    const expand = button('button quiet-button memo-expand', uiText('memoExpand'));
    expand.dataset.memoControl = 'expand'; expand.hidden = true; expand.ariaExpanded = 'false';
    expand.onclick = () => {
      if (expanded.has(note.id)) {
        expanded.delete(note.id);
        layoutCard(node);
      } else {
        expanded.add(note.id);
        layoutCard(node);
        contents.tabIndex = -1; contents.focus({ preventScroll: true });
      }
    };
    contents.ondblclick = event => {
      if (event.target instanceof Element && !event.target.closest('button, a, input, img, audio, .memo-audio') && !matchMedia('(pointer: coarse)').matches) void startEdit(note);
    };
    node.append(metadata, contents, expand);
    node.oncontextmenu = event => {
      if (event.target instanceof Element && event.target.closest('a, audio, .memo-audio')) return;
      event.preventDefault();
      openMenu(note, node, menu, { at: { x: event.clientX, y: event.clientY } }, (event as PointerEvent).pointerType === 'touch');
    };
    return { note, node, cleanups };
  }
  const report = (error: unknown) => actions.error(message(error));
  async function mutate(note: Note, run: (current: Note) => Promise<void>) {
    const book = getBook(), memoId = findMemoFolder(book.data)?.id;
    const current = book.data.notes.find(candidate => candidate.id === note.id && candidate.folderId === memoId && !candidate.trashed);
    if (busy.has(note.id) || !current || book.loadError) return false;
    busy.add(note.id);
    try { await run(current); return true; }
    catch (error) { report(error); return false; }
    finally { busy.delete(note.id); actions.changed(); }
  }
  function orderedTargetFolders(book: Notebook) {
    const folders = book.data.folders.filter(folder => !isInsideMemoFolder(folder));
    return [
      ...folders.filter(folder => folder.id === 'default' || folder.path === '').map(folder => ({
        folder: { ...folder, name: uiText('libraryRoot') }, depth: 0, parentId: null as string | null, hasChildren: false,
      })),
      ...folderRows(folders, new Set(folders.map(folder => folder.id))),
    ];
  }
  function convertToNote(note: Note, targetFolderId: string, book: Notebook, root: string | undefined) {
    const memoId = findMemoFolder(book.data)?.id;
    void mutate(note, async current => {
      if (getBook() !== book || book.root !== root || current.folderId !== memoId) throw new Error(uiText('memoChanged'));
      await book.moveNote(note.id, targetFolderId);
      if (getBook() !== book || book.root !== root) return;
      const moved = book.data.notes.find(candidate => candidate.id === note.id);
      if (!moved) return;
      try { await book.save(moved.source, moved.id, moved.source, true); }
      finally { if (getBook() === book && book.root === root) await actions.openNote(note.id, targetFolderId); }
    });
  }
  function folderSubmenu(note: Note, book: Notebook, root: string | undefined, stillHere: () => boolean): MenuItem[] {
    const items: MenuItem[] = [];
    const submenus = new Map<string | null, MenuItem[]>([[null, items]]);
    for (const { folder, parentId, hasChildren } of orderedTargetFolders(book)) {
      const target: MenuItem = { text: folder.name, run: () => { if (stillHere()) convertToNote(note, folder.id, book, root); } };
      const siblings = submenus.get(parentId) ?? items;
      if (!hasChildren) { siblings.push(target); continue; }
      const nested: MenuItem[] = [target, { text: '' }];
      submenus.set(folder.id, nested);
      siblings.push({ text: folder.name, items: nested });
    }
    return items;
  }
  function openMenu(note: Note, card: HTMLElement, menu: HTMLButtonElement, position: Pick<MenuOptions, 'at' | 'anchor'>, touch = false, leading: MenuItem[] = []) {
    if (busy.has(note.id) || getBook().external) return;
    const book = getBook(), root = book.root;
    const stillHere = () => getBook() === book && book.root === root;
    const preview = touch ? card.cloneNode(true) as HTMLElement : undefined;
    preview?.querySelector('.memo-menu')?.remove();
    if (preview) preview.inert = true;
    const useSheet = Boolean(preview) || document.documentElement.hasAttribute('data-mobile');
    const convertItem: MenuItem | undefined = note.readOnly ? undefined
      : useSheet
        ? { text: uiText('memoToNote'), icon: icon(ArrowUpRight), run: () => { if (stillHere()) chooseFolder(note, menu); } }
        : { text: uiText('memoToNote'), icon: icon(ArrowUpRight), items: folderSubmenu(note, book, root, stillHere) };
    void showMenu([
      ...leading,
      ...(!note.readOnly ? [{ text: uiText('menu.edit'), icon: icon(Pencil), run: () => { if (stillHere()) return startEdit(note); } }] : []),
      { text: uiText(note.pinned ? 'unpin' : 'pin'), icon: icon(note.pinned ? PinOff : Pin),
        run: async () => { if (stillHere()) await mutate(note, () => book.togglePin(note.id)); } },
      { text: uiText('memoCopy'), icon: icon(Copy), needsUserActivation: usesGlisse(document.documentElement), run: () => actions.copyText(note.source) },
      ...(convertItem ? [convertItem] : []),
      { text: '' },
      { text: uiText('moveToTrash'), icon: icon(Trash2), destructive: true,
        run: async () => { if (stillHere() && await mutate(note, () => book.trashNote(note.id))) actions.status?.(uiText('memoTrashed')); } },
    ], { ...position, label: uiText('more'), preview, previewAnchor: preview ? card : undefined,
      onError: actions.error, restoreFocus: () => menu.focus({ preventScroll: true }) });
  }
  function chooseFolder(note: Note, anchor: HTMLElement) {
    const book = getBook(), root = book.root;
    const rows = orderedTargetFolders(book);
    showMoveFolderSheet(rows.map(({ folder, depth }) => ({
      name: folder.name, depth, current: false, preferred: folder.id === 'default' || folder.path === '',
      run: () => convertToNote(note, folder.id, book, root),
    })), () => anchor.focus({ preventScroll: true }));
  }

  function closeEdit() {
    const editor = edit?.editor;
    edit = undefined;
    editor?.deactivate();
    editor?.view.destroy();
  }
  async function cancelEdit() {
    if (!edit || busy.has(edit.id)) return false;
    const current = edit;
    if (sourceOf(current.editor) !== current.initial && !await askToConfirm(uiText('memoDiscard'), uiText('memoDiscardAction'), translatedLabel('取消'))) return false;
    if (edit !== current) return false;
    const id = current.id; closeEdit(); storeEditBackup(current.backupKey); renderTimeline();
    cards.get(id)?.node.querySelector<HTMLButtonElement>('.memo-menu')?.focus({ preventScroll: true });
    return true;
  }
  async function startEdit(note: Note) {
    if (note.readOnly || getBook().loadError || busy.has(note.id)) return;
    if (edit?.id === note.id) { edit.editor.focus(); return; }
    const origin = sequence;
    if (edit && !await cancelEdit()) return;
    if (!visible || sequence !== origin) return;
    const node = element('form', 'memo-card memo-edit'), area = element('div', 'memo-input');
    const backupKey = 'qingyu.memo.edit:' + JSON.stringify([draftKey, note.id]);
    let backup = editBackups.get(backupKey);
    try { backup ??= localStorage.getItem(backupKey) ?? undefined; } catch { /* The in-memory copy is still available. */ }
    node.dataset.memoId = note.id;
    const book = getBook();
    const editor = createRichEditor(area, queueDrafts, {
      fieldLabel: () => uiText('memoPlaceholder'), sortCheckedItems: () => false, onError: actions.error,
      openAsset: actions.openLink, mountAssetImage: actions.mountImage,
    });
    editor.activate(backup ?? note.source, note.id, book.root || book.server ? note.id : undefined, assetBase(book, note.id));
    editor.input.dataset.memoControl = 'editor';
    const buttons = element('div', 'memo-edit-actions');
    const cancel = button('button secondary-button', translatedLabel('取消'));
    const submit = button('button primary-button', uiText('menu.saveDocument'));
    submit.type = 'submit'; buttons.append(cancel, submit);
    // 编辑时保留时间行，卡片换成编辑器后还能看出是哪一条。
    const meta = element('div', 'memo-meta'), stamp = element('time', '', memoStamp(note, locale()));
    stamp.dateTime = new Date(note.createdAt).toISOString(); meta.append(stamp);
    node.append(meta, area, buttons);
    const submitEdit = () => {
      if (busy.has(note.id) || editor.view.compositionStarted) return;
      const source = sourceOf(editor);
      editor.setReadOnly(true); submit.disabled = cancel.disabled = true;
      void mutate(note, async () => {
        try {
          await book.save(source, note.id, note.source);
          storeEditBackup(backupKey);
          if (edit?.node === node) closeEdit();
        }
        catch (error) {
          if (error instanceof NoteSaveConflict) {
            storeEditBackup(backupKey, source);
            if (edit?.node === node) closeEdit();
            throw new Error(uiText('memoChanged'));
          }
          throw error;
        }
      }).then(success => {
        if (success) cards.get(note.id)?.node.querySelector<HTMLButtonElement>('.memo-menu')?.focus({ preventScroll: true });
      }).finally(() => {
        if (edit?.node !== node) return;
        editor.setReadOnly(false); submit.disabled = cancel.disabled = false;
      });
    };
    cancel.onclick = () => { void cancelEdit(); };
    node.onsubmit = event => { event.preventDefault(); submitEdit(); };
    bindEditorKeys(area, editor, submitEdit, () => { void cancelEdit(); });
    edit = { id: note.id, backupKey, initial: note.source, node, editor };
    renderTimeline(); editor.focus();
  }

  let press: { timer: ReturnType<typeof setTimeout>; x: number; y: number } | undefined, held = false;
  function cancelPress() { if (press) clearTimeout(press.timer); press = undefined; }
  timeline.addEventListener('pointerdown', event => {
    cancelPress(); held = false;
    const card = event.target instanceof Element && event.target.closest<HTMLElement>('.memo-card:not(.memo-edit)');
    if (!card || !event.isPrimary || event.pointerType !== 'touch' ||
      (event.target as Element).closest('a, audio, .memo-audio, button, input, textarea')) return;
    const x = event.clientX, y = event.clientY;
    press = { x, y, timer: setTimeout(() => {
      press = undefined;
      if (!card.isConnected) return;
      held = true;
      card.dispatchEvent(new PointerEvent('contextmenu', { bubbles: true, cancelable: true, pointerType: 'touch', clientX: x, clientY: y }));
    }, 450) };
  });
  timeline.addEventListener('pointermove', event => {
    if (press && Math.hypot(event.clientX - press.x, event.clientY - press.y) > 10) cancelPress();
  });
  for (const type of ['pointerup', 'pointercancel', 'blur'] as const) window.addEventListener(type, cancelPress);
  timeline.addEventListener('contextmenu', cancelPress, true);
  timeline.addEventListener('click', event => {
    if (held && event.detail) { event.preventDefault(); event.stopImmediatePropagation(); held = false; }
  }, true);

  function refresh() {
    if (!visible) return;
    loadDraft(); localize(); renderPreviews(); updateComposer(); renderTimeline();
  }
  function scheduleDay() {
    const now = new Date(), midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    dayTimer = setTimeout(() => { refresh(); if (visible) scheduleDay(); }, midnight.getTime() - now.getTime() + 100);
  }
  searchButton.ariaExpanded = 'false';
  searchButton.setAttribute('aria-controls', 'memo-search');
  search.id = 'memo-search';
  return {
    show(next: MemoFilter) {
      if (!visible) {
        pane.hidden = false; searchButton.hidden = false; visible = true;
        scroller.scrollTop = 0; limit = 60; scheduleDay(); loadDraft(); renderPreviews(); updateComposer();
      }
      setFilter(next);
    },
    hide() {
      if (!visible) return;
      closeSheet(); persistDrafts(); closeEdit(); closeTagSuggest(); visible = false; pickerSequence = -1; finishRecording();
      dragDepth = 0; form.removeAttribute('data-dragging');
      pausePreviews();
      pane.hidden = true; searchButton.hidden = true;
      clearTimeout(dayTimer); closeSearch(); cancelPress();
      observer?.unobserve(sentinel); timeline.replaceChildren(); clearCards();
    },
    refresh,
    closeSearch,
    focusSearch,
    save() {
      if (edit?.node.contains(document.activeElement)) edit.node.requestSubmit();
      else void saveMemo();
    },
    focusComposer: () => { if (!phone()) composer.focus(); },
    openComposer: openSheet,
    startRecording: () => recordAudio(),
    closeComposer: closeSheet,
  };
}
