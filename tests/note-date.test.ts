import { expect, it } from 'vitest';
import { EditorState } from '@codemirror/state';
import { insertToday, lineNoteDates, noteDateLabel, parseNoteDate } from '../src/note-date';

it('recognizes stand-alone civil dates and rejects impossible days and embedded dates', () => {
  const line = '日期 2024-02-29，2026-02-29 /2024-02-29 2024-02-29T12:00 2024-02-29.0 2024-02-29。';
  const matches = lineNoteDates(line, 15);
  expect(matches.map(date => line.slice(date.from - 15, date.to - 15))).toEqual(['2024-02-29', '2024-02-29']);
  expect(parseNoteDate('1899-12-31')).toBeNull();
  expect(parseNoteDate('2201-01-01')).toBeNull();
});

it('uses local civil days when choosing the date label', () => {
  const now = new Date(2026, 10, 16, 23, 58);
  expect(noteDateLabel(parseNoteDate('2026-11-16')!, now)).toEqual({ label: '今天', tone: 'today' });
  expect(noteDateLabel(parseNoteDate('2026-11-17')!, now)).toEqual({ label: '明天', tone: 'future' });
  expect(noteDateLabel(parseNoteDate('2026-11-15')!, now)).toEqual({ label: '昨天', tone: 'past' });
  expect(noteDateLabel(parseNoteDate('2026-11-18')!, now).label).toBe('星期三');
  expect(noteDateLabel(parseNoteDate('2027-02-05')!, now).label).toContain('2027');
});

it('inserts an ISO date and leaves the caret after the existing or inserted space', () => {
  const today = new Date(2026, 10, 16, 1);
  const state = EditorState.create({ doc: '开始 继续', selection: { anchor: 2 } });
  const inserted = insertToday(state, today);
  expect(inserted.newDoc.toString()).toBe('开始 2026-11-16 继续');
  expect(inserted.newSelection.main.head).toBe(inserted.newDoc.toString().indexOf('继续'));
  const replaced = insertToday(EditorState.create({ doc: '日期\n继续', selection: { anchor: 0, head: 2 } }), today);
  expect(replaced.newDoc.toString()).toBe('2026-11-16 \n继续');
  expect(replaced.newSelection.main.head).toBe(11);
});
