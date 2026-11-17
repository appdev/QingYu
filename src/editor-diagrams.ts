import DOMPurify from 'dompurify';
import { renderMermaidToSvg } from './vendor/markra/editor/mermaid';
import type { CodeBlockMermaidContext } from './vendor/markra/editor/codemirror/code-block';

type MapNode = { content?: string; children?: MapNode[] };
type DiagramAssets = typeof window & {
  plantumlEncoder: { encode(source: string): string };
  markmap: {
    Transformer: new () => { transform(source: string): { root: MapNode } };
    Markmap: { create(svg: SVGElement, options: object, root: MapNode): { fit(): Promise<void> } };
  };
};
const loads = new Map<string, Promise<void>>();
function load(file: string) {
  const existing = loads.get(file);
  if (existing) return existing;
  const ready = new Promise<void>((resolve, reject) => {
    const script = document.createElement('script');
    script.src = `/miaoyan/js/${file}`;
    script.onload = () => resolve();
    script.onerror = () => { script.remove(); loads.delete(file); reject(new Error('无法加载图表资源')); };
    document.head.append(script);
  });
  loads.set(file, ready);
  return ready;
}

export async function renderEditorDiagram({ source, language, theme, view }: CodeBlockMermaidContext) {
  const assets = window as DiagramAssets;
  const kind = language.toLowerCase();
  if (kind === 'plantuml') {
    await load('plantuml-encoder.min.js');
    const result = document.createElement('div'), image = document.createElement('img');
    image.alt = 'PlantUML 图表';
    image.onload = () => view.requestMeasure();
    image.onerror = () => { result.textContent = source; view.requestMeasure(); };
    image.src = `https://www.plantuml.com/plantuml/svg/~1${assets.plantumlEncoder.encode(source)}`;
    result.append(image);
    return result;
  }
  if (kind === 'markmap') {
    await load('d3.min.js');
    await load('markmap-view.min.js');
    const { Transformer, Markmap } = assets.markmap;
    const { root } = new Transformer().transform(source);
    const nodes = [root];
    while (nodes.length) {
      const node = nodes.pop()!;
      if (node.content) node.content = DOMPurify.sanitize(node.content);
      nodes.push(...node.children ?? []);
    }
    const result = document.createElement('div'), svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.style.width = '100%'; svg.style.height = '320px';
    result.append(svg);
    requestAnimationFrame(() => {
      if (!result.isConnected) return;
      const fallback = () => { result.textContent = source; view.requestMeasure(); };
      try {
        const map = Markmap.create(svg, { duration: 0, color: () => getComputedStyle(view.dom).getPropertyValue('--brand').trim() }, root);
        void map.fit().catch(fallback);
      } catch { fallback(); }
    });
    return result;
  }
  return renderMermaidToSvg(source, { idPrefix: 'qingyu-mermaid', theme });
}
