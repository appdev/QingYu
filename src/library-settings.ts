import { emitTo, listen } from '@tauri-apps/api/event';

export async function libraryLocation(change = false): Promise<string> {
  const id = crypto.randomUUID();
  let finish!: (value: string) => void, fail!: (error: Error) => void;
  const result = new Promise<string>((resolve, reject) => { finish = resolve; fail = reject; });
  const unlisten = await listen<{ id: string; root?: string; error?: string }>('settings-library-reply', ({ payload }) => {
    if (payload.id !== id) return;
    if (payload.error) fail(new Error(payload.error)); else finish(payload.root ?? '');
  }, { target: 'settings' });
  const timer = change ? undefined : setTimeout(() => fail(new Error('主窗口未响应，请打开主窗口后重试')), 10000);
  try { await emitTo('main', 'settings-library-request', { id, change }); return await result; }
  finally { clearTimeout(timer); unlisten(); }
}
