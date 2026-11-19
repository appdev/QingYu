// @vitest-environment jsdom
import { EditorView } from '@codemirror/view';
import { undo } from '@codemirror/commands';
import { expect, it, vi } from 'vitest';
import { createRichEditor } from '../src/rich-editor';
import { mixHex } from '../src/preferences';
import { readFileSync } from 'node:fs';

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

function touch(target: Element, type: string, id: number, x: number, y: number, timeStamp = 0) {
  const event = new MouseEvent(type, { button: 0, bubbles: true, cancelable: true, clientX: x, clientY: y });
  Object.defineProperty(event, 'timeStamp', { value: timeStamp });
  target.dispatchEvent(Object.assign(event, { pointerId: id, pointerType: 'touch' }));
}

function openOnPhone(source = '![日落](/sunset.webp)') {
  document.documentElement.toggleAttribute('data-mobile', true);
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  editor.activate(source, 'phone-viewer');
  const toggles: boolean[] = [];
  const listen = (event: Event) => toggles.push((event as CustomEvent<boolean>).detail);
  document.addEventListener('media-viewer-toggle', listen);
  const image = root.querySelector<HTMLImageElement>('.cm-markra-image')!;
  image.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true, cancelable: true }));
  image.dispatchEvent(new MouseEvent('click', { button: 0, bubbles: true, cancelable: true }));
  const dialog = document.querySelector<HTMLElement>('[role=dialog]')!;
  const content = dialog.querySelector<HTMLElement>('.markra-media-viewer-content')!;
  const canvas = dialog.querySelector<HTMLElement>('.markra-media-viewer-canvas')!;
  // jsdom 没有布局：画布中心固定在 (200, 300)，图片占 (100, 200)–(300, 400)，平移时跟着动。
  canvas.getBoundingClientRect = () => {
    const [, x = '0', y = '0'] = /translate\((-?[\d.]+)px, (-?[\d.]+)px\)/.exec(canvas.style.transform) ?? [];
    return { left: 0 + +x, top: 100 + +y, right: 400 + +x, bottom: 500 + +y, width: 400, height: 400, x: +x, y: 100 + +y, toJSON() {} };
  };
  const picture = () => dialog.querySelector<HTMLElement>('.markra-media-viewer-media')!;
  picture().getBoundingClientRect = () => ({ left: 100, top: 200, right: 300, bottom: 400, width: 200, height: 200, x: 100, y: 200, toJSON() {} });
  const zoom = () => dialog.querySelector('.markra-media-viewer-zoom-value')!.textContent;
  return {
    dialog, content, canvas, picture, zoom, toggles,
    done() {
      if (dialog.isConnected) dialog.querySelector<HTMLButtonElement>('button[aria-label="关闭"]')!.click();
      document.removeEventListener('media-viewer-toggle', listen);
      view.destroy(); root.remove(); document.documentElement.removeAttribute('data-mobile');
    },
  };
}

it('zooms with two fingers around the point between them and springs back when pinched too small', () => {
  const viewer = openOnPhone();
  try {
    expect(viewer.toggles).toEqual([true]);
    // 两指从相距 100 张开到 200：放大到两倍，两指中点 (200, 300) 下的那一点不动。
    touch(viewer.picture(), 'pointerdown', 1, 150, 300);
    touch(viewer.picture(), 'pointerdown', 2, 250, 300);
    touch(viewer.content, 'pointermove', 2, 350, 300);
    touch(viewer.content, 'pointermove', 1, 50, 300);
    expect(viewer.zoom()).toBe('300%');
    expect(viewer.canvas.style.transform).toBe('translate(0px, 0px) scale(3)');
    touch(viewer.content, 'pointerup', 1, 50, 300);
    touch(viewer.content, 'pointerup', 2, 350, 300);
    // 用过双指的手势不算轻点，查看器不关闭。
    expect(viewer.dialog.isConnected).toBe(true);
    // 偏离中心的中点：那一点跟着中点走。
    touch(viewer.picture(), 'pointerdown', 3, 100, 200);
    touch(viewer.picture(), 'pointerdown', 4, 140, 200);
    touch(viewer.content, 'pointermove', 4, 180, 200);
    expect(viewer.zoom()).toBe('600%');
    touch(viewer.content, 'pointerup', 3, 100, 200); touch(viewer.content, 'pointerup', 4, 180, 200);
    // 捏到比原尺寸还小，松手弹回原样并居中。
    touch(viewer.picture(), 'pointerdown', 5, 0, 300);
    touch(viewer.picture(), 'pointerdown', 6, 400, 300);
    touch(viewer.content, 'pointermove', 6, 20, 300);
    touch(viewer.content, 'pointerup', 6, 20, 300); touch(viewer.content, 'pointerup', 5, 0, 300);
    expect(viewer.zoom()).toBe('100%');
    expect(viewer.canvas.style.transform).toBe('translate(0px, 0px) scale(1)');
  } finally { viewer.done(); }
  expect(viewer.toggles).toEqual([true, false]);
});

it('doubles the picture on a double tap where it was tapped and restores it on the next one', () => {
  const viewer = openOnPhone();
  try {
    const tap = (time: number) => { touch(viewer.picture(), 'pointerdown', 7, 150, 250, time); touch(viewer.content, 'pointerup', 7, 150, 250, time); };
    tap(1000); tap(1200);
    expect(viewer.zoom()).toBe('200%');
    // 点的位置 (150, 250) 放大后还在原处：中心 (200, 300)，偏移 (-50, -50) 变成 (-100, -100)。
    expect(viewer.canvas.style.transform).toBe('translate(50px, 50px) scale(2)');
    tap(2000); tap(2100);
    expect(viewer.zoom()).toBe('100%');
    // 隔得太久的两次轻点不算双击。
    tap(3000); tap(3600);
    expect(viewer.zoom()).toBe('100%');
    expect(viewer.dialog.isConnected).toBe(true);
  } finally { viewer.done(); }
});

it('follows the finger across, slides to the next picture and puts the name on top on the phone', async () => {
  const viewer = openOnPhone('![第一张](/a.webp)\n\n![第二张](/b.webp)');
  try {
    const counter = () => viewer.dialog.querySelector('.markra-media-viewer-counter')!.textContent;
    const title = viewer.dialog.querySelector<HTMLElement>('.markra-media-viewer-title')!;
    // 名称在顶部正中，不在工具条里；工具条只有切换和关闭。
    expect(title.classList.contains('markra-media-viewer-title-top')).toBe(true);
    expect(title.parentElement!.classList.contains('markra-media-viewer-panel')).toBe(true);
    expect(title.textContent).toBe('第一张');
    expect(viewer.dialog.querySelector('.markra-media-viewer-toolbar .markra-media-viewer-title')).toBeNull();
    expect(counter()).toBe('1 / 2');

    // 往右拖第一张：前面没有了，只跟三成并弹回。
    touch(viewer.picture(), 'pointerdown', 9, 100, 300);
    touch(viewer.content, 'pointermove', 9, 200, 300);
    expect(viewer.canvas.style.transform).toBe('translate(30px, 0px) scale(1)');
    touch(viewer.content, 'pointerup', 9, 200, 300);
    expect(viewer.canvas.style.transform).toBe('translate(0px, 0px) scale(1)');
    expect(counter()).toBe('1 / 2');

    // 往左拖：图片跟手；不够远松手弹回。
    touch(viewer.picture(), 'pointerdown', 10, 300, 300);
    touch(viewer.content, 'pointermove', 10, 260, 305);
    expect(viewer.canvas.style.transform).toBe('translate(-40px, 0px) scale(1)');
    touch(viewer.content, 'pointerup', 10, 260, 305);
    expect(viewer.canvas.style.transform).toBe('translate(0px, 0px) scale(1)');

    // 拖够远：先滑出屏幕，再换成第二张从右边滑回中间。
    touch(viewer.picture(), 'pointerdown', 8, 300, 300);
    touch(viewer.content, 'pointermove', 8, 150, 310);
    touch(viewer.content, 'pointerup', 8, 150, 310);
    expect(viewer.canvas.style.transform).toMatch(/^translate\(-\d+px, 0px\) scale\(1\)$/);
    expect(counter()).toBe('1 / 2');
    await new Promise(resolve => setTimeout(resolve, 220));
    expect(counter()).toBe('2 / 2');
    expect(title.textContent).toBe('第二张');
    expect(viewer.canvas.style.transform).toBe('translate(0px, 0px) scale(1)');
    // 缩放那组按钮和分隔线都有可按类名整组隐藏的标记（样式在 rich-editor.css）。
    expect(viewer.dialog.querySelectorAll('.markra-media-viewer-zoom-separator')).toHaveLength(2);
    expect(readFileSync('src/rich-editor.css', 'utf8')).toMatch(/html\[data-mobile\] :is\(\.markra-media-viewer-zoom-out-button, \.markra-media-viewer-zoom-value, \.markra-media-viewer-zoom-in-button,\s*\.markra-media-viewer-reset-button, \.markra-media-viewer-fullscreen-button, \.markra-media-viewer-zoom-separator\) \{ display: none; \}/);
  } finally { viewer.done(); }
});

it('darkens what shows behind the phone’s system bars to the viewer’s color', () => {
  expect(mixHex('#FFFFFF', '#000000', .62)).toBe('#616161');
  expect(mixHex('#1E1E1E', '#000000', .62)).toBe('#0B0B0B');
});
