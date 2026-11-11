import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';
import { expect, it, vi } from 'vitest';

function loadPreview(win) {
  win.eval(readFileSync(new URL('../public/mermaid-contrast.js', import.meta.url), 'utf8'));
}

it('keeps custom Mermaid fills and fixes unreadable dark labels after each render', async () => {
  const dom = new JSDOM('<article id="write"><code class="language-mermaid"></code></article>', { url: 'http://localhost', runScripts: 'outside-only' });
  const win = dom.window;
  const node = (fill, color, border) => `<g class="node"><rect style="fill:${fill};stroke:${border};fill-opacity:1;opacity:1"></rect><foreignObject><div xmlns="http://www.w3.org/1999/xhtml"><span class="nodeLabel" style="color:${color}"><p style="color:${color}">节点</p></span></div></foreignObject></g>`;
  const html = `<svg>${node('rgb(225, 245, 254)', 'rgb(214, 209, 198)', 'rgb(231, 233, 234)')}${node('rgb(47, 46, 43)', 'rgb(231, 233, 234)', 'rgb(231, 233, 234)')}${node('rgb(156, 39, 176)', 'rgb(231, 233, 234)', 'rgb(231, 233, 234)')}</svg>`;
  Object.assign(win, {
    matchMedia: () => ({ matches: true, addEventListener() {} }),
    ResizeObserver: class { observe() {} },
    ThemeConfig: { applyCSSVariables() {} },
    MiaoYanCommon: { setupTextSelection() {} },
    DiagramHandler: { initializeMermaid: vi.fn(async () => { win.document.querySelector('code').innerHTML = html; }) },
  });
  loadPreview(win);
  for (let render = 0; render < 2; render++) {
    await win.DiagramHandler.initializeMermaid();
    win.adjustMermaidContrast(win.document.getElementById('write'), true);
    const nodes = win.document.querySelectorAll('.node');
    expect(nodes[0].querySelector('rect').style.fill).toBe('rgb(225, 245, 254)');
    expect(win.getComputedStyle(nodes[0].querySelector('p')).color).toBe('rgb(0, 0, 0)');
    expect(nodes[0].querySelector('rect').style.stroke).toBe('#000000');
    expect(win.getComputedStyle(nodes[1].querySelector('p')).color).toBe('rgb(231, 233, 234)');
    expect(win.getComputedStyle(nodes[2].querySelector('p')).color).toBe('rgb(231, 233, 234)');
  }
  win.document.documentElement.classList.remove('darkmode');
  await win.DiagramHandler.initializeMermaid();
  win.adjustMermaidContrast(win.document.getElementById('write'), false);
  expect(win.getComputedStyle(win.document.querySelector('.node p')).color).toBe('rgb(214, 209, 198)');
  dom.window.close();
});

