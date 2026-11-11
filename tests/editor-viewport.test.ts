// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { revealCaret } from '../src/editor-viewport';

afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });
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
it('does not scroll an unfocused editor or a selection belonging to another field', () => {
  const root = editor(360, 380); root.blur(); revealCaret(root); expect(root.scrollTop).toBe(100);
  const other = editor(360, 380); root.focus();
  const foreign = document.createRange(); foreign.selectNodeContents(other.firstChild!); foreign.collapse(true);
  getSelection()!.removeAllRanges(); getSelection()!.addRange(foreign);
  revealCaret(root); expect(root.scrollTop).toBe(100);
  expect(other.scrollTop).toBe(100); revealCaret();
});
