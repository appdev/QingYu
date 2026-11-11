export function markdownBody(source: string): { body: string; offset: number } {
  const match = source.match(/^---\r?\n[\s\S]*?\r?\n(?:---|\.\.\.)(?:\r?\n|$)/);
  if (!match) return { body: source, offset: 0 };
  return { body: source.slice(match[0].length), offset: (match[0].match(/\n/g) ?? []).length };
}

function plainText(line: string) {
  return line.replace(/^\s*(?:#{1,6}\s+|>\s*|[-*+]\s+(?:\[[ xX]\]\s*)?|\d+[.)]\s+)/, '')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/<[^>]+>/g, '')
    .replace(/[*_`~]/g, '').trim();
}

export function noteText(source: string) {
  let title = '', summary = '';
  // Metadata only needs the first two meaningful lines, even for a large document.
  for (const [line] of markdownBody(source).body.matchAll(/[^\r\n]+/g)) {
    if (!line.trim() || /^\s*(?:[-=]{3,}|`{3,}.*|~{3,}.*|\$\$)\s*$/.test(line)) continue;
    const text = plainText(line);
    if (!text) continue;
    if (!title) title = text.slice(0, 160);
    else { summary = text.slice(0, 180); break; }
  }
  return { title: title || '未命名', summary: summary || '无更多文本' };
}
