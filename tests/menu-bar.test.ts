// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { answerMenuBar } from '../src/menu-bar-host';
import { menuBarSections, quickCapture, type MenuBarReply, type MenuBarSections } from '../src/menu-bar-model';
import { Notebook, type Note } from '../src/notebook';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
});
const { mountMenuBar } = await import('../src/menu-bar');
// jsdom 不排版，也没有 scrollIntoView。
Element.prototype.scrollIntoView ??= () => {};

afterEach(() => { document.body.replaceChildren(); });

const note = (id: string, updatedAt: number, source: string, extra: Partial<Note> = {}): Note =>
  ({ id, folderId: 'default', source, createdAt: 0, updatedAt, ...extra });

it('lists up to five pinned and six recent notes, newest first, leaving the trash out', () => {
  const notes = [
    ...Array.from({ length: 7 }, (_, index) => note(`p${index}`, 100 + index, `# 置顶 ${index}`, { pinned: true })),
    ...Array.from({ length: 8 }, (_, index) => note(`r${index}`, index, `# 最近 ${index}\n\n第二行`)),
    note('gone', 999, '# 废纸篓', { trashed: true }),
  ];
  const { pinned, recent, matches } = menuBarSections(notes, '  ');
  expect(pinned.map(item => item.id)).toEqual(['p6', 'p5', 'p4', 'p3', 'p2']);
  expect(recent.map(item => item.id)).toEqual(['r7', 'r6', 'r5', 'r4', 'r3', 'r2']);
  expect(recent[0]).toEqual({ id: 'r7', title: '最近 7', summary: '第二行', pinned: false });
  expect(matches).toEqual([]);
});

it('searches titles first and then the text, up to eight notes', () => {
  const notes = [
    note('body-new', 50, '# 周报\n\n记得买柠檬'),
    note('title-old', 10, '# 柠檬挞'),
    note('title-new', 20, '# 柠檬水'),
    ...Array.from({ length: 9 }, (_, index) => note(`more${index}`, index, `# 其他\n\n柠檬 ${index}`)),
    note('trashed', 99, '# 柠檬', { trashed: true }),
  ];
  const { matches, pinned, recent } = menuBarSections(notes, ' 柠檬 ');
  expect(matches.map(item => item.id)).toEqual(['title-new', 'title-old', 'body-new', 'more8', 'more7', 'more6', 'more5', 'more4']);
  expect([pinned, recent]).toEqual([[], []]);
});

it('keeps what was typed as the note, with its first line as the title', () => {
  expect(quickCapture('  # 买菜\n- 牛奶\n  ')).toEqual({ title: '买菜', source: '# 买菜\n- 牛奶' });
  expect(quickCapture('明天十点开会')).toEqual({ title: '明天十点开会', source: '明天十点开会' });
  expect(quickCapture(' \n ')).toBeUndefined();
});

it('lets the main window list, create and open notes, and refuses while the library is busy', async () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 第一篇');
  const create = vi.fn(async (title: string) => ({ id: 'new', title }));
  const open = vi.fn(async () => {});
  let blocked: string | undefined = '正在同步或整理笔记库，请稍后再试';
  const actions = { blocked: () => blocked, create, open };
  // 忙的时候也能列出笔记，只是不能新建和打开。
  expect((await answerMenuBar(book, actions, { id: '1', action: 'list', query: '' })).sections?.recent.map(item => item.title)).toEqual(['第一篇']);
  expect(await answerMenuBar(book, actions, { id: '2', action: 'create', text: '新想法' })).toEqual({ id: '2', error: blocked });
  blocked = undefined;
  expect(await answerMenuBar(book, actions, { id: '3', action: 'create', text: '新想法\n细节' })).toEqual({ id: '3', created: { id: 'new', title: '新想法' } });
  expect(create).toHaveBeenCalledWith('新想法', '新想法\n细节');
  expect((await answerMenuBar(book, actions, { id: '4', action: 'create', text: '   ' })).error).toBe('先写点内容');
  expect((await answerMenuBar(book, actions, { id: '5', action: 'open', noteId: 'missing' })).error).toBe('这篇笔记已不存在');
  await answerMenuBar(book, actions, { id: '6', action: 'open', noteId: book.current!.id });
  expect(open).toHaveBeenCalledWith(book.current!.id);
});

function panel(sections: MenuBarSections) {
  const sent: unknown[] = [];
  const host = {
    send: vi.fn(async (request: { action: string; text?: string }): Promise<MenuBarReply> => {
      sent.push(request);
      if (request.action === 'create') return { id: 'x', created: { id: 'new', title: request.text!.split('\n')[0] } };
      return { id: 'x', sections };
    }),
    openMain: vi.fn(async () => {}), openSettings: vi.fn(async () => {}), quit: vi.fn(async () => {}), hide: vi.fn(async () => {}), fit: vi.fn(),
  };
  const root = document.body.appendChild(document.createElement('div'));
  mountMenuBar(root, host);
  const input = root.querySelector('input')!;
  const key = (name: string, init: KeyboardEventInit = {}) => input.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));
  const type = (text: string) => { input.value = text; input.dispatchEvent(new Event('input')); };
  const texts = (selector: string) => [...root.querySelectorAll(selector)].map(element => element.textContent);
  return { root, input, host, sent, key, type, texts };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 120));
const item = (id: string, title: string, pinned = false) => ({ id, title, summary: '无更多文本', pinned });

it('shows pinned and recent notes, walks them with the arrows and opens one in the window', async () => {
  const view = panel({ pinned: [item('a', '欢迎', true)], recent: [item('b', '习惯打卡'), item('c', '会议纪要')], matches: [] });
  await settle();
  expect(view.input.placeholder).toBe('新建笔记或搜索');
  expect(document.activeElement).toBe(view.input);
  expect(view.texts('.menu-bar-header')).toEqual(['置顶', '最近']);
  expect(view.texts('.menu-bar-row .menu-bar-title')).toEqual(['欢迎', '习惯打卡', '会议纪要']);
  expect(view.root.querySelector('.menu-bar-title svg')).not.toBeNull();
  expect(view.texts('.menu-bar-footer button')).toEqual(['打开轻羽', '', '退出']);
  expect(view.host.fit).toHaveBeenCalled();

  view.key('ArrowDown'); view.key('ArrowDown');
  expect(view.texts('[aria-selected="true"] .menu-bar-title')).toEqual(['习惯打卡']);
  view.key('Enter');
  await settle();
  expect(view.sent).toContainEqual({ action: 'open', noteId: 'b' });
  expect(view.host.openMain).toHaveBeenCalledTimes(1);
});

it('saves what was typed on Return and opens it with ⌘Return', async () => {
  const view = panel({ pinned: [], recent: [], matches: [item('m', '柠檬水')] });
  await settle();
  expect(view.root.querySelector('.menu-bar-empty')?.textContent).toBe('置顶或编辑过的笔记会显示在这里。');
  view.type('柠檬');
  await settle();
  // 打字时第一行是「新建笔记」，默认选中；下面是匹配的笔记。
  expect(view.texts('.menu-bar-action span')).toEqual(['新建笔记“柠檬”']);
  expect(view.root.querySelector('.menu-bar-action')?.getAttribute('aria-selected')).toBe('true');
  expect(view.texts('.menu-bar-header')).toEqual(['笔记']);
  view.key('Enter');
  await settle();
  expect(view.sent).toContainEqual({ action: 'create', text: '柠檬' });
  expect(view.input.value).toBe('');
  expect(view.texts('.menu-bar-saved span')).toEqual(['已保存“柠檬”']);
  expect(view.host.openMain).not.toHaveBeenCalled();

  view.type('要打开的');
  view.key('Enter', { metaKey: true });
  await settle();
  expect(view.sent).toContainEqual({ action: 'create', text: '要打开的' });
  expect(view.host.openMain).toHaveBeenCalledTimes(1);

  // 输入法组字时的回车不算。
  view.type('拼音');
  view.key('Enter', { isComposing: true });
  await settle();
  expect(view.sent).not.toContainEqual({ action: 'create', text: '拼音' });
});

it('clears the text on Escape before closing, and runs the footer buttons', async () => {
  const view = panel({ pinned: [], recent: [item('b', '习惯打卡')], matches: [] });
  await settle();
  view.type('草稿');
  view.key('Escape');
  expect(view.input.value).toBe('');
  expect(view.host.hide).not.toHaveBeenCalled();
  view.key('Escape');
  expect(view.host.hide).toHaveBeenCalledTimes(1);
  const [open, settings, quit] = view.root.querySelectorAll<HTMLButtonElement>('.menu-bar-footer button');
  open.click(); settings.click(); quit.click();
  expect(settings.ariaLabel).toBe('设置');
  expect([view.host.openMain, view.host.openSettings, view.host.quit].map(fn => fn.mock.calls.length)).toEqual([1, 1, 1]);
});
