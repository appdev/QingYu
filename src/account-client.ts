import { invoke } from '@tauri-apps/api/core';
import { native } from './preferences';
import { isServerLibrary } from './server-library';

// 轻羽账号的调用入口。账号逻辑都在 Rust 的 client_account 里：
// App 通过 invoke 调用，网页客户端通过部署端 qingyu-api 的 /api/web/account 调用，失败时拿到的错误值相同。

type Commands = {
  account_status: { force?: boolean };
  account_login: { username: string; password: string; rememberDevice: boolean };
  account_logout: { accountId: string; everywhere: boolean };
  account_redeem: { accountId: string; activationCode: string };
  account_redemptions: { accountId: string; force?: boolean };
};

/** 是否能使用轻羽账号：App，或者由 qingyu-api 提供的网页客户端。 */
export const accountAvailable = () => native || isServerLibrary();

async function request<T>(path: string, body?: object): Promise<T> {
  const response = await fetch(`/api/web/account${path}`, body
    ? { method: 'POST', cache: 'no-store', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }
    : { cache: 'no-store' });
  const value = await response.json().catch(() => ({ error: 'network' }));
  if (!response.ok) throw value?.error ?? 'network';
  return value as T;
}

export function callAccount<T, K extends keyof Commands = keyof Commands>(command: K, args?: Commands[K]): Promise<T> {
  if (!isServerLibrary()) return invoke<T>(command, args);
  const query = (values: Record<string, string | boolean | undefined>) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(values)) if (value !== undefined && value !== false) params.set(key, String(value));
    const text = params.toString();
    return text ? `?${text}` : '';
  };
  const values = (args ?? {}) as Record<string, string | boolean | undefined>;
  switch (command) {
    case 'account_status': return request(query({ force: values.force }));
    case 'account_login': return request('/login', values);
    case 'account_logout': return request('/logout', values);
    case 'account_redeem': return request('/redeem', values);
    case 'account_redemptions': return request(`/redemptions${query({ accountId: values.accountId, force: values.force })}`);
  }
  return Promise.reject('invalid_input');
}

/** 打开轻羽门户。网页客户端在新标签页打开，必须在点击时同步调用，否则会被浏览器拦截。 */
export function openPortal(page: string): Promise<void> {
  if (!isServerLibrary()) return invoke('account_open_portal', { page });
  window.open(`/api/web/account/portal?page=${encodeURIComponent(page)}`, '_blank', 'noopener');
  return Promise.resolve();
}
