import { isolateHistory } from '@codemirror/commands';
import { ChangeSet, Text, type ChangeSpec } from '@codemirror/state';
import type { EditorView } from '@codemirror/view';
import { invoke } from '@tauri-apps/api/core';
import { diffVersionLines } from './note-version-diff';
import { isServerLibrary } from './server-library';

/** 排版优化写回编辑器时用的事件名，提示条据此显示「排版已优化」。 */
export const typographyEvent = 'format.typography';

const native = () => '__TAURI_INTERNALS__' in window;

/** 中英文空格和标点在 Rust 端（AutoCorrect）做，App 走命令，网页版走服务器；纯浏览器笔记库没有后端，跳过这一步。 */
async function cleanTypography(source: string) {
  if (!native() && !isServerLibrary()) return source;
  if (native()) return invoke<string>('typography_clean', { source });
  const response = await fetch('/api/web/typography', {
    method: 'POST', credentials: 'same-origin', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source }),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return await response.json() as string;
}

let worker: Worker | undefined;
let nextRequest = 0;
const waiting = new Map<number, { resolve: (source: string) => void; reject: (error: Error) => void }>();
async function formatDocument(source: string) {
  if (typeof Worker === 'undefined') return (await import('./typography-format')).formatMarkdown(source);
  worker ??= Object.assign(new Worker(new URL('./markdown-format-worker.ts', import.meta.url), { type: 'module' }), {
    onmessage({ data }: MessageEvent<{ id: number; source?: string; error?: string }>) {
      const request = waiting.get(data.id);
      waiting.delete(data.id);
      if (data.source !== undefined) request?.resolve(data.source);
      else request?.reject(new Error(data.error));
    },
    onerror(event: ErrorEvent) {
      // Worker 加载失败时，正在等的请求都要结束，下次再重新创建。
      for (const request of waiting.values()) request.reject(new Error(event.message));
      waiting.clear();
      worker?.terminate();
      worker = undefined;
    },
  });
  const id = ++nextRequest;
  return new Promise<string>((resolve, reject) => {
    waiting.set(id, { resolve, reject });
    worker!.postMessage({ id, source });
  });
}

/**
 * 把整篇替换拆成按行的小改动：没变的行不动，变了的行只换前后不同的那一段。
 * 这样光标、选区和滚动位置都跟着原来的文字走，撤销也只记真正改过的地方。
 */
export function documentChanges(before: string, after: string): ChangeSpec[] {
  const changes: { from: number; to: number; insert: string }[] = [];
  const lines = diffVersionLines(before, after);
  let at = 0;
  for (let index = 0; index < lines.length;) {
    if (!lines[index].change) { at += lines[index++].text.length + 1; continue; }
    const removed: string[] = [], added: string[] = [];
    for (; index < lines.length && lines[index].change; index++) (lines[index].change === 'removed' ? removed : added).push(lines[index].text);
    const last = index === lines.length;
    if (removed.length && added.length) {
      // 行数相同就逐行比；行数不同（比如插入了空行）就整段比。两种都只换掉中间真正不同的部分。
      const pairs = removed.length === added.length ? removed.map((old, row) => [old, added[row]]) : [[removed.join('\n'), added.join('\n')]];
      for (const [old, next] of pairs) {
        let start = 0, end = 0;
        while (start < old.length && start < next.length && old[start] === next[start]) start++;
        while (end < old.length - start && end < next.length - start && old[old.length - 1 - end] === next[next.length - 1 - end]) end++;
        changes.push({ from: at + start, to: at + old.length - end, insert: next.slice(start, next.length - end) });
        at += old.length + 1;
      }
    } else if (added.length) {
      // 文末插入时换行在新内容前面，其他位置在后面。
      changes.push({ from: last ? at - 1 : at, to: last ? at - 1 : at, insert: last ? '\n' + added.join('\n') : added.join('\n') + '\n' });
    } else {
      const length = removed.join('\n').length;
      changes.push(last ? { from: at - 1, to: at + length, insert: '' } : { from: at, to: at + length + 1, insert: '' });
      at += length + 1;
    }
  }
  // 拆分结果对不上时宁可整篇替换，也不能把正文改错。
  try {
    if (ChangeSet.of(changes, before.length).apply(Text.of(before.split('\n'))).toString() === after) return changes;
  } catch { /* 落到整篇替换 */ }
  return [{ from: 0, to: before.length, insert: after }];
}

let running = false;

/**
 * 排版优化：先修中英文空格和标点，再按 Prettier 统一 Markdown 格式。顺序不能反，表格列宽要在加完空格后再对齐。
 * 只在用户手动触发时执行。期间正文有任何改动（打字、切换笔记、同步写入），结果一律丢弃，不会盖掉新内容。
 * 改动作为一步写进撤销记录，提示条上可以直接撤销。
 */
export async function optimizeTypography(view: EditorView, notify: (message: '排版无需调整' | '正文已改动，已取消排版' | '排版失败', error?: boolean) => void) {
  if (running || view.state.readOnly || view.compositionStarted) return;
  running = true;
  const before = view.state.doc;
  const source = before.toString();
  try {
    let next: string;
    try {
      next = await formatDocument(await cleanTypography(source));
    } catch (error) {
      console.warn(error);
      notify('排版失败', true);
      return;
    }
    if (view.state.doc !== before || view.state.readOnly || view.compositionStarted) {
      if (view.dom.isConnected) notify('正文已改动，已取消排版');
      return;
    }
    if (next === source) {
      notify('排版无需调整');
      return;
    }
    view.dispatch({
      changes: documentChanges(source, next),
      userEvent: typographyEvent,
      annotations: isolateHistory.of('full'),
    });
  } finally {
    running = false;
  }
}
