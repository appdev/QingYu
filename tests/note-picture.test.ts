// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { mountNotePicture, pictureFilename, picturePassage, showNotePicture } from '../src/note-picture';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} }));
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: {}, configurable: true });
});
vi.mock('html2canvas', () => ({ default: async () => ({ toBlob: (callback: BlobCallback) => callback(new Blob(['preview'])) }) }));

const table = '| 项 | 数 |\n| --- | --- |\n| 中文 | 2 |';
const fence = '```js\nconst 汉字 = 1\n```';
const note = `---\ntags: [private]\n---\n# 标题\n\n前一行内容。\n\n${table}\n\n${fence}\n\n后一行内容。\n`;

it('uses the whole note for an empty or whitespace selection without its file metadata', () => {
  const result = picturePassage(note, { from: 4, to: 19 });
  expect(result).toEqual({ text: note.slice(note.indexOf('# 标题')).trim(), startsAtTitle: true, isSelection: false });
});

it('expands cropped lines, tables and fences without exporting other text', () => {
  for (const [word, expected] of [['内容', '前一行内容。'], ['中文', table], ['汉字', fence]]) {
    const from = note.indexOf(word);
    expect(picturePassage(note, { from, to: from + word.length })).toEqual({ text: expected, startsAtTitle: false, isSelection: true });
  }
});

it('makes a bounded PNG filename without splitting a Unicode character', () => {
  expect(pictureFilename('计划/周报')).toBe('计划-周报.png');
  expect(pictureFilename(' ')).toBe('轻羽笔记.png');
  expect(pictureFilename('a'.repeat(71) + '😀尾')).toBe('a'.repeat(71) + '😀.png');
});

it('opens a macOS modal with desktop controls and keeps the mobile image sheet controls', async () => {
  const html = document.documentElement;
  const request = { title: '', source: '' };
  const dialog = () => document.querySelector<HTMLDialogElement>('dialog[open]')!;
  const actions = () => [...dialog().querySelectorAll<HTMLButtonElement>('.note-picture-actions button')];
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
    configurable: true, value(this: HTMLDialogElement) { this.open = true; },
  });
  try {
    html.dataset.platform = 'macos';
    const restoreFocus = vi.fn();
    await showNotePicture(request, restoreFocus);
    const [done, ...exports] = actions();
    expect(dialog().classList.contains('note-picture-macos-dialog')).toBe(true);
    expect(dialog().querySelector('header')).toBeNull();
    expect(exports.map(button => button.dataset.pictureAction)).toEqual(['copy', 'save', 'share']);
    expect(exports.every(button => button.classList.contains('button-regular'))).toBe(true);
    expect(dialog().querySelector('input[role=switch]')).toBeNull();
    done.click();
    expect(dialog()).toBeNull();
    expect(restoreFocus).toHaveBeenCalledOnce();

    // iPadOS can report a Mac platform and still use the mobile panel.
    html.setAttribute('data-mobile', '');
    await showNotePicture(request, vi.fn());
    expect(dialog().classList.contains('note-picture-macos-dialog')).toBe(false);
    expect(dialog().querySelector('header .floating-button')).not.toBeNull();
    expect(dialog().querySelector('input[role=switch]')).not.toBeNull();
    expect(actions().map(button => button.dataset.pictureAction)).toEqual(['share', 'copy']);
    expect(actions().every(button => button.classList.contains('button-big') && button.classList.contains('button-wide'))).toBe(true);
  } finally {
    dialog()?.remove();
    Reflect.deleteProperty(HTMLDialogElement.prototype, 'showModal');
    delete html.dataset.platform;
    html.removeAttribute('data-mobile');
  }
});

it('keeps dashed and bulleted list markers apart, leaving tasks and numbers alone', async () => {
  const { renderMarkdown } = await import('../src/markdown');
  const { html } = renderMarkdown('- 破折号\n  * 嵌套圆点\n\n* 圆点\n+ 加号\n\n- [ ] 任务\n- 普通项\n\n1. 编号');
  expect(html).toMatch(/<li [^>]*class="li-dash"[^>]*>破折号/);
  expect(html).toMatch(/<li [^>]*class="li-bullet"[^>]*>嵌套圆点/);
  expect(html).toMatch(/<li [^>]*class="li-bullet"[^>]*>圆点/);
  expect(html).toMatch(/<li [^>]*class="li-bullet"[^>]*>加号/);
  expect(html).toMatch(/<li [^>]*class="task-list-item"[^>]*>/);
  expect(html).not.toMatch(/task-list-item[^"]*li-/);
  expect(html).toMatch(/<li [^>]*class="li-dash"[^>]*>普通项/);
  expect(html).toMatch(/<ol[^>]*>\s*<li data-sourcepos="[^"]*">编号/);
});

it('keeps the macOS preview and actions stable until the latest image is decoded', async () => {
  const pending: Array<() => void> = [];
  let sequence = 0;
  const createObjectURL = vi.fn(() => `blob:preview-${++sequence}`);
  const revokeObjectURL = vi.fn();
  Object.defineProperties(URL, {
    createObjectURL: { configurable: true, value: createObjectURL },
    revokeObjectURL: { configurable: true, value: revokeObjectURL },
  });
  Object.defineProperty(HTMLImageElement.prototype, 'decode', {
    configurable: true, value: () => new Promise<void>(resolve => pending.push(resolve)),
  });
  const root = document.body.appendChild(document.createElement('div'));
  const dispose = mountNotePicture(root, { title: 'Example', source: '# Example' }, true, vi.fn());
  const preview = () => root.querySelector<HTMLImageElement>('.note-picture-preview')!;
  const share = root.querySelector<HTMLButtonElement>('[data-picture-action=share]')!;
  try {
    await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledTimes(1));
    pending.shift()?.();
    await vi.waitFor(() => expect(preview().src).toBe('blob:preview-1'));
    root.querySelector<HTMLButtonElement>('[aria-label=海边]')!.click();
    expect(root.querySelector('.note-picture-status')?.textContent).toBe('');
    expect(share.disabled).toBe(false);
    await vi.waitFor(() => expect(pending).toHaveLength(1));
    const finishSea = pending.shift()!;
    root.querySelector<HTMLButtonElement>('[aria-label=暮色]')!.click();
    await vi.waitFor(() => expect(pending).toHaveLength(1));
    expect(preview().src).toBe('blob:preview-1');
    expect(revokeObjectURL).not.toHaveBeenCalled();

    pending.shift()!();
    await vi.waitFor(() => expect(preview().src).toBe('blob:preview-3'));
    finishSea();
    await vi.waitFor(() => expect(revokeObjectURL).toHaveBeenCalledWith('blob:preview-2'));
    expect(preview().src).toBe('blob:preview-3');
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:preview-1');
  } finally {
    dispose();
    root.remove();
    localStorage.removeItem('qingyu.notePicture');
    Reflect.deleteProperty(URL, 'createObjectURL');
    Reflect.deleteProperty(URL, 'revokeObjectURL');
    Reflect.deleteProperty(HTMLImageElement.prototype, 'decode');
  }
});
