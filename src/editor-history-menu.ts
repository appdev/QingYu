import { redo, redoDepth, undo, undoDepth } from '@codemirror/commands';
import { EditorView, ViewPlugin } from '@codemirror/view';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { keepEditorFocusForTouchButtons } from './editor-viewport';
import { editorItemMove, moveEditorItem, taskMarker, toggleTask, untickChecklist, type Format } from './format';
import { showMenu } from './popup-menu';
import { markupLabel, uiText, workflowText } from './ui-language';

type HistoryAction = 'undo' | 'redo';
type FormatAction = Format | 'move-up' | 'move-down' | 'tick' | 'untick';

function focusedEditor(target: Element) {
  const content = target.closest<HTMLElement>('.cm-content');
  const view = content && EditorView.findFromDOM(content);
  return view && !view.state.readOnly ? view : null;
}

export function runEditorHistoryMenu(action: HistoryAction) {
  const target = document.activeElement;
  if (!target || target.closest('[inert]')) return;
  const view = focusedEditor(target);
  if (!view) {
    if (target.matches('input, textarea, [contenteditable="true"]')) document.execCommand(action);
    return;
  }
  runEditorHistory(view, action);
}

function runEditorHistory(view: EditorView, action: HistoryAction) {
  if (view.state.readOnly || view.compositionStarted) return;
  const target = view.root.activeElement;
  // Use the table's existing history handler so it can restore the active cell.
  const event = new InputEvent('beforeinput', {
    bubbles: true, cancelable: true, inputType: action === 'undo' ? 'historyUndo' : 'historyRedo',
  });
  if (!target || !view.contentDOM.contains(target) || target.dispatchEvent(event)) (action === 'undo' ? undo : redo)(view);
}

let schedule = () => {};
export function refreshEditorHistory() { schedule(); }
export const editorHistoryMenuUpdates = ViewPlugin.define(view => {
  let notice: HTMLElement | undefined;
  let dismissTimer = 0;
  function dismissNotice() {
    window.clearTimeout(dismissTimer);
    notice?.remove();
    notice = undefined;
  }
  function showNotice(message: string) {
    const doc = view.dom.ownerDocument;
    const after = view.state.doc;
    const toast = doc.createElement('div');
    toast.className = 'editor-history-notice';
    toast.innerHTML = `<svg viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="9" fill="currentColor"/><path d="m6 10 2.5 2.5L14 7" fill="none" stroke="var(--primary-ink)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg><span role="status"></span><button type="button">${markupLabel('撤销')}</button>`;
    const button = toast.querySelector('button')!;
    button.onpointerdown = event => event.preventDefault();
    if (doc.documentElement.hasAttribute('data-mobile')) keepEditorFocusForTouchButtons(toast);
    button.onclick = () => {
      if (notice !== toast || view.state.doc !== after || view.state.readOnly || view.compositionStarted) return;
      if (undo(view)) view.focus();
    };
    view.dom.append(toast);
    toast.querySelector('span')!.textContent = message;
    notice = toast;
    dismissTimer = window.setTimeout(dismissNotice, 5000);
  }
  schedule();
  return {
    update(update) {
      if (update.docChanged || update.focusChanged || update.selectionSet) schedule();
      if (!update.docChanged) return;
      const edit = update.transactions.filter(transaction => transaction.docChanged).at(-1);
      if (!edit) return;
      dismissNotice();
      if (edit.isUserEvent('input.table.delete')) showNotice('已删除表格');
      else if (edit.isUserEvent('input.task.untickAll')) {
        let count = 0;
        edit.changes.iterChangedRanges(() => count++);
        showNotice(`已取消勾选 ${count} 项`);
      }
    },
    dismissNotice,
    destroy: dismissNotice,
  };
});

export function connectMobileEditorHistory(button: HTMLButtonElement, currentEditor: () => EditorView | undefined) {
  let frame = 0, timer = 0, held = false;
  let start: { x: number; y: number } | undefined;
  schedule = () => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const view = currentEditor();
      button.hidden = !view || view.state.readOnly || !undoDepth(view.state);
      if (!button.hidden && view && redoDepth(view.state)) button.setAttribute('aria-haspopup', 'menu');
      else button.removeAttribute('aria-haspopup');
    });
  };
  const cancelHold = () => { window.clearTimeout(timer); start = undefined; };
  const showRedo = () => {
    const view = currentEditor();
    if (!view || view.compositionStarted || !redoDepth(view.state)) return;
    held = true;
    const { doc } = view.state;
    const focused = view.root.activeElement;
    const restore = () => {
      if (currentEditor() === view && view.state.doc === doc && focused instanceof HTMLElement && view.contentDOM.contains(focused)) focused.focus({ preventScroll: true });
    };
    const run = () => { if (currentEditor() === view && view.state.doc === doc) runEditorHistory(view, 'redo'); };
    void showMenu([{ text: uiText('menu.redo'), run }], { anchor: button, label: '撤销与重做', restoreFocus: restore });
  };
  button.onpointerdown = event => {
    if (!event.isPrimary || event.button !== 0) return;
    event.preventDefault();
    cancelHold(); held = false;
    if (event.pointerType === 'touch' && button.hasAttribute('aria-haspopup')) {
      start = { x: event.clientX, y: event.clientY };
      timer = window.setTimeout(showRedo, 450);
    }
  };
  button.onpointermove = event => { if (start && Math.hypot(event.clientX - start.x, event.clientY - start.y) > 10) cancelHold(); };
  button.onpointerup = button.onpointercancel = cancelHold;
  button.oncontextmenu = event => { event.preventDefault(); cancelHold(); if (!held) showRedo(); };
  button.onclick = event => {
    if (held && event.detail) { held = false; return; }
    const view = currentEditor();
    if (view) runEditorHistory(view, 'undo');
  };
  schedule();
}

export async function connectEditorHistoryMenu(formatEditor?: {
  current: () => EditorView | null;
  apply: (action: Format) => void;
}) {
  if (!('__TAURI_INTERNALS__' in window) || !/Mac/.test(navigator.platform) || navigator.maxTouchPoints > 1) return;
  let scheduled = false;
  let sent = '';
  function formatTarget() {
    const target = document.activeElement;
    const view = target && !target.closest('[inert], .markra-table-edge-input') && focusedEditor(target);
    if (!view || view !== formatEditor?.current() || view.compositionStarted) return null;
    const inTable = Boolean(target.closest('.cm-markra-table'));
    const checkbox = target.matches('.cm-markra-task-checkbox');
    const marker = checkbox ? Number((target as HTMLElement).dataset.taskMarker) : NaN;
    const position = Number.isInteger(marker) && marker >= 0 && marker < view.state.doc.length ? marker : view.state.selection.main.head;
    return { view, inTable, checkbox, line: view.state.doc.lineAt(position) };
  }
  schedule = () => {
    if (scheduled) return;
    scheduled = true;
    // The native menu can open before WebKit paints another frame.
    queueMicrotask(() => {
      scheduled = false;
      if (!document.hasFocus()) return;
      const target = document.activeElement;
      const view = target && focusedEditor(target);
      const available = (action: HistoryAction) => Boolean(target && !target.closest('[inert]') && (
        view ? (action === 'undo' ? undoDepth : redoDepth)(view.state) > 0 : document.queryCommandEnabled(action)
      ));
      const sections = Boolean(document.querySelector('#workspace:not([hidden]) #rich-editor .cm-editor'));
      const writing = formatTarget();
      const text = writing && !writing.inTable ? writing : null;
      const format = {
        editable: Boolean(writing),
        ticked: text ? taskMarker(text.view.state, text.line.number)?.checked ?? null : null,
        untick: Boolean(text && untickChecklist(text.view.state, text.line.from)),
        moves: [true, false].map(up => Boolean(text && !text.checkbox && editorItemMove(text.view, up))),
      };
      const tickLabel = workflowText(format.ticked ? '标为未完成' : '标为已完成');
      const payload = { undo: available('undo'), redo: available('redo'), sections, format, tickLabel };
      const snapshot = JSON.stringify(payload);
      if (snapshot === sent) return;
      sent = snapshot;
      void invoke('editor_menu_state', payload).catch(error => { if (sent === snapshot) sent = ''; console.warn(error); });
    });
  };
  await listen<HistoryAction>('editor-history-menu', ({ payload }) => {
    if (payload === 'undo' || payload === 'redo') runEditorHistoryMenu(payload);
    schedule();
  }, { target: getCurrentWindow().label });
  await listen<FormatAction>('note-format', ({ payload }) => {
    const writing = formatTarget();
    if (!writing) return;
    const { view, line, inTable, checkbox } = writing;
    if (payload === 'move-up' || payload === 'move-down') {
      if (!inTable && !checkbox) moveEditorItem(view, payload === 'move-up');
    } else if (payload === 'tick' || payload === 'untick') {
      if (inTable) return;
      const change = payload === 'tick' ? toggleTask(view.state, line.number) : untickChecklist(view.state, line.from);
      if (change) {
        view.dispatch(change);
        if (checkbox) view.focus();
      }
    } else formatEditor?.apply(payload);
  }, { target: getCurrentWindow().label });
  for (const event of ['focusin', 'focusout', 'input', 'compositionstart', 'compositionend']) document.addEventListener(event, schedule);
  window.addEventListener('focus', () => { sent = ''; schedule(); });
  window.addEventListener('ui-language-change', schedule);
  schedule();
}
