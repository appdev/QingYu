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

it('names the picture, zooms on a double click and closes on a click beside it', () => {
  const editor = mount();
  try {
    editor.image().dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    const dialog = document.querySelector<HTMLElement>('[role=dialog]')!;
    expect(dialog.querySelector('.markra-media-viewer-title')?.textContent).toBe('日落');
    expect([...dialog.querySelectorAll('button')].map(button => button.ariaLabel)).toEqual(['缩小', '放大', '还原大小', '全屏', '关闭']);
    const zoom = () => dialog.querySelector('.markra-media-viewer-zoom-value')!.textContent;
    const picture = dialog.querySelector('.markra-media-viewer-media')!;
    // jsdom 没有布局，给图片一个位置，双击坐标落在它里面。
    picture.getBoundingClientRect = () => ({ left: 0, top: 0, right: 100, bottom: 100, width: 100, height: 100, x: 0, y: 0, toJSON() {} });
    const content = dialog.querySelector<HTMLElement>('.markra-media-viewer-content')!;
    // 双击事件可能以内容区为目标（指针捕获），只看坐标。
    content.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, clientX: 50, clientY: 50 }));
    expect(zoom()).toBe('200%');
    content.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, clientX: 50, clientY: 50 }));
    expect(zoom()).toBe('100%');
    content.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
    expect(zoom()).toBe('100%');

    // 在图片上按下松开不关闭，在旁边空白处点一下关闭。
    const tap = (target: Element) => {
      target.dispatchEvent(Object.assign(new MouseEvent('pointerdown', { button: 0, bubbles: true, clientX: 5, clientY: 5 }), { pointerId: 1 }));
      content.dispatchEvent(Object.assign(new MouseEvent('pointerup', { button: 0, bubbles: true, clientX: 5, clientY: 5 }), { pointerId: 1 }));
    };
    tap(picture);
    expect(document.querySelector('[role=dialog]')).not.toBeNull();
    tap(content);
    expect(document.querySelector('[role=dialog]')).toBeNull();
  } finally { editor.done(); }
});

it('steps through every picture in the note with the arrows and the buttons', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const many = '# 相册\n\n![第一张](/a.webp)\n\n文字\n\n![第二张|small](/b.webp)\n\n![第三张](/c.webp)';
  try {
    editor.activate(many, 'gallery');
    const second = [...root.querySelectorAll<HTMLImageElement>('.cm-markra-image')].find(image => image.alt === '第二张')!;
    second.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    const dialog = document.querySelector<HTMLElement>('[role=dialog]')!;
    const counter = () => dialog.querySelector('.markra-media-viewer-counter')!.textContent;
    const title = () => dialog.querySelector('.markra-media-viewer-title')!.textContent;
    const button = (label: string) => dialog.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
    expect([counter(), title()]).toEqual(['2 / 3', '第二张']);
    const arrow = (key: string) => document.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    arrow('ArrowRight');
    expect([counter(), title()]).toEqual(['3 / 3', '第三张']);
    expect(dialog.querySelector('.markra-media-viewer-media')!.getAttribute('src')).toBe('/c.webp');
    expect(button('下一张').disabled).toBe(true);
    arrow('ArrowRight');
    expect(counter()).toBe('3 / 3');
    button('上一张').click(); button('上一张').click();
    expect([counter(), title()]).toEqual(['1 / 3', '第一张']);
    expect(button('上一张').disabled).toBe(true);
    expect(view.state.doc.toString()).toBe(many);
  } finally { document.querySelector('[role=dialog]')?.remove(); view.destroy(); root.remove(); }
});

it('shows no stepping controls for a note with a single picture', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('![唯一](/only.webp)', 'single');
    root.querySelector('.cm-markra-image')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    const dialog = document.querySelector<HTMLElement>('[role=dialog]')!;
    expect(dialog.querySelector('.markra-media-viewer-counter')).toBeNull();
    expect(dialog.querySelector('button[aria-label="下一张"]')).toBeNull();
  } finally { document.querySelector('[role=dialog]')?.remove(); view.destroy(); root.remove(); }
});

it('keeps typing away from the note while a picture is being viewed', () => {
  const editor = mount();
  try {
    editor.press('mousedown');
    editor.image().dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true }));
    const dialog = document.querySelector<HTMLElement>('[role=dialog]')!;
    const page = editor.view.dom.closest('body > div')!;
    // 查看期间页面其余部分不可交互，焦点在查看器里。
    expect(page.hasAttribute('inert')).toBe(true);
    expect(dialog.hasAttribute('inert')).toBe(false);
    expect(dialog.contains(document.activeElement)).toBe(true);
    dialog.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!.click();
    expect(page.hasAttribute('inert')).toBe(false);
    expect(document.activeElement).toBe(editor.view.contentDOM);
    expect([editor.view.state.selection.main.from, editor.view.state.selection.main.to]).toEqual([imageFrom, imageTo]);
    expect(editor.view.state.doc.toString()).toBe(source);
  } finally { editor.done(); }
});
