import DOMPurify from 'dompurify';
import { isDark } from './preferences';

type MapNode = { content?: string; children?: MapNode[] };
type Libraries = {
  hljs: { highlightElement(element: HTMLElement): void };
  mermaid: { initialize(config: object): void; render(id: string, source: string): Promise<{ svg: string }> };
  ThemeConfig: { getMermaidConfig(dark: boolean): object };
  adjustMermaidContrast(root: Element, dark: boolean): void;
  markmap: {
    Transformer: { prototype: { transform(source: string): { root: MapNode } } };
    autoLoader: { render(element: HTMLElement): void };
  };
  DiagramHandler: {
    processMarkmapElement(element: HTMLElement): void;
    markWhenSettled(element: HTMLElement, svg: SVGElement): void;
    plantumlImageURL(element: HTMLElement, dark: boolean): string;
  };
};

export async function createRichCodeRenderer(root: HTMLElement) {
  const libraries = window as typeof window & Libraries;
  const { mermaid, hljs } = libraries;
  const appearance = { get matches() { return isDark(); } };
  const sheets = await Promise.all(['typography', 'theme-light', 'theme-dark', 'base', 'diagrams', 'preview'].map(async name => {
    const response = await fetch(name === 'preview' ? '/preview.css' : `/miaoyan/css/${name}.css`);
    if (!response.ok) throw new Error('代码渲染样式加载失败');
    return response.text();
  }));
  // Original styles stay inside read-only blocks; they must not recolor the editor UI.
  const style = document.createElement('style');
  style.textContent = `@scope (#rich-editor [data-type="code-block"]) to (.vditor-wysiwyg__pre) { ${sheets.join('\n').replaceAll(':root', ':scope')} }`;
  document.head.append(style);
  // Markdown inside a map is user input too; sanitize labels before d3 inserts HTML.
  const transform = libraries.markmap.Transformer.prototype.transform;
  libraries.markmap.Transformer.prototype.transform = function (source) {
    const result = transform.call(this, source);
    const nodes = [result.root];
    while (nodes.length) {
      const node = nodes.pop()!;
      if (node.content) node.content = DOMPurify.sanitize(node.content);
      nodes.push(...node.children ?? []);
    }
    return result;
  };
  // Vditor has no override for its built-in Mermaid branch. Keep its source/preview
  // interaction, but let the renderer below own drawing, themes and stale requests.
  libraries.mermaid = {
    initialize() {},
    async render(_id, source) {
      const text = document.createElement('div');
      text.textContent = source;
      return { svg: text.innerHTML };
    },
  };
  // We provide the original palette rather than loading Vditor's default code theme.
  // shortcut: This ID/href guard belongs to Vditor 4.0.1; recheck it on an SDK upgrade.
  const theme = document.createElement('meta');
  theme.id = 'vditorHljsStyle';
  theme.setAttribute('href', '/vditor/dist/js/highlight.js/styles/github.min.css');
  document.head.append(theme);
  const requests = new WeakMap<HTMLElement, string>();
  let queue = Promise.resolve();
  let nextId = 0;
  function refresh() {
    document.documentElement.classList.toggle('darkmode', appearance.matches);
    for (const panel of root.querySelectorAll<HTMLElement>('[data-type="code-block"] > .vditor-wysiwyg__preview')) {
      panel.classList.add('heti');
      panel.classList.toggle('darkmode', appearance.matches);
      const original = panel.previousElementSibling?.querySelector('code');
      if (!original) continue;
      const code = panel.querySelector<HTMLElement>(':scope > .language-mermaid, :scope > .language-markmap, :scope > .language-plantuml, :scope > code') ?? document.createElement('code');
      if (!code.parentElement) {
        panel.replaceChildren(code);
      }
      const source = original.textContent ?? '';
      if (original.classList.contains('language-markmap')) {
        // Mark processed before the SDK's deferred renderer runs; source stays in its own pre.
        code.dataset.processed = 'true';
        const key = `markmap:${appearance.matches}:${source}`;
        if (requests.get(code) === key) continue;
        requests.set(code, key);
        delete code.dataset.markmapShown;
        code.textContent = source;
        code.className = 'language-markmap markmap';
        libraries.DiagramHandler.processMarkmapElement(code);
        const typography = getComputedStyle(code);
        for (const property of ['font-family', 'font-size', 'line-height']) {
          document.documentElement.style.setProperty(`--rich-map-${property}`, typography.getPropertyValue(property));
        }
        libraries.markmap.autoLoader.render(code);
        const svg = code.querySelector<SVGElement>('svg');
        code.dataset.markmapRendered = svg ? 'true' : 'failed';
        code.style.visibility = 'visible';
        // The original loader schedules its first fit at 100ms, then checks transitions at 150ms.
        if (svg) setTimeout(() => { if (svg.isConnected) libraries.DiagramHandler.markWhenSettled(code, svg); }, 150);
        continue;
      }
      if (original.classList.contains('language-plantuml')) {
        const key = `plantuml:${appearance.matches}:${source}`;
        if (requests.get(code) === key) continue;
        requests.set(code, key);
        delete code.dataset.miaoyanRendered;
        code.dataset.originalContent = source;
        const image = document.createElement('img');
        image.className = 'plantuml-image';
        image.alt = 'PlantUML 图表';
        image.onload = () => { if (requests.get(code) === key) code.dataset.miaoyanRendered = 'true'; };
        image.onerror = () => {
          if (requests.get(code) !== key || !code.isConnected) return;
          code.textContent = source;
          code.dataset.miaoyanRendered = 'failed';
        };
        // An image leaves no display text for the SDK's object renderer to consume.
        code.replaceChildren(image);
        image.src = libraries.DiagramHandler.plantumlImageURL(code, appearance.matches);
        continue;
      }
      if (!original.classList.contains('language-mermaid')) {
        const key = `${original.className}:${original.textContent}`;
        if (requests.get(code) !== key) {
          requests.set(code, key);
          code.textContent = original.textContent;
          code.className = original.className;
          hljs.highlightElement(code);
        }
        continue;
      }
      const dark = appearance.matches;
      const key = `${dark}:${source}`;
      if (requests.get(code) === key && (code.dataset.miaoyanRendered !== 'true' || code.querySelector('svg'))) continue;
      requests.set(code, key);
      delete code.dataset.miaoyanRendered;
      queue = queue.then(async () => {
        if (!code.isConnected || requests.get(code) !== key) return;
        let svg: string | undefined;
        try {
          mermaid.initialize(libraries.ThemeConfig.getMermaidConfig(dark));
          svg = (await mermaid.render(`rich-mermaid-${++nextId}`, source)).svg;
        } catch {
          // Invalid diagrams keep their editable source, like the normal preview.
        }
        if (!code.isConnected || requests.get(code) !== key) return;
        if (svg) code.innerHTML = DOMPurify.sanitize(svg, { ADD_TAGS: ['foreignObject'], HTML_INTEGRATION_POINTS: { foreignobject: true } });
        else code.textContent = source;
        code.dataset.miaoyanRendered = svg ? 'true' : 'failed';
        panel.classList.toggle('mermaid-source-fallback', !svg);
        libraries.adjustMermaidContrast(panel, dark);
      });
    }
  }
  new MutationObserver(refresh).observe(root, { childList: true, subtree: true });
  window.addEventListener('preferences-applied', refresh);
  return refresh;
}
