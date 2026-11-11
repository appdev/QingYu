import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Notebook } from './notebook';

export async function setupWorkspace(library: Notebook, actions: {
  current(): Notebook;
  save(): Promise<boolean>;
  activate(book: Notebook): Promise<void>;
  error(message: string): void;
}) {
  const back = document.querySelector<HTMLButtonElement>('#workspace-back')!;
  let running = false, timer: ReturnType<typeof setTimeout> | undefined;
  const queue: (string | undefined)[] = [];
  const busy = () => document.body.inert || document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true';
  async function restore() {
    if (actions.current() === library) return;
    await invoke('workspace_close');
    await actions.activate(library); back.hidden = true;
  }
  async function run(close = false) {
    if (running || busy()) { if (!close) timer ??= setTimeout(() => { timer = undefined; void run(); }, 250); return; }
    if (!close && !queue.length) return;
    running = true; document.body.dataset.libraryBusy = 'true';
    const shell = document.querySelector<HTMLElement>('#app-shell'); if (shell) shell.inert = true;
    try {
      if (!await actions.save()) { queue.length = 0; return; }
      if (close) { await restore(); return; }
      const path = queue.shift();
      const external = new Notebook({ getItem: () => null, setItem: () => { throw new Error('临时文件不能写入笔记库'); } }, '');
      if (await external.openExternal(path)) { await actions.activate(external); back.hidden = false; }
    } catch (reason) { actions.error(reason instanceof Error ? reason.message : String(reason)); }
    finally {
      running = false; document.body.dataset.libraryBusy = 'false';
      if (shell) shell.inert = document.body.dataset.syncBusy === 'true';
      if (queue.length) void run();
    }
  }
  async function receive() {
    try { queue.push(...await invoke<string[]>('workspace_requests')); await run(); }
    catch (reason) { actions.error(String(reason)); }
  }
  await listen('workspace-request', receive);
  await listen<string>('library-command', ({ payload }) => {
    if (payload === 'workspace-open') { queue.push(undefined); void run(); }
    else if (payload === 'workspace-close') void run(true);
  });
  back.onclick = () => { void run(true); };
  await receive();
  // Library import/location changes already own the UI lock and have saved the active draft.
  return { restore };
}
