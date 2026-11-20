import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export async function setupWorkspace(windowLabel: string, actions: {
  save(): Promise<boolean>;
  select?(path: string): Promise<void>;
  error(message: string): void;
}) {
  const documentWindow = windowLabel !== 'main';
  const target = { target: windowLabel };
  const back = document.querySelector<HTMLButtonElement>('#workspace-back')!;
  back.hidden = !documentWindow;
  let opening = false, closing = false, quitting = false, exitId: string | undefined;
  let saving: Promise<boolean> | undefined;
  let selecting: Promise<void> | undefined;
  const paths: string[] = [];
  const queue: (string | undefined)[] = [];
  function save() {
    return saving ??= (async () => {
      while (document.body.dataset.libraryBusy === 'true' || document.body.dataset.syncBusy === 'true') await new Promise(resolve => setTimeout(resolve, 50));
      return actions.save();
    })().finally(() => { saving = undefined; });
  }
  async function open() {
    if (opening) return;
    opening = true;
    try {
      while (queue.length) {
        const path = queue.shift();
        try { await invoke('workspace_open', { path }); }
        catch (reason) { actions.error(reason instanceof Error ? reason.message : String(reason)); }
      }
    } finally { opening = false; }
  }
  function selectPending(): Promise<void> {
    if (selecting) return selecting;
    if (closing || quitting || !paths.length) return Promise.resolve();
    document.body.inert = true;
    selecting = (async () => {
      while (paths.length && !closing && !quitting) {
        const path = paths.shift()!;
        if (!await save()) { paths.length = 0; actions.error('当前文档尚未保存，请完成输入或处理保存错误后重新打开文件'); return; }
        await actions.select?.(path);
      }
    })().catch(reason => actions.error(reason instanceof Error ? reason.message : String(reason)))
      .finally(() => { selecting = undefined; if (!closing && !quitting) { document.body.inert = false; if (paths.length) void selectPending(); } });
    return selecting;
  }
  async function receivePaths() {
    try { paths.push(...await invoke<string[]>('workspace_paths')); await selectPending(); }
    catch (reason) { actions.error(String(reason)); }
  }
  async function showLibrary() {
    if (closing || quitting) return;
    document.body.inert = true;
    try { await selecting; document.body.inert = true; if (await save()) await invoke('workspace_show_library'); }
    catch (reason) { actions.error(String(reason)); }
    finally { if (!closing && !quitting) document.body.inert = false; }
  }
  async function close() {
    if (closing || quitting) return;
    closing = true; document.body.inert = true;
    try {
      await selecting;
      if (!await save()) { actions.error('当前文档尚未保存，请完成输入或处理保存错误后重试'); return; }
      await invoke('workspace_finish_close');
    } catch (reason) { actions.error(String(reason)); }
    finally { closing = false; if (!quitting) document.body.inert = false; }
  }
  async function prepareExit(id: string) {
    if (exitId === id) return;
    exitId = id; quitting = true; document.body.inert = true;
    try {
      await selecting;
      const saved = await save();
      if (!saved) actions.error('当前文档尚未保存，已取消退出');
      await invoke('workspace_exit_reply', { id, saved });
    } catch (reason) {
      actions.error(String(reason));
      await invoke('workspace_exit_reply', { id, saved: false }).catch(() => {});
    }
  }
  await listen<string>('library-command', ({ payload }) => {
    if (payload === 'workspace-open') { queue.push(undefined); return open(); }
    if (payload === 'workspace-close') return showLibrary();
  }, target);
  if (documentWindow) {
    back.onclick = () => { void showLibrary(); };
    await listen('workspace-before-close', close, target);
    await listen<string>('workspace-before-exit', ({ payload }) => prepareExit(payload), target);
    await listen('workspace-exit-cancelled', () => { quitting = false; exitId = undefined; document.body.inert = false; void selectPending(); }, target);
    if (actions.select) await listen('workspace-select-request', receivePaths, target);
    const pending = await invoke<string | null>('workspace_ready');
    if (pending) await prepareExit(pending);
    if (actions.select) await receivePaths();
  }
}
