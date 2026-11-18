import { emitTo, listen } from '@tauri-apps/api/event';
import { menuBarSections, quickCapture, type MenuBarReply, type MenuBarRequest } from './menu-bar-model';
import type { Notebook } from './notebook';

// 主窗口替菜单栏面板干活：列笔记、新建、打开。笔记库只在主窗口里写，面板不直接碰。

export type MenuBarActions = {
  /** 当前能不能动笔记库；不能时返回原因。 */
  blocked(): string | undefined;
  create(title: string, source: string): Promise<{ id: string; title: string }>;
  open(id: string): Promise<void>;
};

export async function answerMenuBar(book: Notebook, actions: MenuBarActions, request: MenuBarRequest): Promise<MenuBarReply> {
  const reply: MenuBarReply = { id: request.id };
  try {
    if (request.action === 'list') {
      reply.sections = menuBarSections(book.loadError ? [] : book.data.notes, request.query);
      return reply;
    }
    const reason = actions.blocked();
    if (reason) throw new Error(reason);
    if (request.action === 'create') {
      const note = quickCapture(request.text);
      if (!note) throw new Error('先写点内容');
      reply.created = await actions.create(note.title, note.source);
    } else {
      if (!book.data.notes.some(note => note.id === request.noteId && !note.trashed)) throw new Error('这篇笔记已不存在');
      await actions.open(request.noteId);
    }
  } catch (error) { reply.error = error instanceof Error ? error.message : String(error); }
  return reply;
}

export async function hostMenuBar(book: () => Notebook, actions: MenuBarActions) {
  // 一次只处理一个请求，连按回车也不会同时建两篇。
  let queue = Promise.resolve();
  await listen<MenuBarRequest>('menu-bar-request', ({ payload }) => {
    if (!payload || typeof payload.id !== 'string' || !['list', 'create', 'open'].includes(payload.action)) return;
    queue = queue.then(async () => { await emitTo('menubar', 'menu-bar-reply', await answerMenuBar(book(), actions, payload)); }).catch(() => {});
  }, { target: 'main' });
}
