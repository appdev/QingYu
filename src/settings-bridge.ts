import { emit, emitTo, listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { mobile, native, preferences, type Preferences } from './preferences';
import type { SyncClient, SyncStatus } from './sync-session';
import type { S3Settings } from './sync-store';

export async function connectPreferences() {
  if (!native) return;
  let receiving = false;
  await listen<Preferences>('preferences-changed', ({ payload }) => {
    try { receiving = true; preferences.receive(payload); } catch { showNativeError(); } finally { receiving = false; }
  });
  const apply = () => invoke('apply_preferences', { appearance: preferences.value.appearance, alwaysOnTop: preferences.value.alwaysOnTop, noteTextSize: preferences.value.previewFontSize });
  preferences.subscribe(() => { void apply().catch(showNativeError); if (!receiving) void emit('preferences-changed', preferences.value).catch(showNativeError); });
  await apply();
}
function showNativeError() { window.dispatchEvent(new CustomEvent('settings-error', { detail: '设置已保存，但系统窗口未能更新，请重新打开应用' })); }

type Request = { id: string; action: 'load' | 'save' | 'flush' | 'sync'; config?: S3Settings; accountId?: string };
type Reply = { id: string; config?: S3Settings; error?: string };

export function retrySettingsRead(send: () => void) {
  // Opening settings before the main window has registered its listener can drop the first request.
  const interval = setInterval(send, 300);
  send();
  return () => clearInterval(interval);
}

export async function hostSync(client: SyncClient) {
  if (!native || mobile) return;
  await listen<Request>('settings-sync-request', async ({ payload }) => {
    const { id, action, config, accountId } = payload;
    if (typeof id !== 'string' || !['load', 'save', 'flush', 'sync'].includes(action)) return;
    const reply: Reply = { id };
    try {
      if (action === 'load') {
        reply.config = await client.load();
        const unsubscribe = client.subscribe(status => { void emitTo('settings', 'settings-sync-status', status); }); unsubscribe();
      }
      else if (action === 'flush') await client.flush();
      else {
        if (!config || typeof config !== 'object' || (accountId !== undefined && typeof accountId !== 'string')) throw new Error('连接设置无效');
        if (action === 'save') await client.save(config); else await client.run(config, accountId);
      }
    } catch (error) { reply.error = error instanceof Error ? error.message : '同步未完成'; }
    await emitTo('settings', 'settings-sync-reply', reply);
  }, { target: 'main' });
  client.subscribe(status => { void emitTo('settings', 'settings-sync-status', status); });
}
export async function remoteSync(): Promise<SyncClient> {
  const pending = new Map<string, { resolve: (config?: S3Settings) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const listeners = new Set<(status: SyncStatus) => void>();
  let status: SyncStatus = { busy: false, message: '', error: false };
  await listen<Reply>('settings-sync-reply', ({ payload }) => {
    const request = pending.get(payload.id); if (!request) return;
    clearTimeout(request.timer); pending.delete(payload.id);
    if (payload.error) request.reject(new Error(payload.error)); else request.resolve(payload.config);
  }, { target: 'settings' });
  await listen<SyncStatus>('settings-sync-status', ({ payload }) => { status = payload; for (const listener of listeners) listener(status); }, { target: 'settings' });
  function request(action: Request['action'], config?: S3Settings, accountId?: string): Promise<S3Settings | undefined> {
    let stopRetry: (() => void) | undefined;
    return new Promise<S3Settings | undefined>((resolve, reject) => {
      const id = crypto.randomUUID();
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('主窗口未响应；请保留当前内容，打开主窗口后重试')); }, action === 'load' ? 10000 : 300000);
      pending.set(id, { resolve, reject, timer });
      const send = () => { void emitTo('main', 'settings-sync-request', { id, action, config, accountId }).catch(() => {
        clearTimeout(timer); pending.delete(id); reject(new Error('无法连接主窗口'));
      }); };
      if (action === 'load') stopRetry = retrySettingsRead(send);
      else send();
    }).finally(() => stopRetry?.());
  }
  return {
    load: async () => { const config = await request('load'); if (!config) throw new Error('无法读取同步设置'); return config; },
    save: async config => { await request('save', config); },
    flush: async () => { await request('flush'); },
    run: async (config, accountId) => { await request('sync', config, accountId); },
    subscribe(listener) { listeners.add(listener); listener(status); return () => { listeners.delete(listener); }; },
  };
}
