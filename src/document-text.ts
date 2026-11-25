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

export function noteTitle(note: { source: string; title?: string; filename?: string }) {
  return noteText(note.source, note.title).title;
}
