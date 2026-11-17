import { noteTitle, type Library, type Note } from './notebook';
import { markdownBody, noteText, plainMarkdownLine } from './document-text';
import { GFM, parser } from '@lezer/markdown';
import { markraWikiLink, readWikiLink } from './vendor/markra/editor/codemirror/wiki-link';

const titleKey = (text: string) => text.trim().replace(/\s+/gu, ' ').toLowerCase();
const canLink = (note: Note) => !note.trashed && !/[\[\]|\r\n]/u.test(noteTitle(note));
const fullTitle = (library: Library, note: Note) => {
  const folder = library.folders.find(folder => folder.id === note.folderId);
  return folder ? `${folder.path || folder.name}/${noteTitle(note)}` : noteTitle(note);
};

export function resolveNoteLink(library: Library, written: string, current: Note | undefined) {
  function find(target: string) {
    if (!target.trim()) return current?.trashed ? undefined : current;
    const key = titleKey(target);
    const notes = library.notes.filter(note => !note.trashed);
    const titles = notes.filter(note => titleKey(noteTitle(note)) === key);
    return notes.find(note => titleKey(fullTitle(library, note)) === key)
      ?? titles.find(note => note.folderId === current?.folderId)
      ?? (titles.length === 1 ? titles[0] : undefined);
  }
  const exact = find(written);
  if (exact) return { note: exact, heading: undefined };
  const hash = written.indexOf('#');
  return hash < 0 ? { note: undefined, heading: undefined }
    : { note: find(written.slice(0, hash)), heading: written.slice(hash + 1).trim() };
}

async function noteHeadings(source: string) {
  const { getMarkdownHeadings } = await import('@markra/markdown');
  return getMarkdownHeadings(source).map(({ from, title, titleMarkdown = title }) => ({
    from,
    title: titleMarkdown.includes('[[') ? plainMarkdownLine(titleMarkdown) : title,
    markdown: titleMarkdown,
  }));
}

export async function noteHeadingOffset(source: string, heading: string) {
  const key = titleKey(heading);
  if (!key) return undefined;
  const headings = (await noteHeadings(source)).map(({ from, title, markdown }) => ({
    from, key: titleKey(title), raw: titleKey(markdown),
  }));
  const exact = headings.find(heading => heading.key === key || heading.raw === key);
  return exact?.from ?? headings.find(heading => heading.key.replace(/[^\p{L}\p{N} -]/gu, '').replace(/ +/g, '-') === key)?.from;
}

export async function noteLinkHeadingChoices(library: Library, current: Note | undefined, query: string, draft?: string) {
  const hash = query.indexOf('#');
  if (hash === -1) return [];
  const target = query.slice(0, hash).trim();
  if (!target) return [];
  const { note } = resolveNoteLink(library, target, current);
  if (!note || note.readOnly) return [];
  const body = markdownBody(note.id === current?.id ? draft ?? note.source : note.source).body;
  const first = body.search(/\S/u);
  const typed = titleKey(query.slice(hash + 1));
  const seen = new Set<string>();
  const starts: string[] = [], contains: string[] = [];
  for (const { from, title } of await noteHeadings(body)) {
    if (from <= first || /[\[\]|\r\n]/u.test(title)) continue;
    const key = titleKey(title);
    if (!key || !key.includes(typed) || seen.has(key)) continue;
    seen.add(key);
    (key.startsWith(typed) ? starts : contains).push(title);
  }
  const prefix = query.slice(0, hash + 1);
  return starts.concat(contains).slice(0, 8).map(title => ({ title, target: prefix + title }));
}

export function describeNoteLink(library: Library, target: string, current: Note | undefined) {
  const { note } = resolveNoteLink(library, target, current);
  if (!note) return undefined;
  const { summary } = noteText(note.source);
  const folder = library.folders.find(item => item.id === note.folderId)?.name ?? '备忘录';
  return { detail: summary === '无更多文本' ? undefined : summary, folder };
}

export function noteLinkChoices(library: Library, currentId: string, query: string) {
  const key = titleKey(query);
  const candidates = library.notes.filter(canLink);
  const counts = new Map<string, number>();
  const firstByPath = new Map<string, string>();
  for (const note of candidates) {
    const title = titleKey(noteTitle(note));
    counts.set(title, (counts.get(title) ?? 0) + 1);
    const path = titleKey(fullTitle(library, note));
    if (!firstByPath.has(path)) firstByPath.set(path, note.id);
  }
  return candidates.flatMap(note => {
    if (note.id === currentId) return [];
    const title = noteTitle(note), normalized = titleKey(title);
    const duplicate = (counts.get(normalized) ?? 0) > 1;
    const target = duplicate ? fullTitle(library, note) : title;
    if (!titleKey(target).includes(key) || /[\[\]|\r\n]/u.test(target)) return [];
    const owner = firstByPath.get(titleKey(target));
    if (owner && owner !== note.id) return [];
    return [{ title, target, detail: duplicate ? target.slice(0, -(title.length + 1)) : '', note }];
  }).sort((a, b) => Number(titleKey(b.title).startsWith(key)) - Number(titleKey(a.title).startsWith(key))
    || b.note.updatedAt - a.note.updatedAt || a.title.localeCompare(b.title)).slice(0, 8);
}

export function noteLinkMarkdown(target: string, selected: string) {
  const alias = selected.trim();
  return alias && titleKey(alias) !== titleKey(target.split('/').at(-1) ?? target) && !/[\[\]|\r\n]/u.test(alias)
    ? `[[${target}|${alias}]]` : `[[${target}]]`;
}

const wikiParser = parser.configure([GFM, markraWikiLink]);
export function noteLinkTargets(source: string, cardsOnly = false) {
  const targets = new Set<string>();
  if (!source.includes('[[')) return targets;
  wikiParser.parse(source).iterate({ enter(node) {
    const card = node.name === 'WikiCard';
    if (!card && (cardsOnly || node.name !== 'WikiLink')) return;
    const link = readWikiLink(source.slice(node.from + Number(card), node.to));
    if (link) targets.add(link.target);
  } });
  return targets;
}
