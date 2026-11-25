import { noteTitle } from './document-text';
import type { Folder, Library } from './notebook';

export const dailyFolderName = '每日笔记';
export const dailyTitleStyles = ['long', 'iso', 'weekday'] as const;
export type DailyTitleStyle = typeof dailyTitleStyles[number];
export type DailyNoteSettings = { todayFolder: string; todayTitle: DailyTitleStyle };
type Choice = [string, string];
export type DailyNoteChoices = { folders: Choice[] };

const number = (value: number) => String(value).padStart(2, '0');
const names = (value: string) => value.normalize('NFD').replace(/\p{M}/gu, '').trim().toLowerCase();
const depth = (folder: Folder) => folder.path?.split('/').length ?? 1;
const dateOptions: Record<'long' | 'weekday', Intl.DateTimeFormatOptions> = {
  long: { calendar: 'gregory', year: 'numeric', month: 'long', day: 'numeric' },
  weekday: { calendar: 'gregory', weekday: 'long', month: 'long', day: 'numeric' },
};

export function dailyDateKey(now: Date) {
  return `${now.getFullYear()}-${number(now.getMonth() + 1)}-${number(now.getDate())}`;
}

export async function dailyNoteId(folder: string, now: Date, generation: number) {
  // Both browsers creating the same day can recognize the server's winning write after a retry.
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify([folder, dailyDateKey(now), generation])));
  const bytes = new Uint8Array(digest, 0, 16);
  bytes[6] = (bytes[6] & 0x0f) | 0x80;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export function dailyTitle(now: Date, style: DailyTitleStyle, locale?: string) {
  return style === 'iso' ? dailyDateKey(now) : new Intl.DateTimeFormat(locale, dateOptions[style]).format(now);
}

function namedFolder(folders: Folder[], ...known: string[]) {
  return folders.filter(folder => known.some(name => names(folder.name) === names(name)))
    .sort((a, b) => depth(a) - depth(b))[0];
}

function looksDated(title: string, now: Date, locale?: string) {
  if (/^\d{4}-\d{2}-\d{2}$/u.test(title)) return true;
  return Object.values(dateOptions).some(options => {
    const format = new Intl.DateTimeFormat(locale, options).formatToParts(now).map(({ type, value }) => {
      if (type === 'year') return '\\p{Nd}{4}';
      if (type === 'day') return '\\p{Nd}{1,2}';
      if (type === 'month' || type === 'weekday') return '[\\p{L}\\p{Nd}][\\p{L}\\p{Nd}. -]*';
      return value.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');
    }).join('');
    return new RegExp(`^${format}$`, 'u').test(title);
  });
}

export function findDailyNote(data: Library, settings: DailyNoteSettings, now: Date, locale?: string) {
  const folder = data.folders.find(item => item.id === settings.todayFolder) ?? namedFolder(data.folders, dailyFolderName, 'Daily');
  const titles = [dailyDateKey(now), dailyTitle(now, 'long', locale)];
  const day = dailyDateKey(now);
  const live = data.notes.filter(note => !note.trashed && !note.readOnly).sort((a, b) => a.createdAt - b.createdAt);
  const inFolder = live.filter(note => note.folderId === folder?.id);
  const named = live.filter(note => titles.includes(note.title ?? '') || titles.includes(noteTitle(note)));
  const note = inFolder.find(note => dailyDateKey(new Date(note.createdAt)) === day) ??
    named.find(note => note.folderId === folder?.id) ??
    named.find(note => live.every(other => other.folderId !== note.folderId
      || looksDated(other.title ?? '', now, locale) || looksDated(noteTitle(other), now, locale)));
  return { folder, note };
}

export function dailyNoteChoices(data: Library): DailyNoteChoices {
  return { folders: data.folders.map(folder => [folder.id, folder.path || folder.name]) };
}
