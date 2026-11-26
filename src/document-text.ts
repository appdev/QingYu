import { markdownImageSizeSuffix, unescapeMarkdown } from './vendor/markra/editor/codemirror/syntax';
import { isLocalAsset } from './server-assets';

export function markdownBody(source: string): { body: string; offset: number } {
  const match = source.match(/^---\r?\n[\s\S]*?\r?\n(?:---|\.\.\.)(?:\r?\n|$)/);
  if (!match) return { body: source, offset: 0 };
  return { body: source.slice(match[0].length), offset: (match[0].match(/\n/g) ?? []).length };
}

export function plainMarkdownLine(line: string) {
  return line.replace(/^\s*(?:#{1,6}\s+|>\s*|[-*+]\s+(?:\[[ xX]\]\s*)?|\d+[.)]\s+)/, '')
    .replace(/!?\[\[([^\[\]\r\n]+)\]\]/gu, (_markup, inside: string) => {
      const [target, alias] = inside.split('|', 2);
      return alias?.trim() || target.split('/').at(-1)?.trim() || target;
    })
    .replace(/(!?)\[((?:\\.|[^\]\\])*)\]\([^)]*\)/g, (markup, image: string, label: string, _index, text: string) =>
      unescapeMarkdown(image && markup === text.trim() ? label.replace(markdownImageSizeSuffix, '') : label))
    .replace(/<[^>]+>/g, '')
    .replace(/[*_`~]/g, '').trim();
}

export function noteText(source: string, untitled = '未命名') {
  let title = '', summary = '';
  // Metadata only needs the first two meaningful lines, even for a large document.
  for (const [line] of markdownBody(source).body.matchAll(/[^\r\n]+/g)) {
    if (!line.trim() || /^\s*(?:[-=]{3,}|`{3,}.*|~{3,}.*|\$\$)\s*$/.test(line)) continue;
    const text = plainMarkdownLine(line);
    if (!text) continue;
    if (!title) title = text.slice(0, 160);
    else { summary = text.slice(0, 180); break; }
  }
  return { title: title || untitled, summary: summary || '无更多文本' };
}

export function firstTitleLineEnd(source: string) {
  const { body } = markdownBody(source);
  for (const match of body.matchAll(/[^\r\n]+/g)) {
    if (noteText(match[0], '').title) return source.length - body.length + match.index + match[0].length;
  }
}

export function replaceFirstTitleLine(source: string, title: string) {
  if (noteText(source, '').title === title) return source;
  const end = firstTitleLineEnd(source);
  if (end === undefined) {
    const { body } = markdownBody(source);
    const start = source.length - body.length;
    const newline = /\r\n|\n|\r/u.exec(source)?.[0] ?? '\n';
    return source.slice(0, start) + (start && !source.slice(0, start).endsWith('\n') ? newline : '') + title + newline + body;
  }
  const start = Math.max(source.lastIndexOf('\n', end - 1), source.lastIndexOf('\r', end - 1)) + 1;
  const line = source.slice(start, end);
  const prefix = /^\s*#{1,6}[ \t]+/u.exec(line)?.[0] ?? '';
  return source.slice(0, start) + prefix + title + source.slice(end);
}

/** 笔记里第一张本地图片的路径，列表行尾的缩略图用它。代码块里的不算，网络图片不算。 */
export function firstLocalImage(source: string) {
  let fence = '';
  for (const [line] of markdownBody(source).body.matchAll(/[^\r\n]+/g)) {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker) {
      if (!fence) fence = marker[0].repeat(marker.length);
      else if (marker.startsWith(fence)) fence = '';
      continue;
    }
    if (fence) continue;
    for (const match of line.matchAll(/!\[(?:\\.|[^\]\\])*\]\(\s*(<[^>]+>|[^\s)]+)|<img\b[^>]*?\bsrc\s*=\s*["']([^"']+)["']/gi)) {
      const target = (match[1] ?? match[2] ?? '').replace(/^<|>$/g, '');
      if (isLocalAsset(target) && /\.(?:png|jpe?g|gif|webp|heic|heif|avif|bmp|tiff?)(?:[?#]|$)/i.test(target)) return target;
    }
  }
}

// 笔记列表每次刷新、按标题排序都要取几百篇笔记的标题和首图。按笔记对象缓存，
// 正文没变就直接用上次的结果；笔记被替换成新对象或正文变了会重新解析。
type NoteMeta = { source: string; title: string; summary: string; image?: string | null; lower?: string };
const noteMeta = new WeakMap<object, NoteMeta>();
function metaOf(note: { source: string }) {
  let meta = noteMeta.get(note);
  if (meta?.source !== note.source) {
    const { title, summary } = noteText(note.source, '');
    meta = { source: note.source, title, summary };
    noteMeta.set(note, meta);
  }
  return meta;
}

export function noteTitle(note: { source: string; title?: string; filename?: string }) {
  return metaOf(note).title || (note.title ?? '未命名');
}

export function noteSummary(note: { source: string }) {
  return metaOf(note).summary;
}

/** 正文或标题里是否包含已经转成小写的搜索词。小写正文第一次搜索时生成，之后正文不变就一直复用。 */
export function noteMatches(note: { source: string; title?: string; filename?: string }, query: string) {
  const meta = metaOf(note);
  meta.lower ??= note.source.toLocaleLowerCase();
  return meta.lower.includes(query) || noteTitle(note).toLocaleLowerCase().includes(query);
}

export function noteFirstImage(note: { source: string }) {
  const meta = metaOf(note);
  if (meta.image === undefined) meta.image = firstLocalImage(note.source) ?? null;
  return meta.image ?? undefined;
}
