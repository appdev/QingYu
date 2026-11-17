import { isolateHistory } from '@codemirror/commands';
import type { EditorState } from '@codemirror/state';
import { dailyDateKey } from './daily-note';

export const firstDateYear = 1900;
export const lastDateYear = 2200;
const daysInMilliseconds = 86_400_000;
const datePattern = /(?<![\p{L}\p{N}\p{M}_\-/.:])(\d{4}-\d{2}-\d{2})(?![\p{L}\p{N}\p{M}_\-/:]|\.\d)/gu;
const weekday = new Intl.DateTimeFormat('zh-CN', { weekday: 'long', timeZone: 'UTC' });
const monthDay = new Intl.DateTimeFormat('zh-CN', { month: 'short', day: 'numeric', timeZone: 'UTC' });
const yearMonthDay = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'short', day: 'numeric', timeZone: 'UTC' });
export type NoteDate = { from: number; to: number; date: Date };

export function parseNoteDate(value: string) {
  if (!/^\d{4}-\d{2}-\d{2}$/u.test(value)) return null;
  const year = Number(value.slice(0, 4));
  if (year < firstDateYear || year > lastDateYear) return null;
  const date = new Date(`${value}T00:00:00Z`);
  return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value ? date : null;
}

export function lineNoteDates(line: string, start: number): NoteDate[] {
  return Array.from(line.matchAll(datePattern), match => {
    const from = start + match.index, date = parseNoteDate(match[1]);
    return date && { from, to: from + match[1].length, date };
  }).filter((date): date is NoteDate => date !== null);
}

export function noteDateLabel(date: Date, now: Date) {
  const relative = (date.getTime() - Date.UTC(now.getFullYear(), now.getMonth(), now.getDate())) / daysInMilliseconds;
  const tone = relative < 0 ? 'past' : relative > 0 ? 'future' : 'today';
  if (relative === 0) return { label: '今天', tone };
  if (relative === -1) return { label: '昨天', tone };
  if (relative === 1) return { label: '明天', tone };
  const format = relative >= 2 && relative <= 6 ? weekday : date.getUTCFullYear() === now.getFullYear() ? monthDay : yearMonthDay;
  return { label: format.format(date), tone };
}

export function insertToday(state: EditorState, now = new Date()) {
  const { from, to } = state.selection.main;
  const before = from ? state.sliceDoc(from - 1, from) : '\n';
  const after = state.sliceDoc(to, Math.min(state.doc.length, to + 1));
  const insert = (/[\t\n ]/u.test(before) ? '' : ' ') + dailyDateKey(now) + (after === ' ' ? '' : ' ');
  return state.update({
    changes: { from, to, insert }, selection: { anchor: from + insert.length + Number(after === ' ') },
    userEvent: 'input.date', annotations: isolateHistory.of('full'), scrollIntoView: true,
  });
}
