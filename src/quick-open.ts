import { FileText, Folder, List, Search, TextSearch, X, type IconNode } from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { markdownBody, noteText, noteTitle, plainMarkdownLine } from './document-text';
import type { Library, Note } from './notebook';

export type QuickCommand = { title: string; icon: IconNode; detail?: string; shortcut?: string; keywords?: string[]; run: () => void };
type Choice = { id?: string; title: string; preview?: string; folder?: string; shortcut?: string; lines?: string[]; icon: IconNode; run: () => void };
type Section = { title?: string; rows: Choice[] };
const limit = 8;
const normalize = (value: string) => value.normalize('NFD').replace(/\p{M}/gu, '').toLocaleLowerCase();

function matchText(target: HTMLElement, original: string, query: string) {
  const text = original.normalize('NFC'), folded = normalize(text);
  const at = query && folded.length === text.length ? folded.indexOf(query) : -1;
  if (at < 0) { target.textContent = original; return; }
  const mark = document.createElement('mark');
  mark.textContent = text.slice(at, at + query.length);
  target.append(text.slice(0, at), mark, text.slice(at + query.length));
}

export function quickOpenScore(title: string, query: string) {
  if (!query) return 0;
  if (title === query) return 0;
  if (title.startsWith(query)) return 1;
  for (let at = title.indexOf(query); at >= 0; at = title.indexOf(query, at + 1)) {
    if (!/[\p{L}\p{N}]$/u.test(title.slice(0, at))) return 2;
  }
  if (title.includes(query)) return 3;
  const words = query.split(/\s+/u);
  if (words.length > 1 && words.every(word => title.includes(word))) return 4;
  return Infinity;
}

function snippet(line: string, index: number) {
  const start = Math.max(0, index - 28), end = start + 120;
  return (start ? '…' : '') + line.slice(start, end) + (end < line.length ? '…' : '');
}

export async function quickOpenTextHits(notes: readonly Note[], query: string, signal: AbortSignal) {
  const hits: { note: Note; lines: string[] }[] = [];
  let sliceStart = performance.now();
  for (const note of notes) {
    if (signal.aborted) return [];
    if (note.readOnly || note.trashed) continue;
    const lines: string[] = [];
    let count = 0;
    const { body } = markdownBody(note.source);
    for (const match of body.matchAll(/[^\r\n]*(?:\r\n?|\n|$)/gu)) {
      if (!match[0]) break;
      const text = plainMarkdownLine(match[0]);
      const at = normalize(text).indexOf(query);
      if (at >= 0) {
        if (count < 3) lines.push(snippet(text, at));
        count++;
      }
      if (performance.now() - sliceStart > 5) {
        await new Promise<void>(resolve => window.setTimeout(resolve, 0));
        if (signal.aborted) return [];
        sliceStart = performance.now();
      }
    }
    if (count > 3) lines.push(`另有 ${count - 3} 处匹配`);
    if (lines.length) hits.push({ note, lines });
    if (hits.length === limit + 1) break;
  }
  return hits;
}

export function openQuickOpen({ library, commands, openNote, showAll, restore, closed }: {
  library: Library;
  commands: () => QuickCommand[];
  openNote: (note: Note) => void;
  showAll: (query: string) => void;
  restore: () => void;
  closed: () => void;
}) {
  const folders = new Map(library.folders.map(folder => [folder.id, folder.name]));
  const notes = library.notes.filter(note => !note.trashed).sort((a, b) => b.updatedAt - a.updatedAt).map(note => {
    const title = noteTitle(note), folder = folders.get(note.folderId);
    return { note, title, folder, normalizedTitle: normalize(title), normalizedFolder: normalize(folder ?? '') };
  });
  const panel = document.createElement('div');
  panel.className = 'search-picker quick-open';
  panel.popover = 'manual';
  panel.role = 'dialog';
  panel.ariaLabel = '跳转到笔记';
  panel.innerHTML = '<header class="search-picker-header"><input type="search" role="combobox" aria-label="跳转到笔记" aria-autocomplete="list" aria-controls="quick-open-results" placeholder="跳转到笔记，或输入命令"><kbd aria-hidden="true"></kbd><button class="icon-button" type="button" aria-label="关闭笔记跳转"></button></header><div id="quick-open-results" role="listbox" aria-label="搜索结果"></div><p role="status" hidden></p>';
  const header = panel.querySelector('header')!, input = panel.querySelector('input')!;
  const list = panel.querySelector<HTMLDivElement>('#quick-open-results')!, closeButton = header.querySelector('button')!;
  const status = panel.querySelector('p')!;
  header.prepend(createLucideIcon(document, Search, ''));
  header.querySelector('kbd')!.textContent = /Mac/.test(navigator.platform) ? '⌘K' : 'Ctrl K';
  closeButton.append(createLucideIcon(document, X, ''));
  let timer = 0, read: AbortController | undefined, highlighted = 0, value: string | undefined;
  let options: HTMLButtonElement[] = [];

  function close(restoreFocus = true) {
    window.clearTimeout(timer);
    read?.abort();
    const focused = panel.contains(document.activeElement);
    panel.hidePopover();
    panel.remove();
    closed();
    if (restoreFocus && focused) restore();
  }
  function highlight(index: number, scroll = false) {
    highlighted = index;
    const active = options[index];
    for (const button of options) button.ariaSelected = String(button === active);
    if (active) {
      input.setAttribute('aria-activedescendant', active.id);
      if (scroll) active.scrollIntoView({ block: 'nearest' });
    } else input.removeAttribute('aria-activedescendant');
  }
  function render(sections: Section[], pending: boolean) {
    const previous = options[highlighted]?.dataset.row;
    const scroll = list.scrollTop;
    const query = normalize(value?.trim() ?? '');
    options = [];
    list.replaceChildren();
    for (const { title: heading, rows } of sections) {
      if (!rows.length) continue;
      const group = document.createElement('div');
      if (heading) {
        group.role = 'group'; group.ariaLabel = heading;
        const label = document.createElement('div');
        label.className = 'quick-open-section'; label.textContent = heading; label.ariaHidden = 'true';
        group.append(label); list.append(group);
      }
      for (const row of rows) {
        const button = document.createElement('button');
        button.type = 'button'; button.role = 'option'; button.tabIndex = -1;
        button.id = `quick-open-option-${options.length}`;
        button.dataset.row = row.id ?? row.title;
        button.classList.toggle('quick-open-command', heading === '命令');
        const label = document.createElement('strong'); matchText(label, row.title, query);
        const content = document.createElement('span'); content.append(label);
        if (row.preview) { const preview = document.createElement('span'); preview.textContent = row.preview; content.append(preview); }
        button.append(createLucideIcon(document, row.icon, ''), content);
        if (row.lines) {
          content.className = 'quick-open-lines';
          for (const line of row.lines) { const detail = document.createElement('span'); matchText(detail, line, query); content.append(detail); }
        }
        if (row.folder || row.shortcut) {
          const detail = document.createElement('small');
          if (row.folder) detail.append(createLucideIcon(document, Folder, ''), row.folder);
          else detail.textContent = row.shortcut ?? '';
          button.append(detail);
        }
        button.onmousedown = event => event.preventDefault();
        button.onpointerenter = () => highlight(options.indexOf(button));
        button.onclick = () => { close(false); row.run(); };
        options.push(button); (heading ? group : list).append(button);
      }
    }
    list.hidden = !options.length;
    status.hidden = Boolean(options.length);
    status.textContent = pending ? '正在搜索笔记…' : value?.trim() ? '未找到匹配的笔记或命令' : '暂无笔记';
    input.ariaExpanded = String(Boolean(options.length));
    highlight(Math.max(0, options.findIndex(button => button.dataset.row === previous)));
    list.scrollTop = scroll;
  }

  function refresh() {
    if (value === input.value) return;
    value = input.value;
    options = []; highlighted = 0; list.scrollTop = 0;
    window.clearTimeout(timer);
    read?.abort();
    const query = normalize(value.trim());
    const matches = notes.map(entry => ({ ...entry, score: Math.min(quickOpenScore(entry.normalizedTitle, query),
      quickOpenScore(entry.normalizedFolder, query) < Infinity ? 5 : Infinity) })).filter(entry => entry.score < Infinity)
      .sort((a, b) => a.score - b.score);
    const actions = commands().map(command => ({ ...command, score: Math.min(quickOpenScore(normalize(command.title), query),
      command.keywords?.some(word => normalize(word).startsWith(query)) ? 2 : Infinity) })).filter(command => command.score <= 3)
      .sort((a, b) => a.score - b.score);
    const noteSection = { title: query ? '笔记' : '最近笔记', rows: matches.slice(0, limit).map(({ note, title, folder }) => {
      const { summary } = note.readOnly ? { summary: '只读' } : noteText(note.source);
      return { id: note.id, title, folder, preview: summary === '无更多文本' ? undefined : summary, icon: FileText, run: () => openNote(note) };
    }) };
    const commandSection = { title: '命令', rows: actions.map(({ detail, ...row }) => ({ ...row, preview: detail })) };
    const sections: Section[] = (actions[0]?.score ?? Infinity) < (matches[0]?.score ?? Infinity)
      ? [commandSection, noteSection] : [noteSection, commandSection];
    const all: Section = { rows: [{ title: `显示所有包含「${value.trim()}」的笔记`, icon: List, run: () => showAll(input.value.trim()) }] };
    const searchText = [...query].length >= 2;
    render(query && (matches.length > limit || !searchText && sections.some(section => section.rows.length)) ? [...sections, all] : sections, searchText);
    if (!searchText) return;
    const controller = read = new AbortController();
    const listed = new Set(matches.map(({ note }) => note.id));
    timer = window.setTimeout(async () => {
      const hits = await quickOpenTextHits(notes.filter(({ note }) => !listed.has(note.id)).map(({ note }) => note), query, controller.signal);
      if (controller.signal.aborted) return;
      const textSection = { title: '笔记正文', rows: hits.slice(0, limit).map(({ note, lines }) => ({
        id: note.id, title: noteTitle(note), folder: folders.get(note.folderId), lines, icon: TextSearch, run: () => openNote(note),
      })) };
      render(matches.length > limit || hits.length > limit ? [...sections, textSection, all] : [...sections, textSection], false);
    }, 120);
  }
  let composing = false;
  input.addEventListener('compositionstart', () => { composing = true; });
  input.addEventListener('compositionend', () => { composing = false; refresh(); });
  input.oninput = event => { if (!composing && !(event as InputEvent).isComposing) refresh(); };
  panel.onkeydown = event => {
    if (event.isComposing || composing) return;
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
    else if (event.target === input && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
      event.preventDefault();
      highlight(Math.max(0, Math.min(options.length - 1, highlighted + (event.key === 'ArrowDown' ? 1 : -1))), true);
    } else if (event.target === input && event.key === 'Enter') { event.preventDefault(); options[highlighted]?.click(); }
  };
  closeButton.onclick = () => close();
  document.body.append(panel);
  refresh(); panel.showPopover(); input.focus();
  return { panel, input, close, focus: () => input.focus() };
}
