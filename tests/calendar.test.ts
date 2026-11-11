// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { calendarEvent, setupCalendar } from '../src/calendar';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); vi.resetAllMocks(); });

it('validates dates, title, optional and custom alarms before invoking native code', () => {
  const create = (reminder: string) => calendarEvent('  会议  ', '备注', '2026-11-12T10:00', '2026-11-12T11:00', reminder);
  expect(create('15')).toEqual({ title: '会议', notes: '备注', startMs: new Date('2026-11-12T10:00').getTime(), endMs: new Date('2026-11-12T11:00').getTime(), reminderMinutes: 15 });
  expect(create('').reminderMinutes).toBeNull(); expect(create('0').reminderMinutes).toBe(0);
  for (const reminder of ['-1', '0.5', '40321', 'custom']) expect(() => create(reminder)).toThrow();
  expect(() => calendarEvent(' ', '', '', '', '')).toThrow();
  expect(() => calendarEvent('日程', '', 'bad', '2026-11-12T11:00', '')).toThrow();
  expect(() => calendarEvent('日程', '', '2026-11-12T11:00', '2026-11-12T10:00', '')).toThrow();
  expect(() => calendarEvent('日程\n注入', '', '2026-11-12T10:00', '2026-11-12T11:00', '')).toThrow();
});

it('keeps web entry hidden and never invokes native code', () => {
  document.body.innerHTML = '<button id="calendar-button" hidden></button>';
  setupCalendar(() => '标题'); document.querySelector<HTMLButtonElement>('button')!.click();
  expect(document.querySelector('button')!.hidden).toBe(true); expect(document.querySelector('dialog')).toBeNull(); expect(invoke).not.toHaveBeenCalled();
});

it('preserves failed drafts, prevents duplicate submissions and reports confirmed native result', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {});
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; this.dispatchEvent(new Event('close')); };
  document.body.innerHTML = '<button id="calendar-button" hidden></button>';
  setupCalendar(() => '<标题>'); document.querySelector<HTMLButtonElement>('button')!.click();
  const title = document.querySelector<HTMLInputElement>('#event-title')!;
  expect(title.value).toBe('<标题>'); expect(document.querySelector('#calendar-dialog 标题')).toBeNull();
  const form = document.querySelector('form')!;
  vi.mocked(invoke).mockRejectedValueOnce('日历权限被拒绝');
  form.dispatchEvent(new Event('submit', { cancelable: true })); await vi.waitFor(() => expect(document.querySelector('#event-feedback')!.textContent).toContain('权限被拒绝'));
  expect(title.value).toBe('<标题>'); expect(title.disabled).toBe(false);
  const reminder = document.querySelector<HTMLSelectElement>('#event-reminder')!; reminder.value = 'custom'; reminder.dispatchEvent(new Event('change'));
  document.querySelector<HTMLInputElement>('#event-minutes')!.value = '42';
  let finish!: (value: string) => void;
  vi.mocked(invoke).mockImplementationOnce(() => new Promise(resolve => { finish = resolve as (value: string) => void; }));
  form.dispatchEvent(new Event('submit', { cancelable: true })); form.dispatchEvent(new Event('submit', { cancelable: true }));
  expect(invoke).toHaveBeenCalledTimes(2); expect(vi.mocked(invoke).mock.calls[1][1]).toMatchObject({ event: { title: '<标题>', reminderMinutes: 42 } });
  const cancel = new Event('cancel', { cancelable: true }); document.querySelector('dialog')!.dispatchEvent(cancel); expect(cancel.defaultPrevented).toBe(true);
  finish('日程已写入系统日历'); await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('#event-save')!.hidden).toBe(true));
  form.dispatchEvent(new Event('submit', { cancelable: true })); expect(invoke).toHaveBeenCalledTimes(2);
  document.querySelector<HTMLButtonElement>('#event-cancel')!.click(); expect(document.querySelector('dialog')).toBeNull();
});
