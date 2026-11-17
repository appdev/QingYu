import { history, isolateHistory } from '@codemirror/commands';
import { ensureSyntaxTree } from '@codemirror/language';
import { EditorSelection, EditorState, Transaction, type ChangeSpec, type Line, type Text } from '@codemirror/state';
import { ViewPlugin, type EditorView } from '@codemirror/view';
import { checklistItemPattern, taskMarker } from './format';
import { checklistToggleEvent, focusTaskCheckbox } from './vendor/markra/editor/codemirror/tasks';

const sortEvent = 'input.type.checklistSort';
const sortDelay = 600;

function sortChecklists(state: EditorState, items: ReadonlySet<number>) {
  const { doc } = state;
  const inspected = new Set<number>();
  const changes: ChangeSpec[] = [];
  const positions = new Map<number, number>();
  const marker = (number: number) => checklistItemPattern.exec(doc.line(number).text);
  for (const number of [...items].sort((a, b) => a - b)) {
    if (inspected.has(number)) continue;
    const indent = marker(number)?.[1];
    if (indent === undefined) continue;
    let first = number, last = number;
    while (first > 1 && marker(first - 1)?.[1] === indent) first--;
    while (last < doc.lines && marker(last + 1)?.[1] === indent) last++;
    if (first === last) continue;
    const start = doc.line(first), end = doc.line(last);
    let following = last + 1;
    while (following < doc.lines && !doc.line(following).text.trim()) following++;
    const tree = ensureSyntaxTree(state, doc.line(Math.min(following, doc.lines)).to, 10);
    if (!tree) continue;
    const open: Line[] = [], checked: Line[] = [];
    let singleLineItems = true;
    for (let i = first; i <= last; i++) {
      inspected.add(i);
      const line = doc.line(i);
      const bullet = tree.resolveInner(line.from + indent.length, 1);
      // Moving a list item with children or continuing text could detach them.
      if (bullet.name !== 'ListMark' || bullet.parent?.name !== 'ListItem' || bullet.parent.to > line.to) singleLineItems = false;
      (marker(i)?.[2] === ' ' ? open : checked).push(line);
    }
    if (!singleLineItems || !open.length || !checked.length || open.at(-1)!.number < checked[0].number) continue;
    const ordered = [...open, ...checked];
    let from = start.from;
    for (const line of ordered) {
      positions.set(line.from, from);
      from += line.length + 1;
    }
    changes.push({ from: start.from, to: end.to, insert: ordered.map(line => line.text).join('\n') });
  }
  if (!changes.length) return null;
  const { selection } = state;
  const ranges = selection.ranges.map(range => {
    const line = doc.lineAt(range.from);
    const from = positions.get(line.from);
    if (from === undefined || range.to > line.to) return range;
    const offset = from - line.from;
    return range.empty ? EditorSelection.cursor(range.head + offset, range.assoc)
      : EditorSelection.range(range.anchor + offset, range.head + offset);
  });
  return { changes, positions, selection: EditorSelection.create(ranges, selection.mainIndex) };
}

function taskRow(view: EditorView, at: number) {
  if (at < view.viewport.from || at > view.viewport.to) return null;
  const { node } = view.domAtPos(at);
  return (node instanceof Element ? node : node.parentElement)?.closest<HTMLElement>('.cm-line[data-list-kind=task]');
}

function focusedTask(view: EditorView) {
  const active = view.root.activeElement;
  return active instanceof HTMLInputElement && active.dataset.taskMarker !== undefined ? active : null;
}

const checkboxAt = (view: EditorView, marker: number) =>
  view.contentDOM.querySelector<HTMLInputElement>(`.cm-markra-task-checkbox[data-task-marker="${marker}"]`);

/**
 * 勾选时圆圈弹一下，和 Pinto 的 CheckPop 同一组参数：0.32 秒内 0.55 → 1.12 → 0.97 → 1，
 * 对勾晚 0.06 秒开始，用 0.2 秒画出来。整张清单勾完时，其余圆圈由近到远依次跳起，并从刚勾的圆圈撒出十个小点。
 */
function playCheckPop(view: EditorView, line: number) {
  const { state } = view;
  const marker = taskMarker(state, line);
  const box = marker?.checked ? checkboxAt(view, marker.from) : null;
  const motion = view.dom.ownerDocument.defaultView;
  if (!box || typeof box.animate !== 'function' || motion?.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return;
  box.animate({ transform: ['scale(.55)', 'scale(1.12)', 'scale(.97)', 'scale(1)'], offset: [0, .45, .75, 1], easing: ['ease-out', 'ease-in-out', 'ease-in-out'] }, { duration: 320 });
  box.animate({ clipPath: ['inset(-2px 100% -2px -2px)', 'inset(-2px)'] }, { pseudoElement: '::before', duration: 200, delay: 60, easing: 'ease-out', fill: 'backwards' });

  // 同一缩进、连在一起的清单项算一张清单。
  const indent = checklistItemPattern.exec(state.doc.line(line).text)?.[1];
  const sameList = (number: number) => number >= 1 && number <= state.doc.lines && checklistItemPattern.exec(state.doc.line(number).text)?.[1] === indent;
  let first = line, last = line;
  while (sameList(first - 1)) first--;
  while (sameList(last + 1)) last++;
  const others: HTMLInputElement[] = [];
  for (let number = first; number <= last; number++) {
    const other = taskMarker(state, number);
    if (!other?.checked) return;
    const element = number === line ? null : checkboxAt(view, other.from);
    if (element) others.push(element);
  }
  if (first === last) return;
  const origin = box.getBoundingClientRect(), size = origin.width;
  const middle = (element: Element) => { const bounds = element.getBoundingClientRect(); return bounds.top + bounds.height / 2; };
  others.sort((a, b) => Math.abs(middle(a) - middle(box)) - Math.abs(middle(b) - middle(box))).forEach((other, index) => {
    other.animate({
      transform: ['none', `translateY(${-size * .5}px) scale(1.3)`, `translateY(${size * .06}px) scale(.92)`, 'none'], offset: [0, .38, .72, 1],
    }, { duration: 340, delay: 120 + index * 55 });
  });
  const burst = document.createElement('div');
  burst.className = 'check-burst';
  burst.style.left = `${origin.left + size / 2}px`;
  burst.style.top = `${origin.top + size / 2}px`;
  for (let index = 0; index < 10; index++) {
    const angle = index / 10 * 2 * Math.PI - Math.PI / 2, far = index % 2 === 0;
    const reach = size * (far ? 1.7 : 1.2) + 3, start = size * .6 + 1.5;
    const at = (distance: number, drop = 0) => `translate(${Math.cos(angle) * distance}px, ${Math.sin(angle) * distance + drop}px)`;
    const dot = burst.appendChild(document.createElement('i'));
    dot.style.width = dot.style.height = `${size * (far ? .24 : .16)}px`;
    dot.animate([
      { transform: at(start), opacity: 0, offset: 0, easing: 'ease-out' },
      { opacity: 1, offset: .06 },
      { transform: at(reach), offset: .42, easing: 'ease-in' },
      { transform: `${at(reach)} scale(1)`, offset: .5 },
      { opacity: 1, offset: .68 },
      { transform: `${at(reach * 1.14, size * .8)} scale(.35)`, opacity: 0, offset: 1 },
    ], { duration: 800, delay: 80, fill: 'both' });
  }
  view.dom.ownerDocument.body.append(burst);
  motion?.setTimeout(() => burst.remove(), 950);
}

export function checklistSorting(enabled: () => boolean) {
  let pending: { doc: Text; items: Set<number>; time: number; timer: number } | undefined;
  let slides: Animation[] = [];
  function stopSlides() {
    slides.forEach(slide => slide.cancel());
    slides = [];
  }
  function cancelPending() {
    if (pending) window.clearTimeout(pending.timer);
    pending = undefined;
  }
  function cancel() { cancelPending(); stopSlides(); }
  function sort(view: EditorView) {
    const tick = pending;
    cancelPending();
    if (!tick || tick.doc !== view.state.doc || !enabled() || view.state.readOnly || view.compositionStarted) return;
    const edit = sortChecklists(view.state, tick.items);
    if (!edit) return;
    const { changes, positions, selection } = edit;
    const checkbox = focusedTask(view);
    const marker = checkbox ? Number(checkbox.dataset.taskMarker) : undefined;
    const oldFrom = marker === undefined ? undefined : view.state.doc.lineAt(marker).from;
    const newFrom = oldFrom === undefined ? undefined : positions.get(oldFrom);
    const tops = new Map<number, number>();
    if (typeof view.dom.animate === 'function' && !view.dom.ownerDocument.defaultView?.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      for (const [from, to] of positions) {
        const row = taskRow(view, from);
        if (row) tops.set(to, row.getBoundingClientRect().top);
      }
    }
    view.dispatch({
      changes, selection,
      userEvent: sortEvent,
      // Record the delayed move in the same history event as the last click.
      annotations: [Transaction.time.of(tick.time), isolateHistory.of('after')],
    });
    if (marker !== undefined && oldFrom !== undefined && newFrom !== undefined && (newFrom !== oldFrom || !checkbox?.isConnected)) {
      focusTaskCheckbox(view, newFrom + marker - oldFrom);
    }
    // Animate the rendered rows themselves so clicks still hit the visible checkbox.
    for (const [at, oldTop] of tops) {
      const row = taskRow(view, at);
      if (!row) continue;
      const distance = oldTop - row.getBoundingClientRect().top;
      if (!distance) continue;
      slides.push(row.animate({
        transform: [`translateY(${distance}px)`, 'translateY(0)'],
        backgroundColor: 'var(--paper)', zIndex: String(Math.ceil(Math.abs(distance))),
      }, { duration: 350, easing: 'ease-in-out' }));
    }
  }
  const extension = [
    history({ joinToEvent: (transaction, adjacent) => adjacent || transaction.isUserEvent(sortEvent) }),
    EditorState.transactionExtender.of(transaction => {
      if (pending?.doc !== transaction.startState.doc || !transaction.selection || transaction.docChanged) return null;
      // Moving the caret must not split the pending click's undo event.
      return { annotations: Transaction.addToHistory.of(false) };
    }),
    ViewPlugin.define(view => ({
      update(update) {
        const checkbox = focusedTask(view);
        if (checkbox && update.docChanged && update.transactions.some(transaction => transaction.isUserEvent('undo') || transaction.isUserEvent('redo'))) {
          const { doc } = update.state;
          const marker = Number(checkbox.dataset.taskMarker);
          const number = update.startState.doc.lineAt(marker).number;
          queueMicrotask(() => {
            if (view.dom.isConnected && view.state.doc === doc && !checkbox.isConnected &&
              view.root.activeElement === view.dom.ownerDocument.body && number <= doc.lines) {
              focusTaskCheckbox(view, doc.line(number).from);
            }
          });
        }
        for (const transaction of update.transactions) {
          if (!transaction.docChanged) continue;
          const previous = pending;
          cancel();
          if (!transaction.isUserEvent(checklistToggleEvent)) continue;
          // 弹跳要等新的圆圈画出来，所以放到这次更新之后。
          transaction.changes.iterChangedRanges((_from, _to, from) => {
            const line = transaction.newDoc.lineAt(from).number;
            queueMicrotask(() => { if (view.state.doc === transaction.newDoc) playCheckPop(view, line); });
          });
          if (!enabled()) continue;
          const items = previous?.doc === transaction.startState.doc ? previous.items : new Set<number>();
          transaction.changes.iterChangedRanges(from => items.add(transaction.startState.doc.lineAt(from).number));
          pending = { doc: transaction.newDoc, items, time: transaction.annotation(Transaction.time)!, timer: window.setTimeout(() => sort(view), sortDelay) };
        }
      },
      destroy: cancel,
    }), {
      eventHandlers: {
        mousedown(event) {
          if (!(event.target instanceof Element && event.target.closest('.cm-markra-task-checkbox'))) stopSlides();
        },
      },
    }),
  ];
  return { extension, cancel };
}
