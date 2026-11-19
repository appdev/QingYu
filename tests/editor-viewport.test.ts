// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { keepPanelTargetVisible, keepMobileViewportAtTop, revealCaret, showEditorInputPanel } from '../src/editor-viewport';

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); document.body.replaceChildren(); });

it('resets the page scroll from keyboard focus and preserves panning while zoomed', () => {
  const scrollTo = vi.fn();
  vi.stubGlobal('scrollTo', scrollTo);
  vi.stubGlobal('scrollY', 68);
  vi.stubGlobal('visualViewport', { scale: 1 });
  keepMobileViewportAtTop();
  expect(scrollTo).toHaveBeenCalledExactlyOnceWith(0, 0);
  vi.stubGlobal('scrollY', 0);
  keepMobileViewportAtTop();
  vi.stubGlobal('scrollY', 68);
  vi.stubGlobal('visualViewport', { scale: 2 });
  keepMobileViewportAtTop();
  expect(scrollTo).toHaveBeenCalledTimes(1);
});
function editor(top: number, bottom: number) {
  const root = document.createElement('div'); root.contentEditable = 'true'; root.tabIndex = 0;
  const p = document.createElement('p'); p.textContent = '中文光标'; root.append(p); document.body.append(root); root.focus();
  const range = document.createRange(); range.setStart(p.firstChild!, 2); range.collapse(true);
  getSelection()!.removeAllRanges(); getSelection()!.addRange(range);
  vi.spyOn(Range.prototype, 'cloneRange').mockReturnValue(range);
  Object.defineProperty(range, 'getClientRects', { value: () => [{ top, bottom, height: bottom - top }] });
  vi.spyOn(root, 'getBoundingClientRect').mockReturnValue({ top: 52, bottom: 350 } as DOMRect);
  root.scrollTop = 100;
  return root;
}
it('scrolls a caret above the keyboard and toolbar without changing focus or selection', () => {
  const root = editor(360, 380), offset = getSelection()!.anchorOffset;
  revealCaret(root);
  expect(root.scrollTop).toBe(162); expect(document.activeElement).toBe(root); expect(getSelection()!.anchorOffset).toBe(offset);
});
it('brings a caret below the titlebar into view and keeps a visible caret still', () => {
  const root = editor(40, 60); revealCaret(root); expect(root.scrollTop).toBe(56);
  const other = editor(100, 120); revealCaret(other); expect(other.scrollTop).toBe(100);
});
it('reveals a caret covered by the floating toolbar', () => {
  const root = editor(100, 120);
  root.style.scrollPaddingTop = '64px';
  revealCaret(root);
  expect(root.scrollTop).toBe(52);
});
it('does not scroll an unfocused editor or a selection belonging to another field', () => {
  const root = editor(360, 380); root.blur(); revealCaret(root); expect(root.scrollTop).toBe(100);
  const other = editor(360, 380); root.focus();
  const foreign = document.createRange(); foreign.selectNodeContents(other.firstChild!); foreign.collapse(true);
  getSelection()!.removeAllRanges(); getSelection()!.addRange(foreign);
  revealCaret(root); expect(root.scrollTop).toBe(100);
  expect(other.scrollTop).toBe(100); revealCaret();
});

it('makes room for the format panel and reveals its target without taking focus', () => {
  const workspace = document.createElement('div');
  const scroller = workspace.appendChild(document.createElement('div'));
  const panel = document.body.appendChild(document.createElement('dialog'));
  panel.tabIndex = 0;
  panel.focus();
  let panelTop = 350;
  let frame: FrameRequestCallback;
  let resize: ResizeObserverCallback;
  const disconnect = vi.fn();
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => { frame = callback; return 1; });
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe() {}
    disconnect = disconnect;
  });
  vi.spyOn(workspace, 'getBoundingClientRect').mockReturnValue({ bottom: 600 } as DOMRect);
  vi.spyOn(panel, 'getBoundingClientRect').mockImplementation(() => ({ top: panelTop }) as DOMRect);
  vi.spyOn(scroller, 'getBoundingClientRect').mockImplementation(() => ({
    top: 52, bottom: 600 - parseFloat(workspace.style.paddingBottom || '0'),
  }) as DOMRect);
  const layout = keepPanelTargetVisible(workspace, scroller, panel, () => ({
    top: 405 - scroller.scrollTop, bottom: 445 - scroller.scrollTop,
  }));
  frame!(0);
  expect(workspace.style.paddingBottom).toBe('250px');
  expect(scroller.scrollTop).toBe(103);
  expect(document.activeElement).toBe(panel);
  panelTop = 400;
  resize!([], {} as ResizeObserver);
  frame!(0);
  expect(workspace.style.paddingBottom).toBe('200px');
  expect(scroller.scrollTop).toBe(103);
  layout.stop();
  expect(workspace.style.paddingBottom).toBe('');
  expect(disconnect).toHaveBeenCalledOnce();
});

it('keeps the editor interactive while the panel owns the soft-keyboard space', () => {
  const input = editor(100, 120);
  input.setAttribute('inputmode', 'text');
  const dialog = document.createElement('dialog');
  const panel = showEditorInputPanel(dialog, input);
  expect(dialog.open).toBe(true);
  expect(document.activeElement).toBe(input);
  expect(input.getAttribute('inputmode')).toBe('none');
  const replacement = editor(100, 120);
  panel.useInput(replacement);
  expect(input.getAttribute('inputmode')).toBe('text');
  expect(replacement.getAttribute('inputmode')).toBe('none');
  panel.close();
  expect(dialog.isConnected).toBe(false);
  expect(replacement.getAttribute('inputmode')).toBeNull();
  expect(document.activeElement).toBe(document.body);
});
