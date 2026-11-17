import { emitTo, listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { DailyNoteChoices } from './daily-note';
import { libraryKey, type Library } from './notebook';
import { isServerLibrary } from './server-library';
import { retrySettingsRead } from './settings-bridge';

export type StorageUsage = { notes: number; files: number; trash: number };

export function formatStorageBytes(bytes: number | undefined) {
  if (bytes === undefined || !Number.isFinite(bytes) || bytes < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  let unit = 0;
  while (bytes >= 1000 && unit < units.length - 1) { bytes /= 1000; unit++; }
  return bytes.toLocaleString('zh-CN', { maximumFractionDigits: unit ? 1 : 0 }) + ' ' + units[unit];
}

export async function libraryStorageUsage(): Promise<StorageUsage> {
  if ('__TAURI_INTERNALS__' in window) return invoke('library_storage_usage');
  if (isServerLibrary()) {
    const response = await fetch('/api/web/storage-usage');
    if (!response.ok) throw new Error('无法读取服务器笔记库用量');
    return response.json();
  }
  const usage = { notes: 0, files: 0, trash: 0 };
  const saved = localStorage.getItem(libraryKey);
  if (saved) {
    const { notes } = JSON.parse(saved) as Library;
    const encoder = new TextEncoder();
    for (const note of notes) usage[note.trashed ? 'trash' : 'notes'] += encoder.encode(note.source).length;
  }
  return usage;
}

type LibraryReply = { id: string; root?: string; daily?: DailyNoteChoices; error?: string };
async function libraryRequest(change: boolean, daily = false): Promise<LibraryReply> {
  const id = crypto.randomUUID();
  let finish!: (value: LibraryReply) => void, fail!: (error: Error) => void;
  const result = new Promise<LibraryReply>((resolve, reject) => { finish = resolve; fail = reject; });
  const unlisten = await listen<LibraryReply>('settings-library-reply', ({ payload }) => {
    if (payload.id !== id) return;
    if (payload.error) fail(new Error(payload.error)); else finish(payload);
  }, { target: 'settings' });
  const timer = change ? undefined : setTimeout(() => fail(new Error('主窗口未响应，请打开主窗口后重试')), 10000);
  const send = () => { void emitTo('main', 'settings-library-request', { id, change, ...(daily ? { daily } : {}) }).catch(fail); };
  const stopRetry = change ? undefined : retrySettingsRead(send);
  if (change) send();
  try { return await result; }
  finally { stopRetry?.(); clearTimeout(timer); unlisten(); }
}
export async function libraryLocation(change = false) {
  return (await libraryRequest(change)).root ?? '';
}
export async function libraryDailyChoices() {
  const { daily } = await libraryRequest(false, true);
  if (!daily) throw new Error('无法读取今日笔记的文件夹');
  return daily;
}
