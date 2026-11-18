import { noteText, noteTitle } from './document-text';
import type { Note } from './notebook';

// 菜单栏面板列什么：不打字时是置顶和最近的笔记，打字时是匹配的笔记。规则照 Pinto 的 MenuBarList。

export const pinnedLimit = 5, recentLimit = 6, searchLimit = 8;

export type MenuBarNote = { id: string; title: string; summary: string; pinned: boolean };
export type MenuBarSections = { pinned: MenuBarNote[]; recent: MenuBarNote[]; matches: MenuBarNote[] };

export type MenuBarRequest =
  | { id: string; action: 'list'; query: string }
  | { id: string; action: 'create'; text: string }
  | { id: string; action: 'open'; noteId: string };
export type MenuBarReply = { id: string; sections?: MenuBarSections; created?: { id: string; title: string }; error?: string };

function row(note: Note): MenuBarNote {
  const text = noteText(note.source);
  return { id: note.id, title: noteTitle(note), summary: text.summary, pinned: Boolean(note.pinned) };
}

export function menuBarSections(notes: readonly Note[], query: string): MenuBarSections {
  const newest = notes.filter(note => !note.trashed).sort((a, b) => b.updatedAt - a.updatedAt);
  const q = query.trim().toLocaleLowerCase();
  if (q) {
    // 标题命中的排前面，其次是正文命中的；各自从新到旧。
    const inTitle = newest.filter(note => noteTitle(note).toLocaleLowerCase().includes(q));
    const inBody = newest.filter(note => !inTitle.includes(note) && note.source.toLocaleLowerCase().includes(q));
    return { pinned: [], recent: [], matches: [...inTitle, ...inBody].slice(0, searchLimit).map(row) };
  }
  return {
    pinned: newest.filter(note => note.pinned).slice(0, pinnedLimit).map(row),
    recent: newest.filter(note => !note.pinned).slice(0, recentLimit).map(row),
    matches: [],
  };
}

/** 面板里打的字存成笔记：去掉首尾空白，第一行是标题。没有内容时不存。 */
export function quickCapture(text: string) {
  const source = text.trim();
  if (!source) return undefined;
  const title = source.split(/\r?\n/, 1)[0].replace(/^#{1,6}\s+/, '').trim().slice(0, 80);
  return { title, source };
}
