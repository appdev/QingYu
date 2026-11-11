import { parser, GFM } from '@lezer/markdown';
import type Vditor from 'vditor';
import { markdownDefinitions } from './rich-references';

const syntax = parser.configure(GFM);
const previous = new WeakMap<object, Map<string, string[]>>();

function key(element: Element) {
  const copy = element.cloneNode(true) as Element;
  copy.querySelectorAll('.vditor-wysiwyg__preview, wbr').forEach(node => node.remove());
  if (copy.tagName === 'LI') copy.querySelectorAll('ul,ol').forEach(node => node.remove());
  return `${copy.tagName}:${copy.getAttribute('data-type') ?? ''}:${copy.textContent?.replace(/[\s\u200b\ufeff]/g, '')}:${copy.querySelector('input')?.hasAttribute('checked') ?? ''}`;
}

// Lezer supplies source ranges only; all display HTML still comes from the same Lute adapter.
export function previewPositions(root: HTMLElement, source: string, lute: Vditor['vditor']['lute']) {
  root.querySelectorAll<HTMLElement>('[data-source-line]').forEach(node => delete node.dataset.sourceLine);
  const tree = syntax.parse(source);
  const definitions = markdownDefinitions(source).map(item => item.source).join('\n');
  const cached = previous.get(lute), current = new Map<string, string[]>();
  const render = (text: string) => {
    const source = text + '\n\n' + definitions;
    const kept = current.get(source) ?? cached?.get(source);
    if (kept) { current.set(source, kept); return kept; }
    const template = document.createElement('template');
    template.innerHTML = lute.Md2VditorDOM(source);
    const keys = [...template.content.children].filter(element => !element.matches('[data-type="link-ref-defs-block"], [data-type="footnotes-block"]')).map(key);
    current.set(source, keys);
    return keys;
  };
  const line = (offset: number) => 1 + (source.slice(0, offset).match(/\n/g)?.length ?? 0);
  const blocks = new Map<string, number[]>(), tasks = new Map<string, number[]>();
  const add = (map: Map<string, number[]>, id: string, position: number) => {
    const values = map.get(id) ?? [];
    values.push(position); map.set(id, values);
  };
  for (let node = tree.topNode.firstChild; node; node = node.nextSibling) {
    if (node.name === 'LinkReference') continue;
    for (const id of render(source.slice(node.from, node.to))) add(blocks, id, line(node.from));
  }
  tree.iterate({ enter(node) {
    if (node.name !== 'Task') return;
    const fragment = document.createElement('template');
    fragment.innerHTML = lute.Md2VditorDOM('- ' + source.slice(node.from, node.to) + '\n\n' + definitions);
    const item = fragment.content.querySelector('li');
    if (item) add(tasks, key(item), line(node.from));
  } });
  previous.set(lute, current);
  for (const element of root.children) {
    const position = blocks.get(key(element))?.shift();
    if (position) (element as HTMLElement).dataset.sourceLine = String(position);
  }
  for (const input of root.querySelectorAll<HTMLInputElement>('li.vditor-task > input')) {
    const position = tasks.get(key(input.closest('li')!))?.shift();
    // Never toggle a guessed source line when an SDK extension cannot be matched.
    input.disabled = !position;
    if (position) input.dataset.sourceLine = String(position);
  }
}
