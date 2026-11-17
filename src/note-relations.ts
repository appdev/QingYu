import { GFM, parser } from '@lezer/markdown';
import { markdownBody, plainMarkdownLine } from './document-text';
import { noteTitle, type Library, type Note } from './notebook';
import { resolveNoteLink } from './note-links';
import { markraWikiLink, readWikiLink } from './vendor/markra/editor/codemirror/wiki-link';

type Range = { from: number; to: number };
export type RelationLine = { number: number; spots: Range[]; text: string; marks: Range[] };
export type RelationNote = { id: string; title: string; source: string; lines: RelationLine[] };
export type NoteRelations = { linked: RelationNote[]; mentions: RelationNote[] };

export function sameRelationInputs(a: Library, b: Library, id: string) {
  if (a === b) return true;
  if (a.folders.length !== b.folders.length || a.notes.length !== b.notes.length) return false;
  const front = (source: string) => source.slice(0, source.length - markdownBody(source).body.length);
  return a.folders.every((folder, i) => {
    const other = b.folders[i];
    return folder.id === other.id && folder.path === other.path && folder.name === other.name;
  }) && a.notes.every((note, i) => {
    const other = b.notes[i];
    if (note === other) return true;
    if (note.id !== other.id || note.folderId !== other.folderId || note.trashed !== other.trashed || note.readOnly !== other.readOnly) return false;
    return note.id === id ? noteTitle(note) === noteTitle(other) && front(note.source) === front(other.source)
      : note.source === other.source && note.title === other.title && note.updatedAt === other.updatedAt;
  });
}

const markdown = parser.configure([GFM, markraWikiLink]);
const protectedNodes = new Set(['Link', 'Image', 'Autolink', 'URL', 'InlineCode', 'FencedCode', 'CodeBlock',
  'HTMLBlock', 'HTMLTag', 'CommentBlock', 'Comment', 'LinkReference', 'WikiLink', 'WikiCard']);
const word = /[\p{L}\p{N}_]/u;
const accents = /\p{M}/gu;
const folded = (value: string) => value.normalize('NFD').replace(accents, '').toLowerCase();
const unquote = (value: string) => value.trim().replace(/^(["'])(.*)\1$/u, '$2');

function names(note: Note) {
  const front = note.source.slice(0, note.source.length - markdownBody(note.source).body.length);
  const values = [noteTitle(note)];
  for (const item of front.matchAll(/^(?:alias|aliases):[ \t]*([^\r\n]*)(\r?\n(?:[ \t]+-[ \t]+[^\r\n]*\r?\n?)*)/gmu)) {
    const scalar = item[1].trim();
    if (scalar.startsWith('[') && scalar.endsWith(']')) values.push(...scalar.slice(1, -1).split(',').map(unquote));
    else if (scalar) values.push(unquote(scalar));
    for (const entry of item[2].matchAll(/^[ \t]+-[ \t]+(.*)$/gmu)) values.push(unquote(entry[1]));
  }
  return [...new Set(values.map(value => folded(value.trim())))]
    .filter(value => [...value].length >= 3 && !['new note', '未命名'].includes(value))
    .sort((a, b) => b.length - a.length);
}

function sourceRanges(body: string) {
  const excluded: Range[] = [], links: (Range & { target: string })[] = [];
  markdown.parse(body).iterate({ enter(node) {
    if (!protectedNodes.has(node.name)) return;
    const { from, to } = node;
    if (node.name === 'WikiLink' || node.name === 'WikiCard') {
      const link = readWikiLink(body.slice(from + Number(node.name === 'WikiCard'), to));
      if (link) links.push({ from, to, target: link.target });
    }
    excluded.push({ from, to });
    return false;
  } });
  for (const url of body.matchAll(/[a-z][a-z0-9+.-]*:\/\/[^\s]+/giu)) excluded.push({ from: url.index!, to: url.index! + url[0].length });
  return { links, excluded };
}

function mentionSpots(body: string, needles: string[], excluded: Range[]) {
  const spans: Range[] = [];
  if (!needles.length) return spans;
  let offset = 0;
  for (const [line] of body.matchAll(/[^\r\n]*(?:\r\n|\r|\n|$)/g)) {
    const origin = offset;
    offset += line.length;
    if (!origin || !line.trim()) continue;
    const normalized = folded(line);
    if (!needles.some(name => normalized.includes(name))) continue;
    const starts: number[] = [], ends: number[] = [];
    let original = origin;
    for (const character of line) {
      const size = folded(character).length;
      for (let i = 0; i < size; i++) { starts.push(original); ends.push(original + character.length); }
      original += character.length;
      if (!size && ends.length) ends[ends.length - 1] = original;
    }
    const row: Range[] = [];
    for (const name of needles) {
      let start = 0;
      while ((start = normalized.indexOf(name, start)) !== -1) {
        const end = start + name.length;
        const from = starts[start], to = ends[end - 1];
        const standalone = !word.test(normalized[start - 1] ?? '') && !word.test(normalized[end] ?? '');
        if (standalone && !excluded.some(range => range.from < to && from < range.to)
          && !row.some(range => range.from < to && from < range.to)) row.push({ from, to });
        start = end;
      }
    }
    spans.push(...row.sort((a, b) => a.from - b.from));
  }
  return spans;
}

function snippets(body: string, spots: Range[], lineOffset: number, sourceOffset: number): RelationLine[] {
  if (!spots.length) return [];
  const result: RelationLine[] = [];
  let i = 0, number = lineOffset, offset = 0;
  for (const [line] of body.matchAll(/[^\r\n]*(?:\r\n|\r|\n|$)/g)) {
    const limit = offset + line.length;
    number++;
    if (spots[i].from < limit) {
      const text = plainMarkdownLine(line);
      const marks: Range[] = [], positions: Range[] = [];
      let begin = 0;
      while (i < spots.length && spots[i].from < limit) {
        const { from, to } = spots[i++];
        positions.push({ from: from + sourceOffset, to: to + sourceOffset });
        const excerpt = plainMarkdownLine(body.slice(from, to));
        const prefix = plainMarkdownLine(body.slice(offset, from));
        const found = excerpt ? text.indexOf(excerpt, Math.max(prefix.length, begin)) : -1;
        if (found !== -1) { begin = found + excerpt.length; marks.push({ from: found, to: begin }); }
      }
      const clip = Math.max(0, (marks[0]?.from ?? 0) - 56);
      const visible = text.slice(clip, clip + 160);
      const prefix = clip ? '…' : '', suffix = clip + 160 < text.length ? '…' : '';
      result.push({ number, spots: positions, text: prefix + visible + suffix,
        marks: marks.filter(mark => mark.from >= clip && mark.to <= clip + 160)
          .map(({ from, to }) => ({ from: from - clip + prefix.length, to: to - clip + prefix.length })) });
      if (result.length === 5 || i === spots.length) break;
    }
    offset = limit;
  }
  return result;
}

export async function findNoteRelations(library: Library, target: Note, signal: AbortSignal): Promise<NoteRelations> {
  const related: NoteRelations = { linked: [], mentions: [] };
  const needles = names(target);
  const key = (title: string) => title.trim().replace(/\s+/gu, ' ').toLowerCase();
  const title = noteTitle(target), folder = library.folders.find(item => item.id === target.folderId);
  const linkNames = [key(title), ...(folder ? [key(`${folder.path || folder.name}/${title}`)] : [])];
  const peers = library.notes.filter(note => note.id !== target.id && !note.trashed && !note.readOnly)
    .sort((a, b) => b.updatedAt - a.updatedAt);
  let chunk = performance.now();
  for (const note of peers) {
    if (signal.aborted) break;
    const { body, offset } = markdownBody(note.source);
    const searchable = needles.length ? folded(body) : '';
    if (body.includes('[[') || needles.some(name => searchable.includes(name))) {
      const { links, excluded } = sourceRanges(body);
      const references = links.filter(link => {
        const written = key(link.target);
        const hash = written.indexOf('#');
        const beforeHeading = hash < 0 ? written : written.slice(0, hash).trim();
        return linkNames.some(name => written === name || beforeHeading === name)
          && resolveNoteLink(library, link.target, note).note?.id === target.id;
      });
      const matches = mentionSpots(body, needles, excluded);
      const base = { id: note.id, title: noteTitle(note), source: note.source };
      const sourceOffset = note.source.length - body.length;
      if (references.length) related.linked.push({ ...base, lines: snippets(body, references, offset, sourceOffset) });
      if (matches.length) related.mentions.push({ ...base, lines: snippets(body, matches, offset, sourceOffset) });
    }
    if (performance.now() - chunk > 8) {
      await new Promise<void>(resolve => setTimeout(resolve, 0));
      chunk = performance.now();
    }
  }
  return related;
}

export function linkMentionLine(library: Library, target: Note, other: Note, spans: Range[]) {
  const title = noteTitle(target);
  const folder = library.folders.find(item => item.id === target.folderId);
  const path = resolveNoteLink(library, title, other).note?.id === target.id ? title : `${folder?.path || folder?.name}/${title}`;
  if (/[\[\]|\r\n]/u.test(path) || resolveNoteLink(library, path, other).note?.id !== target.id) return;
  const { body } = markdownBody(other.source);
  const offset = other.source.length - body.length;
  const current = mentionSpots(body, names(target), sourceRanges(body).excluded);
  const confirmed = spans.filter(span => current.some(spot => spot.from + offset === span.from && spot.to + offset === span.to));
  let source = other.source;
  for (const { from, to } of confirmed.reverse()) {
    const displayed = source.slice(from, to);
    if (/[\[\]|\r\n]/u.test(displayed)) continue;
    const markup = displayed === path ? `[[${path}]]` : `[[${path}|${displayed}]]`;
    source = source.slice(0, from) + markup + source.slice(to);
  }
  return source;
}
