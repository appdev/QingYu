import './theme.css';
import './preview.css';
import { createRichEditor } from './rich-editor';
import { markdownBody } from './document-text';
import { preferences, sizes } from './preferences';
import { previewPositions } from './preview-source';

const parentElement = document.querySelector<HTMLElement>('#rich-editor')!;
const send = (type: string, extra = {}) => parent.postMessage({ type, ...extra }, location.origin);
let scrollingUntil = 0;
let editor: Awaited<ReturnType<typeof createRichEditor>>;
let pending: { id: number; source: string; noteId?: string; assetBase?: string } | undefined;
let received = -1;
let anchors: { line: number; top: number }[] = [];

function rebuild() {
  if (!editor) return;
  const { element } = editor;
  const top = element.getBoundingClientRect().top;
  anchors = [...element.querySelectorAll<HTMLElement>('[data-source-line]')].filter(node => node.getClientRects().length)
    .map(node => ({ line: Number(node.dataset.sourceLine), top: node.getBoundingClientRect().top - top + element.scrollTop }))
    .sort((a, b) => a.line - b.line);
}
function scrollToLine(line: number) {
  rebuild();
  const first = anchors[0];
  if (!editor || !first) return;
  scrollingUntil = performance.now() + 160;
  let top = line <= first.line ? 0 : first.top;
  for (let i = 1; i < anchors.length && line > first.line; i++) {
    const before = anchors[i - 1], after = anchors[i];
    if (after.line > line) { top = before.top + (line - before.line) / Math.max(1, after.line - before.line) * (after.top - before.top); break; }
    top = after.top;
  }
  editor.element.scrollTop = top;
}
function render() {
  if (!editor || !pending) return;
  const { id, source, noteId, assetBase } = pending;
  pending = undefined;
  try {
    const top = editor.element.scrollTop;
    editor.activate(source, String(id), noteId, assetBase);
    previewPositions(editor.element, markdownBody(source).body, editor.lute);
    editor.element.scrollTop = top;
    rebuild();
    send('preview-rendered', { id });
  } catch (error) { console.error('Preview rendering failed', error); send('preview-error'); }
}

window.addEventListener('message', ({ source, origin, data }) => {
  if (source !== parent || origin !== location.origin || !data) return;
  if (data.type === 'render' && typeof data.source === 'string' && Number.isInteger(data.id) && data.id >= received) {
    received = data.id; pending = { id: data.id, source: data.source, noteId: typeof data.noteId === 'string' ? data.noteId : undefined, assetBase: typeof data.assetBase === 'string' && /^(qingyu-asset:|https?:\/\/qingyu-asset\.localhost\/)/.test(data.assetBase) ? data.assetBase : undefined }; render();
  } else if (data.type === 'scroll' && Number.isFinite(data.line)) scrollToLine(data.line);
  else if (data.type === 'preferences') {
    if (typeof data.dark !== 'boolean' || typeof data.font !== 'string' || typeof data.codeFont !== 'string' || !sizes.includes(data.size) || ![0,600,800,1000,1200,1400].includes(data.width)) return;
    // Native settings live in the parent WebView; receive their resolved appearance and typography.
    preferences.receive({ ...preferences.value, appearance: data.dark ? 'dark' : 'light' });
    const style = document.documentElement.style;
    style.setProperty('--preview-font', data.font); style.setProperty('--code-font', data.codeFont);
    style.setProperty('--preview-size', `${data.size}px`); style.setProperty('--preview-width', data.width ? `${data.width}px` : '100%');
    rebuild();
  }
});
try {
  editor = await createRichEditor(parentElement, () => {}, { readOnly: true, openAsset(noteId, path) { send('open-asset', { noteId, path }); }, onTask(input) {
    const line = Number(input.dataset.sourceLine);
    if (Number.isInteger(line) && line > 0) send('task', { line });
  } });
  new ResizeObserver(rebuild).observe(editor.element);
  editor.element.addEventListener('scroll', () => {
    rebuild();
    if (performance.now() <= scrollingUntil || !anchors.length) return;
    const y = editor.element.scrollTop;
    let before = anchors[0], line = before.line;
    for (const after of anchors.slice(1)) {
      if (after.top > y) { line = before.line + Math.max(0, y - before.top) / Math.max(1, after.top - before.top) * (after.line - before.line); break; }
      before = after; line = before.line;
    }
    send('preview-scroll', { line });
  }, { passive: true });
  render(); send('preview-ready');
} catch (error) {
  parentElement.textContent = '预览加载失败，原文已保留';
  console.error(error); send('preview-error');
}
