import MarkdownIt from 'markdown-it';
import taskLists from 'markdown-it-task-lists';
import { dailyDateKey } from './daily-note';
import { noteMatches } from './document-text';
import type { Folder, Library, Note } from './notebook';
import { uiText } from './ui-language';

export const memoFolderName = '随记';
export type MemoFilter = { kind: 'all' } | { kind: 'todo' } | { kind: 'tag'; tag: string };
export const allMemos: MemoFilter = { kind: 'all' };
const normalizedName = (name: string) => name.normalize('NFD').replace(/\p{M}/gu, '').trim().toLowerCase();
export function isMemoFolderName(name: string) {
  const normalized = normalizedName(name);
  return normalized === memoFolderName || normalized === 'jots';
}
export function isInsideMemoFolder(folder: Folder) {
  return folder.id !== 'default' && isMemoFolderName((folder.path ?? folder.name).split('/')[0]);
}
export const memoTagKey = (tag: string) => tag.normalize('NFC').toLowerCase();

export function findMemoFolder(library: Library) {
  const roots = library.folders.filter(folder => folder.id !== 'default' && !(folder.path ?? folder.name).includes('/'));
  return roots.find(folder => normalizedName(folder.name) === memoFolderName)
    ?? roots.find(folder => isMemoFolderName(folder.name));
}

export function isMemo(library: Library, note: Note) {
  return note.folderId === findMemoFolder(library)?.id;
}

export function memoFileTitle(now: Date) {
  const time = [now.getHours(), now.getMinutes(), now.getSeconds()].map(value => String(value).padStart(2, '0'));
  return `${dailyDateKey(now)} ${time.join('-')}`;
}

const tagPattern = /(?:^|[\s\u3000-\u303f\uff01-\uff0f\uff1a-\uff20\u2018-\u201f])#([\p{L}\p{N}_/-]{1,32})(?![\p{L}\p{N}_/-])/gu;
const taskLine = /^((?:[ \t]*>[ \t]?)*[ \t]*(?:[-+*]|\d{1,9}[.)])[ \t]+\[)([ xX])(?=\](?:[ \t]|$))/u;
type MemoMarkdown = { tags?: Map<string, string>; tasks?: number[] };
const markdown = new MarkdownIt({ html: false, linkify: true, breaks: true }).use(taskLists);
markdown.core.ruler.push('memo-tags', state => {
  const env: MemoMarkdown = state.env;
  for (const token of state.tokens) {
    if (token.type !== 'inline' || !token.children) continue;
    if (token.map && token.children[0]?.content.includes('class="task-list-item-checkbox"')) env.tasks?.push(token.map[0]);
    let inLink = false;
    token.children = token.children.flatMap(child => {
      if (child.type === 'link_open' || child.type === 'link_close') inLink = child.type === 'link_open';
      if (inLink || child.type !== 'text' || !child.content.includes('#')) return [child];
      const tokens = [];
      const appendText = (value: string) => {
        if (!value) return;
        const text = new state.Token('text', '', 0);
        text.content = value; tokens.push(text);
      };
      let position = 0;
      for (const match of child.content.matchAll(tagPattern)) {
        const tag = match[1].replace(/\/+$/u, '');
        if (!tag || /^\p{N}+$/u.test(tag)) continue;
        const start = match.index + match[0].indexOf('#');
        appendText(child.content.slice(position, start));
        const key = memoTagKey(tag);
        if (env.tags && !env.tags.has(key)) env.tags.set(key, tag);
        const open = new state.Token('memo_tag_open', 'button', 1);
        open.attrs = [['type', 'button'], ['class', 'memo-tag'], ['data-tag', tag]];
        tokens.push(open); appendText(`#${tag}`);
        tokens.push(new state.Token('memo_tag_close', 'button', -1));
        position = start + tag.length + 1;
      }
      if (!position) return [child];
      appendText(child.content.slice(position));
      return tokens;
    });
  }
});

type MemoAnalysis = { tags: string[]; tasks: { position: number; checked: boolean }[]; html: string };
const analysisCache = new WeakMap<Note, MemoAnalysis>();
function analyze(source: string): MemoAnalysis {
  const tags = new Map<string, string>(), rows: number[] = [];
  const env = { tags, tasks: rows };
  const tokens = markdown.parse(source, env);
  const lines = [...source.matchAll(/[^\r\n]*(?:\r\n|\r|\n|$)/gu)];
  const tasks = rows.map(row => {
    const line = lines[row], match = line?.[0].match(taskLine);
    return { position: match ? line.index + match[1].length : -1, checked: Boolean(match && match[2] !== ' ') };
  });
  return { tasks, tags: [...tags.values()], html: markdown.renderer.render(tokens, markdown.options, env) };
}

function analyzeNote(note: Note) {
  let analysis = analysisCache.get(note);
  if (!analysis) { analysis = analyze(note.source); analysisCache.set(note, analysis); }
  return analysis;
}

export function memoTags(source: string) { return analyze(source).tags; }
export function hasOpenTask(source: string) { return analyze(source).tasks.some(task => task.position >= 0 && !task.checked); }
export function renderMemo(note: Note) { return analyzeNote(note).html; }

export function toggleTask(source: string, index: number) {
  const task = analyze(source).tasks[index];
  if (!task || task.position < 0) return source;
  return source.slice(0, task.position) + (task.checked ? ' ' : 'x') + source.slice(task.position + 1);
}

export function regularNotes(library: Library) {
  const memoId = findMemoFolder(library)?.id;
  return memoId ? library.notes.filter(note => note.trashed || note.folderId !== memoId) : library.notes;
}

export function memoNotes(library: Library, filter: MemoFilter, query: string) {
  const id = findMemoFolder(library)?.id, search = query.trim().toLocaleLowerCase();
  if (!id) return [];
  return library.notes.filter(note => note.folderId === id && !note.trashed
    && (!search || noteMatches(note, search))
    && (filter.kind !== 'todo' || analyzeNote(note).tasks.some(task => task.position >= 0 && !task.checked))
    && (filter.kind !== 'tag' || analyzeNote(note).tags.some(tag => memoTagKey(tag) === memoTagKey(filter.tag))))
    .sort((a, b) => Number(Boolean(b.pinned)) - Number(Boolean(a.pinned)) || b.createdAt - a.createdAt);
}

export function memoTagCounts(library: Library): [string, number][] {
  const counts = new Map<string, [string, number]>();
  for (const note of memoNotes(library, allMemos, '')) {
    for (const tag of analyzeNote(note).tags) {
      const key = memoTagKey(tag);
      const entry = counts.get(key);
      if (entry) entry[1]++;
      else counts.set(key, [tag, 1]);
    }
  }
  return [...counts.values()].sort(([a, x], [b, y]) => y - x || a.localeCompare(b));
}

export function groupMemosByDay(notes: Note[], now: Date, locale: string) {
  const today = dailyDateKey(now);
  const yesterday = dailyDateKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1));
  const relative = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const capitalize = (text: string) => text ? text[0].toLocaleUpperCase(locale) + text.slice(1) : text;
  const sameYear = new Intl.DateTimeFormat(locale, { month: 'long', day: 'numeric', weekday: 'short' });
  const otherYear = new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'long', day: 'numeric' });
  const groups = new Map<string, { label: string; key: string; notes: Note[] }>();
  for (const note of notes) {
    const date = new Date(note.createdAt), key = note.pinned ? 'pinned' : dailyDateKey(date);
    let group = groups.get(key);
    if (!group) {
      const label = note.pinned ? uiText('pinned', locale) : key === today ? capitalize(relative.format(0, 'day'))
        : key === yesterday ? capitalize(relative.format(-1, 'day'))
          : (date.getFullYear() === now.getFullYear() ? sameYear : otherYear).format(date);
      group = { label, key, notes: [] }; groups.set(key, group);
    }
    group.notes.push(note);
  }
  return [...groups.values()];
}
