// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRichCodeRenderer } from '../src/rich-code';
import { preferences } from '../src/preferences';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });

let changeTheme: () => void;
let appearance: { matches: boolean; addEventListener: (_: string, callback: () => void) => void };
let libraries: { initialize: ReturnType<typeof vi.fn>; render: ReturnType<typeof vi.fn> };
let highlight: ReturnType<typeof vi.fn>;
beforeEach(() => {
  preferences.update({}, true);
  document.head.innerHTML = '';
  document.body.innerHTML = '';
  appearance = { matches: false, addEventListener: (_, callback) => { changeTheme = callback; } };
  vi.stubGlobal('matchMedia', () => appearance);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, text: async () => '' }));
  libraries = { initialize: vi.fn(), render: vi.fn().mockResolvedValue({ svg: '<svg id="result"></svg>' }) };
  highlight = vi.fn((code: HTMLElement) => { code.innerHTML = '<span class="hljs-keyword">let</span> x'; });
  Object.assign(window, { mermaid: libraries, hljs: { highlightElement: highlight }, ThemeConfig: { getMermaidConfig: (dark: boolean) => ({ dark }) }, adjustMermaidContrast: vi.fn(),
    markmap: { Transformer: class { transform() { return { root: { content: '<img src=x onerror="alert(1)">标题', children: [] } }; } }, autoLoader: { render: vi.fn((code: HTMLElement) => { code.innerHTML = '<svg><text>标题</text></svg>'; }) } },
    DiagramHandler: { processMarkmapElement: vi.fn(), markWhenSettled: vi.fn(), plantumlImageURL: vi.fn(() => 'https://www.plantuml.com/plantuml/svg/test') },
  });
  const append = document.head.append.bind(document.head);
  // CSS @scope is verified in real WebKit; jsdom does not implement it.
  vi.spyOn(document.head, 'append').mockImplementation((...nodes) => append(...nodes.filter(node => !(node instanceof HTMLStyleElement))));
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function block(language: string, source: string) {
  const root = document.createElement('div');
  const original = document.createElement('code');
  original.className = `language-${language}`;
  original.textContent = source;
  const raw = document.createElement('pre'); raw.className = 'vditor-wysiwyg__pre'; raw.append(original);
  const panel = document.createElement('pre'); panel.className = 'vditor-wysiwyg__preview';
  const rendered = document.createElement(['mermaid', 'markmap', 'plantuml'].includes(language) ? 'div' : 'code');
  rendered.className = original.className; rendered.textContent = source; panel.append(rendered);
  root.dataset.type = 'code-block'; root.append(raw, panel); document.body.append(root);
  return { root, original, rendered };
}

it('highlights only the display copy once with the bundled highlighter, even without dataset.highlighted', async () => {
  const { root, original, rendered } = block('swift', 'let x');
  const refresh = await createRichCodeRenderer(root);
  refresh(); refresh();
  await vi.waitFor(() => expect(highlight).toHaveBeenCalledOnce());
  expect(original.textContent).toBe('let x');
  expect(original.children).toHaveLength(0);
  expect(rendered.querySelector('.hljs-keyword')?.textContent).toBe('let');
});

it('keeps unintegrated SDK diagram types visible as code without changing their original language', async () => {
  const { root, original, rendered } = block('flowchart', 'A=>start: A');
  const placeholder = document.createElement('div');
  placeholder.className = 'language-flowchart'; rendered.replaceWith(placeholder);
  highlight.mockImplementation(() => {});
  const refresh = await createRichCodeRenderer(root); refresh();
  expect(root.querySelector('.vditor-wysiwyg__preview code')?.textContent).toBe('A=>start: A');
  expect(original.className).toBe('language-flowchart');
});

it('draws Markmap with the original renderer and sanitizes labels without rewriting source', async () => {
  const { root, original, rendered } = block('markmap', '# A\n## B');
  const refresh = await createRichCodeRenderer(root); refresh(); refresh();
  await vi.waitFor(() => expect(rendered.querySelector('svg')).not.toBeNull());
  expect(original.textContent).toBe('# A\n## B');
  const map = (window as unknown as { markmap: { Transformer: new () => { transform(): { root: { content: string } } } } }).markmap;
  expect(new map.Transformer().transform().root.content).not.toContain('onerror');
  preferences.update({ appearance: 'dark' });
  expect(rendered.parentElement?.classList.contains('darkmode')).toBe(true);
});

it('renders PlantUML as an image and keeps an editable code fallback on network failure', async () => {
  const { root, original, rendered } = block('plantuml', '@startuml\nA -> B\n@enduml');
  const refresh = await createRichCodeRenderer(root); refresh();
  const image = rendered.querySelector('img');
  expect(image?.src).toBe('https://www.plantuml.com/plantuml/svg/test');
  expect(rendered.textContent).toBe('');
  image?.dispatchEvent(new Event('error'));
  expect(rendered.textContent).toBe(original.textContent);
  expect(root.querySelector('object')).toBeNull();
});

it('discards obsolete diagram results and sanitizes the current SVG without touching editable source', async () => {
  let finish!: (value: { svg: string }) => void;
  libraries.render.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValueOnce({ svg: '<svg id="current"><script>alert(1)</script><foreignObject><div xmlns="http://www.w3.org/1999/xhtml" onclick="alert(1)">标签<a href="javascript:alert(1)"></a></div></foreignObject></svg>' });
  const { root, original, rendered } = block('mermaid', 'graph LR\nA-->B');
  const refresh = await createRichCodeRenderer(root); refresh();
  await vi.waitFor(() => expect(libraries.render).toHaveBeenCalledOnce());
  original.textContent = 'graph LR\nA-->C'; refresh();
  finish({ svg: '<svg id="obsolete"></svg>' });
  await vi.waitFor(() => expect(rendered.querySelector('#current')).not.toBeNull());
  expect(rendered.querySelector('#obsolete,script')).toBeNull();
  expect(rendered.querySelector('foreignObject')?.textContent).toBe('标签');
  expect(rendered.querySelector('[onclick],[href]')).toBeNull();
  expect(original.textContent).toBe('graph LR\nA-->C');
});

it('redraws diagrams with the current theme and retains source on syntax failure', async () => {
  const { root, original, rendered } = block('mermaid', 'graph LR\nA-->B');
  const refresh = await createRichCodeRenderer(root); refresh();
  await vi.waitFor(() => expect(rendered.dataset.miaoyanRendered).toBe('true'));
  preferences.update({ appearance: 'dark' });
  await vi.waitFor(() => expect(libraries.initialize).toHaveBeenLastCalledWith({ dark: true }));
  libraries.render.mockRejectedValueOnce(new Error('Invalid diagram'));
  original.textContent = 'graph LR\nA[invalid'; refresh();
  await vi.waitFor(() => expect(rendered.dataset.miaoyanRendered).toBe('failed'));
  expect(rendered.textContent).toBe(original.textContent);
  expect(rendered.parentElement?.classList.contains('mermaid-source-fallback')).toBe(true);
});
