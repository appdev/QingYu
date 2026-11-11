import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { EditorBuffer, Note, Notebook } from './notebook';

export async function setupFileWatch(book: Notebook, actions: {
  pending(): boolean;
  buffer(): EditorBuffer | undefined;
  refresh(previous?: Note): void;
  backedUp(paths: string[]): void;
  error(message: string): void;
}, windowLabel = 'main') {
  let timer: ReturnType<typeof setTimeout> | undefined, pendingRoot: string | undefined, running = false, stopped = false;
  function schedule(root: string, delay = 150) {
    pendingRoot = root;
    if (running || stopped) return;
    clearTimeout(timer); timer = setTimeout(check, delay);
  }
  async function check() {
    const root = pendingRoot;
    if (stopped || !root) return;
    pendingRoot = undefined;
    if (root !== book.root) return;
    // MiaoYan keeps one 1.6s deferred recheck while a save/composition is pending.
    if (document.body.inert || document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true' || actions.pending()) { schedule(root, 1600); return; }
    running = true;
    document.body.dataset.libraryBusy = 'true';
    const shell = document.querySelector<HTMLElement>('#app-shell'); if (shell) shell.inert = true;
    let delay = 150;
    try {
      const previous = book.current, result = await book.reconcile(root, actions.buffer());
      if (result.changed) actions.refresh(previous);
      if (result.backups.length) actions.backedUp(result.backups);
    } catch (reason) {
      actions.error(reason instanceof Error ? reason.message : String(reason));
      pendingRoot = root; delay = 1600;
    } finally {
      running = false; document.body.dataset.libraryBusy = 'false';
      if (shell) shell.inert = document.body.dataset.syncBusy === 'true';
      if (pendingRoot) schedule(pendingRoot, delay);
    }
  }
  const prefix = book.external ? 'workspace' : 'library';
  const unlisten = await listen<string>(`${prefix}-files-changed`, ({ payload }) => { if (typeof payload === 'string' && payload === book.root) schedule(payload); }, { target: windowLabel });
  const unlistenError = await listen<{ root: string; error: string }>(`${prefix}-watch-error`, ({ payload }) => { if (payload.root === book.root) actions.error(payload.error); }, { target: windowLabel });
  try { if (book.root && !book.external) await invoke('library_watch'); }
  catch (reason) { actions.error(`文件监听未启动：${reason instanceof Error ? reason.message : String(reason)}`); }
  if (book.root && book.external) schedule(book.root);
  return () => { stopped = true; clearTimeout(timer); unlisten(); unlistenError(); };
}
