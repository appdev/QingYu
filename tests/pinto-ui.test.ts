// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { mountSettings, type SettingsSection } from '../src/settings-view';
import { preferences, PreferenceStore, previewPreferences } from '../src/preferences';
import { themeNames, themePalette } from '../src/themes';
import { showLockNotePanel, showShare } from '../src/reference-panels';
import { formatTransaction } from '../src/format';
import { EditorState } from '@codemirror/state';
import { groupNotes, noteListDate, type Note } from '../src/notebook';
import type { SyncClient } from '../src/sync-session';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
const platform = vi.hoisted(() => ({ native: false }));
vi.mock('../src/preferences', async importOriginal => ({
  ...await importOriginal<typeof import('../src/preferences')>(),
  get native() { return platform.native; },
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(null) }));
const sync: SyncClient = { load: async () => ({ endpoint: '', bucket: '', region: '', prefix: '', accessKeyId: '', secretAccessKey: '' }), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
beforeEach(() => { platform.native = false; localStorage.clear(); preferences.update({}, true); document.body.replaceChildren(); });

it('persists all six appearance themes, updates rendering tokens and restores the default palette', async () => {
  const root = document.createElement('div'); document.body.append(root); const view = mountSettings(root, sync, vi.fn()); await view.loaded; view.select('appearance');
  expect(root.querySelectorAll('[name=previewFont]')).toHaveLength(1);
  expect(root.querySelector('#settings-panel-typography, [name=codeFont], [name=interfaceFont]')).toBeNull();
  expect(root.querySelector('#settings-panel-appearance')?.textContent).not.toContain('更多字体设置');
  root.querySelector<HTMLButtonElement>('[data-appearance=dark]')!.click();
  for (const name of Object.keys(themeNames) as (keyof typeof themeNames)[]) {
    const button = root.querySelector<HTMLButtonElement>(`[data-color-theme=${name}]`)!; button.click();
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(new PreferenceStore(localStorage).value.theme).toBe(name);
    expect(previewPreferences()).toMatchObject({ theme: name, dark: true });
    if (name !== 'default') expect(document.documentElement.style.getPropertyValue('--paper')).toBe(themePalette(name, true).paper);
  }
  preferences.update({ theme: 'default', previewFont: 'Georgia' });
  expect(document.documentElement.style.getPropertyValue('--paper')).toBe('');
  expect(root.querySelector<HTMLSelectElement>('[name=previewFont]')!.value).toBe('Georgia');
  view.destroy();
});

it('omits the removed privacy sections from settings and search', async () => {
  const root = document.createElement('div'); document.body.append(root); const view = mountSettings(root, sync, vi.fn()); await view.loaded;
  for (const section of ['privacy', 'encryption']) {
    expect(root.querySelector('#settings-tab-' + section + ', #settings-panel-' + section)).toBeNull();
    expect(view.select(section as SettingsSection)).toBe('general');
  }
  const search = root.querySelector<HTMLInputElement>('.settings-search input')!;
  for (const term of ['隐私', '加密', '诊断', '服务条款']) {
    search.value = term; search.dispatchEvent(new Event('input'));
    expect(root.querySelector('.settings-search-results')?.textContent).toBe('没有匹配的设置');
  }
  view.destroy();
});

it('previews settings while searching and only changes history after choosing a result', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, sync, vi.fn());
  await view.loaded;
  view.select('appearance');
  const search = root.querySelector<HTMLInputElement>('.settings-search input')!;
  const query = (text: string) => {
    search.value = text;
    search.dispatchEvent(new Event('input', { bubbles: true }));
  };
  search.focus();
  query('正文宽度');
  expect(root.querySelector('.settings-search-results button')?.textContent).toBe('正文宽度通用');
  expect(root.querySelector('h1')?.textContent).toBe('通用');
  expect(document.activeElement).toBe(search);
  query('不存在的设置');
  expect(root.querySelector('.settings-search-results')?.textContent).toContain('没有匹配的设置');
  query('');
  expect(root.querySelector('h1')?.textContent).toBe('外观');
  const escape = vi.fn();
  root.addEventListener('keydown', escape);
  query('正文宽度');
  search.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', cancelable: true, bubbles: true }));
  expect(escape).not.toHaveBeenCalled();
  expect(root.querySelector('h1')?.textContent).toBe('外观');
  query('正文宽度');
  search.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true, bubbles: true }));
  expect(search.value).toBe('');
  expect(root.querySelector('h1')?.textContent).toBe('通用');
  root.querySelector<HTMLButtonElement>('.settings-back')!.click();
  expect(root.querySelector('h1')?.textContent).toBe('外观');
  query('Bucket');
  expect(root.querySelector('.settings-search-results button')?.textContent).toBe('Bucket存储与同步');
  expect(root.querySelector('h1')?.textContent).toBe('存储与同步');
  query('S3 同步');
  root.querySelector<HTMLButtonElement>('.settings-search-results button')!.click();
  expect(root.querySelector('#settings-panel-storage')?.hasAttribute('hidden')).toBe(false);
  expect(root.querySelector('.settings-sync-form input[name=bucket]')).not.toBeNull();
  view.destroy();
});

it('shows the hosted MCP address and keeps local API controls in the desktop app', () => {
  const meta = document.createElement('meta');
  meta.name = 'qingyu-web-auth';
  meta.content = 'enabled';
  document.head.append(meta);
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, sync, vi.fn());
  try {
    root.querySelector<HTMLButtonElement>('#settings-tab-ai')!.click();
    expect(root.querySelector('h1')!.textContent).toBe('MCP 配置');
    const page = root.querySelector<HTMLElement>('#settings-panel-ai')!;
    expect(page.textContent).toContain(`${location.origin}/mcp`);
    expect(page.textContent).toContain('Authorization');
    expect(page.querySelector('button')).toBeNull();
  } finally {
    view.destroy();
    meta.remove();
  }
});

it('adjusts note text live with the five slider stops and navigates back and forward', () => {
  const root = document.body.appendChild(document.createElement('div')), view = mountSettings(root, sync, vi.fn());
  view.select('appearance');
  const slider = root.querySelector<HTMLInputElement>('input[name=previewFontSize]')!;
  expect(slider.getAttribute('aria-valuetext')).toBe('13 pt');
  expect(slider.max).toBe('4');
  slider.value = '3'; slider.dispatchEvent(new Event('input', { bubbles: true }));
  expect(slider.getAttribute('aria-valuetext')).toBe('20 pt');
  expect(previewPreferences().size).toBe(20);
  expect(new PreferenceStore(localStorage).value.previewFontSize).toBe(20);
  preferences.update({ previewFontSize: 22 });
  expect(slider.getAttribute('aria-valuetext')).toBe('24 pt');
  expect(slider.max).toBe('4');
  expect(root.querySelectorAll('.appearance-size-stops i')).toHaveLength(5);
  expect(root.querySelectorAll('.mode-picture[data-mode=system] .mode-window')).toHaveLength(2);

  root.querySelector<HTMLButtonElement>('.settings-back')!.click();
  expect(root.querySelector('h1')!.textContent).toBe('通用');
  root.querySelector<HTMLButtonElement>('.settings-forward')!.click();
  expect(root.querySelector('h1')!.textContent).toBe('外观');
  view.destroy();
});

it('shows the parent of the current desktop settings page independently of navigation history', () => {
  platform.native = true;
  const root = document.body.appendChild(document.createElement('div'));
  const close = vi.fn(), view = mountSettings(root, sync, close);
  const back = root.querySelector<HTMLButtonElement>('.settings-back')!;
  const forward = root.querySelector<HTMLButtonElement>('.settings-forward')!;
  const title = () => root.querySelector('h1')?.textContent;
  const parent = () => root.querySelector<HTMLButtonElement>('.settings-breadcrumb button')!;
  const shortcut = (key: string, isComposing = false) => root.dispatchEvent(new KeyboardEvent('keydown', {
    key, metaKey: true, bubbles: true, cancelable: true, isComposing,
  }));
  expect(back.disabled).toBe(true);
  expect(forward.disabled).toBe(true);
  back.click();
  expect(close).not.toHaveBeenCalled();
  view.select('appearance');
  view.select('personal');
  expect(title()).toBe('个人信息');
  expect(parent().textContent).toBe('个人资料');
  expect(root.querySelector('#settings-tab-account')?.getAttribute('aria-selected')).toBe('true');
  expect(shortcut('[', true)).toBe(true);
  expect(title()).toBe('个人信息');
  expect(shortcut('[')).toBe(false);
  expect(title()).toBe('外观');
  expect(parent().closest('.settings-breadcrumb')?.hasAttribute('hidden')).toBe(true);
  expect(shortcut(']')).toBe(false);
  expect(title()).toBe('个人信息');
  parent().click();
  expect(title()).toBe('个人资料');
  expect(forward.disabled).toBe(true);
  view.destroy();
});

it('shows sharing tabs with mock web publishing and keeps unavailable lock operations disabled', () => {
  platform.native = true;
  Object.defineProperty(HTMLElement.prototype, 'showPopover', { configurable: true, value() {} });
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.open = true; } });
  const anchor = document.createElement('button'); document.body.append(anchor);
  showShare(anchor, '<unsafe>', '<script>not markup</script>', vi.fn());
  const panel = document.querySelector('.share-popover')!;
  const tabs = [...panel.querySelectorAll<HTMLButtonElement>('[role=tab]')];
  expect(tabs.map(tab => tab.textContent)).toEqual(['发送副本', '网页', '图片']);
  expect(panel.textContent).not.toMatch(/协作|邀请成员|通过链接加入/);
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('发送副本');
  expect(panel.querySelector<HTMLButtonElement>('.share-copy')!.disabled).toBe(false);
  tabs[1].click();
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('网页');
  expect(panel.querySelector('strong')!.textContent).toBe('<unsafe>');
  expect(panel.querySelector('script')).toBeNull();
  expect(panel.querySelector<HTMLButtonElement>('.primary-button')!.disabled).toBe(false);
  expect(panel.textContent).toContain('模拟测试');
  tabs[2].click();
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('图片');
  expect(panel.querySelector('[aria-label="图片背景"]')).not.toBeNull();
  expect([...panel.querySelectorAll('.share-actions button')].map(button => button.textContent)).toEqual(['复制图片', '保存…', '分享…']);
  tabs[2].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('发送副本');
  tabs[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
  expect(panel.querySelector('[aria-selected=true]')!.textContent).toBe('图片');
  showLockNotePanel();
  const lock = document.querySelector('.reference-lock')!;
  expect(lock.getAttribute('aria-label')).toBe('设置密码');
  expect(lock.querySelectorAll('input:disabled')).toHaveLength(3);
  expect(lock.querySelector('header button:disabled')!.textContent).toBe('完成');
  delete (HTMLElement.prototype as Partial<HTMLElement>).showPopover; delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).showModal;
});

it.each([['title', '# text'], ['subheading', '### text'], ['list', '- text'], ['ordered', '1. text'], ['codeblock', '\n```\ntext\n```\n'], ['divider', '\n---\n'] ] as const)('applies %s through the existing editor transaction path', (format, expected) => {
  const state = EditorState.create({ doc: 'text', selection: { anchor: 0, head: 4 } });
  expect(formatTransaction(state, format).state.doc.toString()).toBe(expected);
});

it('uses local calendar boundaries for Today, Yesterday, 7 days and 30 days', () => {
  vi.useFakeTimers();
  try {
    vi.setSystemTime(new Date(2026, 10, 13, 12));
    const notes = [0,1,2,7,8,30,31].map(days => ({ id: String(days), updatedAt: new Date(2026, 10, 13 - days).getTime() } as Note));
    expect(groupNotes(notes).map(group => [group.label, group.notes.map(note => note.id)])).toEqual([
      ['今天', ['0']], ['昨天', ['1']], ['过去 7 天', ['2','7']], ['过去 30 天', ['8','30']], ['10月', ['31']],
    ]);
  } finally { vi.useRealTimers(); }
});

it('formats the note row time by local day and switches to a date at seven days', () => {
  const date = noteListDate(new Date(2026, 10, 17, 12));
  expect(date(+new Date(2026, 10, 17, 10, 19))).toBe('10:19');
  expect(date(+new Date(2026, 10, 16, 23, 59))).toBe('昨天');
  expect(date(+new Date(2026, 10, 11, 0))).toBe('星期三');
  expect(date(+new Date(2026, 10, 10, 23, 59))).toBe('2026/11/10');
});

it('sorts by title or creation date and keeps pinned notes above the ungrouped list', () => {
  const now = new Date(), yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1, 12).getTime();
  const notes = [
    { id: 'a', title: 'Title 10', source: '', createdAt: yesterday, updatedAt: +now },
    { id: 'b', title: 'Title 2', source: '', createdAt: +now, updatedAt: +now },
    { id: 'p', title: 'Pinned', source: '', pinned: true, createdAt: yesterday, updatedAt: +now },
  ] as Note[];
  expect(groupNotes(notes, { key: 'title', reverse: false, grouped: true }).map(group => [group.label, group.notes.map(note => note.id)]))
    .toEqual([['置顶', ['p']], ['', ['b', 'a']]]);
  expect(groupNotes(notes, { key: 'createdAt', reverse: true, grouped: true }).map(group => group.label)).toEqual(['置顶', '昨天', '今天']);
});
