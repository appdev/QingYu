// @vitest-environment jsdom
import './fixtures/chinese-ui';
import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';

const html = readFileSync('login.html', 'utf8');
const fetchMock = vi.fn();
const media = { matches: false, addEventListener: vi.fn() };
const password = () => document.querySelector<HTMLInputElement>('#password')!;
const button = () => document.querySelector<HTMLButtonElement>('#login-button')!;
const message = () => document.querySelector('#form-message')!.textContent;
const submit = () => document.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
beforeEach(async () => {
  vi.resetModules(); fetchMock.mockReset(); vi.stubGlobal('fetch', fetchMock);
  media.matches = false; media.addEventListener.mockClear(); vi.stubGlobal('matchMedia', () => media);
  document.documentElement.removeAttribute('data-theme'); document.documentElement.style.colorScheme = '';
  const page = new DOMParser().parseFromString(html, 'text/html');
  document.head.innerHTML = page.head.innerHTML; document.body.innerHTML = page.body.innerHTML;
  history.replaceState(null, '', '/login.html?expired=1'); localStorage.clear();
  await import('../src/login');
});
afterEach(() => vi.unstubAllGlobals());

it('opens with the saved appearance and follows appearance changes from another window', async () => {
  localStorage.setItem('miaoyan-editor.preferences.v1', JSON.stringify({ version: 1, appearance: 'dark' }));
  vi.resetModules(); await import('../src/login');
  expect(document.documentElement.dataset.theme).toBe('dark');
  expect(document.documentElement.style.colorScheme).toBe('dark');
  window.dispatchEvent(new StorageEvent('storage', {
    key: 'miaoyan-editor.preferences.v1', newValue: JSON.stringify({ version: 1, appearance: 'light' }),
  }));
  expect(document.documentElement.dataset.theme).toBe('light');
  expect(fetchMock).not.toHaveBeenCalled();
});

it('follows system appearance without writing preferences or changing the password', () => {
  password().value = 'synthetic-unsent-password';
  expect(document.documentElement.dataset.theme).toBe('light');
  media.matches = true;
  const change = media.addEventListener.mock.calls.filter(([name]) => name === 'change').at(-1)?.[1];
  expect(change).toBeTypeOf('function'); change();
  expect(document.documentElement.dataset.theme).toBe('dark');
  media.matches = false; change();
  expect(document.documentElement.dataset.theme).toBe('light');
  expect(password().value).toBe('synthetic-unsent-password');
  expect(localStorage.length).toBe(0); expect(fetchMock).not.toHaveBeenCalled();
});

it('keeps an explicit light appearance when the system is dark', async () => {
  media.matches = true;
  localStorage.setItem('miaoyan-editor.preferences.v1', JSON.stringify({ version: 1, appearance: 'light' }));
  vi.resetModules(); await import('../src/login');
  expect(document.documentElement.dataset.theme).toBe('light');
  expect(document.documentElement.style.colorScheme).toBe('light');
});

it('only asks for a password, shows expiry and prevents empty submission', () => {
  expect(document.querySelectorAll('input')).toHaveLength(1);
  expect(document.querySelector<HTMLElement>('#session-notice')!.hidden).toBe(false);
  submit(); expect(fetchMock).not.toHaveBeenCalled();
  expect(password().getAttribute('aria-invalid')).toBe('true');
  expect(document.querySelector('#password-help')!.textContent).toContain('请输入');
  document.querySelector<HTMLButtonElement>('#password-visibility')!.click();
  expect(password().type).toBe('text');
  expect(document.querySelector('#password-visibility')!.getAttribute('aria-label')).toBe('隐藏密码');
});

it('sends the password only to the same-origin login endpoint and handles rejection without storing it', async () => {
  fetchMock.mockResolvedValue({ status: 401 }); password().value = 'synthetic-wrong-password';
  submit();
  await vi.waitFor(() => expect(message()).toContain('密码不正确'));
  expect(fetchMock).toHaveBeenCalledExactlyOnceWith('/auth/login', {
    method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password: 'synthetic-wrong-password' }),
  });
  expect(localStorage.length).toBe(0);
  expect(password().getAttribute('aria-invalid')).toBe('true');
  password().dispatchEvent(new Event('input'));
  expect(password().hasAttribute('aria-invalid')).toBe(false);
  expect(message()).toBe(''); expect(button().disabled).toBe(false);
});

it('blocks duplicate submissions while pending and restores the form after a rate limit', async () => {
  let complete!: (value: { status: number }) => void;
  fetchMock.mockReturnValue(new Promise(resolve => { complete = resolve; }));
  password().value = 'synthetic-password'; submit(); submit();
  expect(fetchMock).toHaveBeenCalledOnce(); expect(button().disabled).toBe(true);
  expect(button().getAttribute('aria-busy')).toBe('true');
  complete({ status: 429 });
  await vi.waitFor(() => expect(message()).toContain('一分钟'));
  expect(button().disabled).toBe(false); expect(button().getAttribute('aria-busy')).toBe('false');
});

it('allows retry after server and network failures', async () => {
  password().value = 'synthetic-password';
  fetchMock.mockResolvedValueOnce({ status: 503 }); submit();
  await vi.waitFor(() => expect(message()).toContain('暂时无法登录'));
  fetchMock.mockRejectedValueOnce(new TypeError('offline')); submit();
  await vi.waitFor(() => expect(message()).toContain('无法连接服务'));
  expect(button().disabled).toBe(false); expect(localStorage.length).toBe(0);
});
