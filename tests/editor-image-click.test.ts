// @vitest-environment jsdom
import { EditorView } from '@codemirror/view';
import { undo } from '@codemirror/commands';
import { expect, it, vi } from 'vitest';
import { createRichEditor } from '../src/rich-editor';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({
    matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }));
});

const source = '# 图片\n\n![日落](/sunset.webp)\n\n后文';
const imageFrom = source.indexOf('![');
const imageTo = source.indexOf(')') + 1;

function mount(mobile = false) {
  const html = document.documentElement;
  const wasMobile = html.hasAttribute('data-mobile');
  html.toggleAttribute('data-mobile', mobile);
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  editor.activate(source, mobile ? 'image-tap' : 'image-click');
  const image = () => root.querySelector<HTMLImageElement>('.cm-markra-image-line .cm-markra-image')!;
  const node = () => image().closest<HTMLElement>('.markra-image-node')!;
  const press = (type: string) => image().dispatchEvent(new MouseEvent(type, { button: 0, bubbles: true, cancelable: true }));
  const key = (name: string) => view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }));
  return {
    view, image, node, press, key,
    done() {
      document.querySelector('[role=dialog]')?.remove();
      view.destroy(); root.remove(); html.toggleAttribute('data-mobile', wasMobile);
    },
  };
}

it('selects the whole picture on a click, as Pinto does, without showing its Markdown', () => {
  const editor = mount();
  try {
    editor.press('mousedown'); editor.press('mouseup'); editor.press('click');
    const { from, to } = editor.view.state.selection.main;
    expect([from, to]).toEqual([imageFrom, imageTo]);
    expect(editor.node().classList.contains('markra-image-node-selected')).toBe(true);
    expect(editor.node().querySelector('input')).toBeNull();
    expect(editor.view.state.doc.toString()).toBe(source);

    // 光标移开后取消选中，图片也不会显示源码。
    editor.view.dispatch({ selection: { anchor: source.length } });
    expect(editor.node().classList.contains('markra-image-node-selected')).toBe(false);
    editor.view.dispatch({ selection: { anchor: imageFrom + 2 } });
    expect(editor.node().classList.contains('markra-image-node-selected')).toBe(false);
    expect(editor.node().querySelector('input')).toBeNull();
  } finally { editor.done(); }
});

it('continues below the picture on Return and removes it on Delete once it is selected', () => {
  const editor = mount();
  try {
    editor.press('mousedown');
    editor.key('Enter');
    expect(editor.view.state.doc.toString()).toBe(source);
    expect(editor.view.state.selection.main.head).toBe(imageTo + 1);

    editor.press('mousedown');
    editor.key('Backspace');
    expect(editor.view.state.doc.toString()).toBe(source.slice(0, imageFrom) + source.slice(imageTo));
    expect(undo(editor.view)).toBe(true);
    expect(editor.view.state.doc.toString()).toBe(source);
  } finally { editor.done(); }
});

it('opens the picture on a double click on the desktop and on a tap on the phone', () => {
  const desktop = mount();
  try {
    desktop.press('mousedown');
    desktop.image().dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    expect(document.querySelector('[role=dialog]')).not.toBeNull();
  } finally { desktop.done(); }

  const phone = mount(true);
  try {
    phone.press('mousedown'); phone.press('click');
    expect(document.querySelector('[role=dialog]')).not.toBeNull();
    expect(phone.view.state.selection.main.empty).toBe(true);
    expect(phone.view.state.doc.toString()).toBe(source);
  } finally { phone.done(); }
});

it('opens the selected picture with Space and closes it with Space again, as Quick Look does', () => {
  const editor = mount();
  try {
    // 只放了光标、没有选中图片时，空格照常输入。
    editor.view.dispatch({ selection: { anchor: imageTo } });
    editor.key(' ');
    expect(document.querySelector('[role=dialog]')).toBeNull();

    editor.press('mousedown');
    editor.key(' ');
    expect(document.querySelector('[role=dialog]')).not.toBeNull();
    expect(editor.view.state.doc.toString()).toBe(source);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true }));
    expect(document.querySelector('[role=dialog]')).toBeNull();
    expect(editor.view.state.selection.main.from).toBe(imageFrom);
    expect(editor.view.state.selection.main.to).toBe(imageTo);
  } finally { editor.done(); }
});
