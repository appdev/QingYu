import { invoke } from '@tauri-apps/api/core';
import './calendar.css';

export function calendarEvent(title: string, notes: string, start: string, end: string, reminder: string) {
  const startMs = new Date(start).getTime(), endMs = new Date(end).getTime();
  const reminderMinutes = reminder === '' ? null : Number(reminder);
  if (!title.trim() || [...title].length > 200 || /[\x00-\x1f\x7f-\x9f]/.test(title)) throw new Error('请输入 1–200 字的日程标题');
  if ([...notes].length > 10_000 || notes.includes('\0')) throw new Error('备注不能超过 10000 字或包含空字符');
  if (!Number.isFinite(startMs) || startMs < 0 || !Number.isFinite(endMs) || endMs <= startMs || endMs > 253_402_300_799_000 || localTime(new Date(startMs)) !== start || localTime(new Date(endMs)) !== end) throw new Error('请选择有效的起止时间，结束时间须晚于开始时间');
  if (reminderMinutes !== null && (!Number.isInteger(reminderMinutes) || reminderMinutes < 0 || reminderMinutes > 40_320)) throw new Error('提醒时间须在日程开始前 0–40320 分钟内');
  return { title: title.trim(), notes, startMs, endMs, reminderMinutes };
}

function localTime(date: Date) {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}T${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
}

export function setupCalendar(getTitle: () => string) {
  const button = document.querySelector<HTMLButtonElement>('#calendar-button');
  if (!button || !('__TAURI_INTERNALS__' in window)) return;
  button.hidden = false;
  let dialog: HTMLDialogElement | undefined;
  button.onclick = () => {
    if (dialog) { if (!dialog.open) dialog.showModal(); return; }
    dialog = document.createElement('dialog'); dialog.id = 'calendar-dialog'; dialog.setAttribute('aria-labelledby', 'calendar-title');
    dialog.innerHTML = `<form>
      <h2 id="calendar-title">创建日程</h2>
      <label for="event-title">标题</label><input id="event-title" required maxlength="400" autocomplete="off">
      <label for="event-start">开始时间</label><input id="event-start" type="datetime-local" required min="1970-01-02T00:00" max="9999-12-30T23:59">
      <label for="event-end">结束时间</label><input id="event-end" type="datetime-local" required min="1970-01-02T00:00" max="9999-12-30T23:59">
      <label for="event-reminder">提醒</label><select id="event-reminder"><option value="">不提醒</option><option value="0">日程开始时</option><option value="5">提前 5 分钟</option><option value="15" selected>提前 15 分钟</option><option value="30">提前 30 分钟</option><option value="60">提前 1 小时</option><option value="1440">提前 1 天</option><option value="custom">自定义</option></select>
      <div id="event-custom" hidden><label for="event-minutes">提前多少分钟</label><input id="event-minutes" type="number" min="0" max="40320" step="1" value="15"></div>
      <label for="event-notes">备注（可选）</label><textarea id="event-notes" rows="3" maxlength="20000"></textarea>
      <p class="calendar-help">按当前设备时区创建。Apple / Android 保存到系统日历；Windows / Linux 需在默认日历软件中确认导入。提醒通知由日历软件和系统设置控制。</p>
      <p id="event-feedback" role="status" aria-live="polite"></p>
      <div class="dialog-actions"><button id="event-cancel" type="button">取消</button><button id="event-save" class="primary-button" type="submit">保存到日历</button></div>
    </form>`;
    document.body.append(dialog);
    const current = dialog;
    const field = <T extends HTMLElement>(id: string) => current.querySelector<T>(`#${id}`)!;
    const title = field<HTMLInputElement>('event-title'), start = field<HTMLInputElement>('event-start'), end = field<HTMLInputElement>('event-end');
    const reminder = field<HTMLSelectElement>('event-reminder'), minutes = field<HTMLInputElement>('event-minutes');
    const notes = field<HTMLTextAreaElement>('event-notes'), feedback = field<HTMLParagraphElement>('event-feedback');
    const cancel = field<HTMLButtonElement>('event-cancel'), save = field<HTMLButtonElement>('event-save');
    const form = current.querySelector('form')!;
    const next = new Date(Math.ceil((Date.now() + 16 * 60_000) / 900_000) * 900_000);
    title.value = [...getTitle()].slice(0, 200).join(''); start.value = localTime(next); end.value = localTime(new Date(next.getTime() + 3_600_000));
    reminder.onchange = () => { field('event-custom').hidden = reminder.value !== 'custom'; minutes.required = reminder.value === 'custom'; };
    let busy = false, saved = false;
    current.addEventListener('cancel', event => { if (busy) event.preventDefault(); });
    current.addEventListener('close', () => {
      current.remove(); dialog = undefined;
      const trigger = button.closest('#format-menu') ? document.getElementById('format-button') : button;
      trigger?.focus();
    });
    cancel.onclick = () => { if (!busy) current.close(); };
    form.onsubmit = async event => {
      event.preventDefault();
      if (busy || saved) return;
      try {
        const value = calendarEvent(title.value, notes.value, start.value, end.value, reminder.value === 'custom' ? minutes.value : reminder.value);
        if (reminder.value === 'custom' && minutes.value === '') throw new Error('请输入提前提醒的分钟数');
        busy = true; form.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement | HTMLTextAreaElement>('input, button, select, textarea').forEach(control => { control.disabled = true; });
        feedback.textContent = '正在保存，请完成系统日历授权…'; feedback.dataset.error = 'false';
        feedback.textContent = await invoke<string>('create_calendar_event', { event: value });
        saved = true; save.hidden = true; cancel.textContent = '完成';
      } catch (reason) {
        feedback.textContent = reason instanceof Error ? reason.message : String(reason); feedback.dataset.error = 'true';
      } finally {
        busy = false;
        if (!saved) form.querySelectorAll<HTMLInputElement | HTMLButtonElement | HTMLSelectElement | HTMLTextAreaElement>('input, button, select, textarea').forEach(control => { control.disabled = false; });
        cancel.disabled = false;
      }
    };
    current.showModal(); title.focus(); title.select();
  };
}
