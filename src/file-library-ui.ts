import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import type { Notebook } from './notebook';

export async function setupFileLibrary(book: Notebook, actions: { save(): Promise<boolean>; prepareExit?(): Promise<void>; cancelExit?(): Promise<void>; sync?(): Promise<void>; refresh(): void | Promise<void>; error(message: string): void }) {
  let quitting = false;
  let changing = false;
  async function command(payload: string) {
    if (!['library-choose', 'library-import', 'library-import-folder', 'library-reload'].includes(payload)) return;
    if (quitting || changing || document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') throw new Error('笔记库正在处理其他操作，请完成后重试');
    changing = true; document.body.dataset.libraryBusy = 'true';
    const shell = document.querySelector<HTMLElement>('#app-shell'); if (shell) shell.inert = true;
    try {
      if (!book.loadError && !await actions.save()) throw new Error('当前输入尚未保存，请完成输入后重试');
      if (payload === 'library-choose') await book.chooseRoot();
      else if (payload === 'library-import') await book.importFiles();
      else if (payload === 'library-import-folder') await book.importFiles(true);
      else if (payload === 'library-reload') await book.reload();
      else return;
      await actions.refresh();
      if (payload === 'library-choose' || payload === 'library-reload') await invoke('library_watch');
    } finally {
      changing = false; document.body.dataset.libraryBusy = 'false';
      if (shell) shell.inert = document.body.dataset.syncBusy === 'true';
    }
  }
  await listen<string>('library-command', async ({ payload }) => {
    try { await command(payload); } catch (error) { actions.error(error instanceof Error ? error.message : String(error)); }
  }, { target: 'main' });
  await listen<{ id: string; change: boolean }>('settings-library-request', async ({ payload }) => {
    if (typeof payload.id !== 'string' || typeof payload.change !== 'boolean') return;
    try {
      if (payload.change) await command('library-choose');
      await emitTo('settings', 'settings-library-reply', { id: payload.id, root: book.root ?? '' });
    } catch (error) { await emitTo('settings', 'settings-library-reply', { id: payload.id, error: error instanceof Error ? error.message : String(error) }); }
  }, { target: 'main' });
  await listen('library-close-main', async () => {
    if (quitting || changing) return;
    changing = true; document.body.inert = true;
    try { if (!book.loadError && !await actions.save()) return; await invoke('library_hide_main'); }
    catch (reason) { actions.error(String(reason)); }
    finally { changing = false; document.body.inert = false; }
  }, { target: 'main' });
  await listen('library-before-exit', async () => {
    if (quitting) return;
    quitting = true; document.body.inert = true;
    while (changing || document.body.dataset.libraryBusy === 'true') await new Promise(resolve => setTimeout(resolve, 50));
    try {
      if (!book.loadError && !await actions.save()) return;
      await actions.prepareExit?.();
      if (!book.loadError && actions.sync) {
        let timeout: ReturnType<typeof setTimeout> | undefined;
        try {
          await Promise.race([actions.sync().catch(() => {}), new Promise<void>(resolve => { timeout = setTimeout(resolve, 3000); })]);
        } catch { /* Local data is saved; sync failures must never prevent quitting. */ }
        finally { clearTimeout(timeout); }
      }
      await invoke('library_finish_exit');
    } catch (reason) { await actions.cancelExit?.().catch(() => {}); actions.error(reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : '退出失败'); }
    finally { document.body.inert = false; quitting = false; }
  }, { target: 'main' });
}
