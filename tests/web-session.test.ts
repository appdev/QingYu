// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { requireWebSession, watchWebSession } from '../src/web-session';

const fetchMock = vi.fn();
beforeEach(() => {
  vi.useFakeTimers(); fetchMock.mockReset(); vi.stubGlobal('fetch', fetchMock);
  document.head.innerHTML = ''; document.body.innerHTML = '<button id="logout-button" hidden>退出登录</button>';
  vi.spyOn(window, 'addEventListener').mockImplementation(() => {});
});
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
function enable() { document.head.innerHTML = '<meta name="qingyu-web-auth" content="enabled">'; }

it('leaves native and static development entries alone and verifies server-hosted sessions before startup', async () => {
  await requireWebSession(); watchWebSession(vi.fn(), vi.fn());
  expect(fetchMock).not.toHaveBeenCalled(); expect(document.querySelector<HTMLButtonElement>('button')!.hidden).toBe(true);
  enable(); fetchMock.mockResolvedValue({ status: 204 }); await requireWebSession();
  expect(fetchMock).toHaveBeenCalledWith('/auth/session', { credentials: 'same-origin', cache: 'no-store' });
});

it('stops initialization when session verification is unavailable', async () => {
  enable(); fetchMock.mockResolvedValue({ status: 503 });
  await expect(requireWebSession()).rejects.toThrow('无法验证');
  expect(document.body.textContent).toContain('检查网络后刷新');
});

it('defers logout and expired-session navigation while a draft cannot be saved', async () => {
  enable(); const save = vi.fn(async () => false), error = vi.fn();
  watchWebSession(save, error);
  document.querySelector<HTMLButtonElement>('button')!.click();
  await Promise.resolve(); expect(save).toHaveBeenCalledOnce(); expect(fetchMock).not.toHaveBeenCalled();
  fetchMock.mockResolvedValue({ status: 401 });
  await vi.advanceTimersByTimeAsync(60_000);
  expect(save).toHaveBeenCalledTimes(2); expect(fetchMock).toHaveBeenCalledTimes(1); expect(error).not.toHaveBeenCalled();
});

it('saves before logout and keeps the page when revocation fails', async () => {
  enable(); const order: string[] = [], error = vi.fn();
  fetchMock.mockImplementation(async () => { order.push('logout'); return { status: 503 }; });
  watchWebSession(async () => { order.push('save'); return true; }, error);
  document.querySelector<HTMLButtonElement>('button')!.click();
  await vi.advanceTimersByTimeAsync(0);
  expect(order).toEqual(['save', 'logout']);
  expect(error).toHaveBeenCalledWith('退出登录失败，请稍后重试');
  expect(fetchMock).toHaveBeenCalledWith('/auth/logout', { method: 'POST', credentials: 'same-origin' });
});
