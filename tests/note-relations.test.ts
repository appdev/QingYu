import { expect, it } from 'vitest';
import { Notebook, type Note } from '../src/notebook';
import { findNoteRelations, linkMentionLine, sameRelationInputs } from '../src/note-relations';

const signal = () => new AbortController().signal;
const makeBook = (source: string) => {
  const values = new Map<string, string>();
  return { storage: values, book: new Notebook({ getItem: key => values.get(key) ?? null, setItem: (key, data) => { values.set(key, data); } }, source) };
};
const otherNote = (base: Note, title: string, source: string): Note => ({ ...base, id: title, title, source });

it('finds actual incoming links, skips protected syntax, and locates mentions by original Markdown line', async () => {
  const { book } = makeBook('---\r\naliases:\r\n  - Réunion\r\n---\r\n# 项目方案\r\n');
  const note = book.current;
  const peer = otherNote(note, '引用者', '# 引用者\r\n\r\n看 [[项目方案#做法|这个步骤]]。\r\n' +
    '查 [[项目方案]]。\r\n\r\n参考 **réunion** 和 项目方案。\r\n\r\n' +
    '`项目方案` [项目方案](https://example.com) https://example.com/项目方案\r\n\r\n```\r\n项目方案\r\n```');
  const fake = otherNote(note, '代码', '# 项目方案\n\n`[[项目方案]]`，但另一篇项目方案外不匹配');
  book.data.notes.push(peer, fake, { ...peer, id: 'trash', trashed: true });
  const result = await findNoteRelations(book.data, note, signal());
  expect(result.linked.map(item => item.id)).toEqual([peer.id]);
  expect(result.linked[0].lines.map(line => [line.text, line.number])).toEqual([['看 这个步骤。', 3], ['查 项目方案。', 4]]);
  expect(result.mentions.map(item => item.id)).toEqual([peer.id]);
  const line = result.mentions[0].lines[0];
  expect([line.text, line.number, line.spots.map(spot => peer.source.slice(spot.from, spot.to))]).toEqual([
    '参考 réunion 和 项目方案。', 6, ['réunion', '项目方案'],
  ]);
  expect(linkMentionLine(book.data, note, peer, line.spots)).toBe(peer.source.replace('**réunion** 和 项目方案', '**[[项目方案|réunion]]** 和 [[项目方案]]'));
  expect(linkMentionLine(book.data, { ...note, title: '改过标题', source: '# 改过标题' }, peer, line.spots)).toBeUndefined();
});

it('reuses the scan for edits to the open body and rescans if its aliases or another note change', () => {
  const { book } = makeBook('---\naliases: Planning\n---\n# Project plan\n\nFirst draft');
  const note = book.current;
  book.data.notes.push(otherNote(note, 'Meeting', '# Meeting\n\nPlanning.'));
  const copy = structuredClone(book.data);
  copy.notes[0].source = copy.notes[0].source.replace('First draft', 'Revision');
  expect(sameRelationInputs(book.data, copy, note.id)).toBe(true);
  copy.notes[0].source = copy.notes[0].source.replace('Planning', 'Roadmap');
  expect(sameRelationInputs(book.data, copy, note.id)).toBe(false);
  copy.notes[0].source = book.data.notes[0].source;
  copy.notes[1].source += '\nLater';
  expect(sameRelationInputs(book.data, copy, note.id)).toBe(false);
});

it('qualifies a duplicate title by folder without changing what the mention says', async () => {
  const { book } = makeBook('# Project plan');
  const target = book.current;
  const folder = await book.createFolder('Shared');
  target.folderId = folder;
  const peer = otherNote({ ...target, folderId: 'default' }, 'Meeting', '# Meeting\n\nRead Project plan.');
  const twin = otherNote({ ...peer }, 'Project plan', '# Project plan');
  book.data.notes.push(peer, twin);
  const result = await findNoteRelations(book.data, target, signal());
  expect(linkMentionLine(book.data, target, peer, result.mentions[0].lines[0].spots)).toBe('# Meeting\n\nRead [[Shared/Project plan|Project plan]].');
});

it('rejects a queued mention edit or undo when the source has already changed', async () => {
  const { book, storage } = makeBook('# 原稿');
  const { id, source } = book.current;
  const first = book.save('# 另一处编辑', id);
  const stale = book.save('# 旧的引用修改', id, source);
  await first;
  await expect(stale).rejects.toThrow('正文已更新');
  const current = book.current.source;
  const [key, saved] = [...storage.entries()][0];
  storage.set(key, saved.replace('另一处编辑', '另一个窗口'));
  await expect(book.save('# 旧的撤销', id, current)).rejects.toThrow('其他窗口更新');
  expect(storage.get(key)).toContain('另一个窗口');
});
