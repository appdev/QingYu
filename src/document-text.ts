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

export function noteText(source: string) {
  let title = '', summary = '';
  // Metadata only needs the first two meaningful lines, even for a large document.
  for (const [line] of markdownBody(source).body.matchAll(/[^\r\n]+/g)) {
    if (!line.trim() || /^\s*(?:[-=]{3,}|`{3,}.*|~{3,}.*|\$\$)\s*$/.test(line)) continue;
    const text = plainMarkdownLine(line);
    if (!text) continue;
    if (!title) title = text.slice(0, 160);
    else { summary = text.slice(0, 180); break; }
  }
  return { title: title || '未命名', summary: summary || '无更多文本' };
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

export function noteTitle(note: { source: string; title?: string }) {
  return note.title ?? noteText(note.source).title;
}
