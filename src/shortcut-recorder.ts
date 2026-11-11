import { invoke } from '@tauri-apps/api/core';

export type ShortcutStatus = { shortcut: string; active: boolean; recording: boolean; error: string | null; defaultShortcut: string };
export function shortcutFromEvent(event: KeyboardEvent): string | null {
  if (event.isComposing || event.repeat || !(event.ctrlKey || event.altKey || event.metaKey)) return null;
  if (!/^(Key[A-Z]|Digit[0-9]|F([1-9]|1\d|2[0-4])|Space|Backquote|Minus|Equal|BracketLeft|BracketRight|Backslash|Semicolon|Quote|Comma|Period|Slash|ArrowUp|ArrowDown|ArrowLeft|ArrowRight|Enter|Home|End|PageUp|PageDown|Insert|Delete|Backspace|Tab|Escape)$/.test(event.code)) return null;
  return [...(event.ctrlKey ? ['Control'] : []), ...(event.altKey ? ['Alt'] : []), ...(event.shiftKey ? ['Shift'] : []), ...(event.metaKey ? ['Super'] : []), event.code].join('+');
}
export function shortcutLabel(value: string, mac = /Mac/.test(navigator.platform)) {
  const keys: Record<string, string> = { ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→', Space: '空格', Escape: 'Esc', Enter: '↩', Tab: '⇥', Backspace: '⌫', Delete: '⌦', Comma: ',', Period: '.', Slash: '/', Backslash: '\\', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Semicolon: ';', Quote: "'", Backquote: '`' };
  const modifiers: Record<string, string> = mac ? { Control: '⌃', Alt: '⌥', Shift: '⇧', Super: '⌘' } : { Control: 'Ctrl', Alt: 'Alt', Shift: 'Shift', Super: 'Super' };
  return value.split('+').map(key => modifiers[key] ?? keys[key] ?? key.replace(/^(Key|Digit)/, '')).join(mac ? '' : '+');
}

export function createShortcutRecorder(onError: (message: string) => void) {
  const control = document.createElement('div'); control.className = 'settings-shortcut';
  const record = document.createElement('button'); record.type = 'button'; record.setAttribute('aria-label', '录入唤起快捷键');
  const clear = document.createElement('button'); clear.type = 'button'; clear.className = 'shortcut-clear'; clear.textContent = '×'; clear.setAttribute('aria-label', '清除唤起快捷键');
  control.append(record, clear);
  record.textContent = '正在读取…'; clear.hidden = true;
  let status: ShortcutStatus | undefined, recording = false, disposed = false, generation = 0, applying = false;
  let jobs: Promise<unknown> = Promise.resolve();
  function show(value: ShortcutStatus) {
    if (disposed) return;
    status = value; recording = value.recording;
    record.textContent = recording ? '请按快捷键…' : value.shortcut ? shortcutLabel(value.shortcut) : '设置快捷键';
    record.title = recording ? 'Esc 取消录入' : value.shortcut ? shortcutLabel(value.shortcut) : '点击后按下快捷键组合';
    control.dataset.recording = String(recording); clear.hidden = !value.shortcut || recording;
    if (value.error) onError(value.error);
  }
  async function request(action: string, shortcut?: string) {
    const token = ++generation;
    const job = jobs.catch(() => {}).then(() => invoke<ShortcutStatus>('activation_shortcut_command', { action, shortcut })); jobs = job;
    try { const value = await job; if (token === generation) { if (action !== 'status') onError(''); show(value); } return value; }
    catch (error) { onError(error instanceof Error ? error.message : String(error)); if (action !== 'status') void request('status'); }
  }
  function cancel() { if (recording) { recording = false; void request('cancel'); } }
  record.onclick = () => { if (recording) return; recording = true; record.textContent = '请按快捷键…'; control.dataset.recording = 'true'; clear.hidden = true; record.focus(); void request('record'); };
  clear.onclick = () => { void request('set', ''); };
  record.onkeydown = event => {
    if (!recording || applying) return;
    if (event.key === 'Tab' && !(event.ctrlKey || event.altKey || event.metaKey)) { cancel(); return; }
    event.preventDefault(); event.stopPropagation();
    if (event.isComposing || event.repeat) return;
    if (event.key === 'Escape' && !(event.ctrlKey || event.altKey || event.metaKey)) { cancel(); return; }
    if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) return;
    const value = shortcutFromEvent(event);
    if (value) { applying = true; void request('set', value).finally(() => { applying = false; }); } else onError('请搭配 Command、Ctrl 或 Alt 设置快捷键');
  };
  record.onblur = cancel;
  const refresh = () => { void request('status'); };
  window.addEventListener('focus', refresh); window.addEventListener('blur', cancel);
  const loaded = request('status');
  return { control, loaded, async reset() { if (status) return request('set', status.defaultShortcut); }, destroy() { cancel(); disposed = true; window.removeEventListener('focus', refresh); window.removeEventListener('blur', cancel); } };
}
