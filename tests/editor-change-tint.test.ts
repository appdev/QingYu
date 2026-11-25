// @vitest-environment jsdom
import { EditorView } from '@codemirror/view';
import { expect, it, vi } from 'vitest';
import { changedLines } from '../src/editor-change-tint';
import { createRichEditor } from '../src/rich-editor';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({
    matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }));
});

function mount(source: string) {
  const root = document.body.appendChild(document.createElement('div'));
  const saved: string[] = [];
  const editor = createRichEditor(root, () => { saved.push(view.state.doc.toString()); });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  editor.activate(source, 'tint');
  return { root, editor, view, saved, done() { view.destroy(); root.remove(); } };
}

it('finds the lines a change added, leaving blank and untouched lines out', () => {
  expect(changedLines('# 购物\n\n- [ ] 牛奶\n- [ ] 柠檬', '# 购物\n\n- [ ] 藏红花\n- [ ] 牛奶\n\n- [ ] 辣肠\n- [ ] 柠檬')).toEqual([3, 6]);
  expect(changedLines('一\n二', '一\n二')).toEqual([]);
  expect(changedLines('一\n二\n三', '一\n三')).toEqual([]);
});

it('tints outside changes until the next edit and restores the old text from the receipt', () => {
  const before = '# 购物\n\n- [ ] 牛奶\n- [ ] 柠檬';
  const after = '# 购物\n\n- [ ] 藏红花\n- [ ] 牛奶\n- [ ] 柠檬';
  const { root, editor, view, saved, done } = mount(before);
  try {
    // 外部改动经由 activate 载入，然后标出改过的行。
    editor.activate(after, 'tint');
    expect(editor.markChanges(before)).toBe(1);
    const lines = [...root.querySelectorAll('.cm-change-line')];
    expect(lines.map(line => line.textContent)).toEqual(['藏红花']);
    // 底色只铺在文字后面，不盖住圆圈。
    expect(root.querySelector('.cm-change-tint')?.textContent).toBe('藏红花');
    expect(root.querySelector('.cm-change-tint .cm-markra-task-checkbox')).toBeNull();
    const receipt = root.querySelector<HTMLElement>('.editor-change-receipt')!;
    expect(receipt.querySelector('[role=status]')?.textContent).toBe('外部修改了 1 行，已自动重载');

    receipt.querySelector('button')!.click();
    expect(view.state.doc.toString()).toBe(before);
    expect(saved.at(-1)).toBe(before);
    expect(root.querySelector('.cm-change-line, .editor-change-receipt')).toBeNull();

    editor.activate(after, 'tint');
    editor.markChanges(before);
    view.dispatch({ changes: { from: view.state.doc.length, insert: '！' }, userEvent: 'input.type' });
    expect(root.querySelector('.cm-change-line, .editor-change-receipt')).toBeNull();

    // 换到别的笔记后不留标记。
    editor.activate(after, 'tint');
    editor.markChanges(before);
    editor.activate('另一篇', 'other');
    expect(root.querySelector('.cm-change-line, .editor-change-receipt')).toBeNull();
  } finally { done(); }
});

it('pops the circle when a checklist item is ticked and celebrates a finished list', async () => {
  const animate = vi.fn(() => ({ cancel() {} }));
  const original = Object.getOwnPropertyDescriptor(Element.prototype, 'animate');
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, value: animate });
  const { root, done } = mount('- [x] 面包\n- [ ] 鸡蛋\n\n- [ ] 另一张清单');
  try {
    root.querySelectorAll<HTMLInputElement>('.cm-markra-task-checkbox')[2].click();
    await Promise.resolve();
    // 单独一项的清单只弹一下：圆圈和对勾各一段动画。
    expect(animate.mock.calls.map(call => (call as unknown[])[1])).toEqual([
      { duration: 320 }, { pseudoElement: '::before', duration: 200, delay: 60, easing: 'ease-out', fill: 'backwards' },
    ]);
    expect(document.querySelector('.check-burst')).toBeNull();

    animate.mockClear();
    root.querySelectorAll<HTMLInputElement>('.cm-markra-task-checkbox')[1].click();
    await Promise.resolve();
    // 整张清单勾完：另一个圆圈跳起，并撒出十个小点。
    expect(animate.mock.calls.some(call => ((call as unknown[])[1] as { duration: number; delay: number }).duration === 340)).toBe(true);
    expect(document.querySelectorAll('.check-burst i')).toHaveLength(10);
  } finally {
    done();
    document.querySelector('.check-burst')?.remove();
    if (original) Object.defineProperty(Element.prototype, 'animate', original);
    else Reflect.deleteProperty(Element.prototype, 'animate');
  }
});
