import MarkdownIt from 'markdown-it';
import reference from 'markdown-it/lib/rules_block/reference.mjs';

type Definition = { label: string; source: string; carets: number[]; start: number; end: number };
const parser = new MarkdownIt({ html: true });
// This parser only retains literal definition text; Lute still sanitizes all rendered/opened links.
parser.validateLink = () => true;
parser.block.ruler.at('reference', (state, start, end, silent) => {
  if (!reference(state, start, end, silent)) return false;
  if (!silent) {
    const source = state.getLines(start, state.line, state.blkIndent, false);
    const label = source.trimStart().match(/^\[((?:\\.|[^\]\\])*)\]:/s)?.[1];
    if (label !== undefined) {
      const offset = state.bMarks[start];
      // getLines excludes the final newline, so a caret at source.length is still inside this definition.
      const carets = (state.env.carets as number[]).filter(pos => pos >= offset && pos <= offset + source.length).map(pos => pos - offset);
      (state.env.definitions as Definition[]).push({ label: parser.utils.normalizeReference(label), source, carets, start: offset, end: state.bMarks[state.line] ?? state.src.length });
    }
  }
  return true;
});

function definitions(source: string, carets: number[] = []) {
  const env = { definitions: [] as Definition[], carets };
  parser.parse(source, env);
  return env.definitions;
}

export function markdownDefinitions(source: string) {
  if (!source.includes(']:')) return [];
  return definitions(source.replace(/\r\n?/g, '\n'));
}

export function domDefinitions(html: string) {
  if (!html.includes('link-ref-defs-block')) return [];
  const template = document.createElement('template');
  template.innerHTML = html;
  return [...template.content.querySelectorAll('[data-type="link-ref-defs-block"]')].flatMap(block => {
    let source = '';
    const carets: number[] = [];
    const walk = (node: Node) => {
      if (node.nodeType === Node.TEXT_NODE) source += node.textContent;
      else if (node instanceof Element && node.tagName === 'WBR') carets.push(source.length);
      else if (node instanceof Element && node.tagName === 'BR') source += '\n';
      else node.childNodes.forEach(walk);
    };
    walk(block);
    return definitions(source, carets);
  });
}

export function withoutDefinitionCarets(html: string) {
  if (!html.includes('link-ref-defs-block') || !html.includes('<wbr')) return html;
  const template = document.createElement('template');
  template.innerHTML = html;
  for (const block of template.content.querySelectorAll('[data-type="link-ref-defs-block"]')) {
    // Lute treats a caret immediately after <destination> as invalid trailing text.
    if (domDefinitions(block.outerHTML).some(definition => definition.carets.length)) block.querySelectorAll('wbr').forEach(caret => caret.remove());
  }
  return template.innerHTML;
}

export function restoreDefinitions(html: string, originals: Definition[]) {
  if (!originals.length || !html.includes('link-ref-defs-block')) return html;
  const template = document.createElement('template');
  template.innerHTML = html;
  const available = [...originals];
  for (const block of template.content.querySelectorAll('[data-type="link-ref-defs-block"]')) {
    const text = block.textContent ?? '';
    const current = definitions(text);
    if (!current.length) continue;
    block.replaceChildren();
    let position = 0;
    for (const normalized of current) {
      block.append(text.slice(position, normalized.start));
      const index = available.findIndex(definition => definition.label === normalized.label);
      const original = index < 0 ? normalized : available.splice(index, 1)[0];
      let offset = 0;
      for (const caret of original.carets) {
        block.append(original.source.slice(offset, caret), document.createElement('wbr'));
        offset = caret;
      }
      block.append(original.source.slice(offset));
      if (!original.source.endsWith('\n')) block.append('\n');
      position = normalized.end;
    }
    block.append(text.slice(position));
  }
  return template.innerHTML;
}
