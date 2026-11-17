import { isolateHistory } from '@codemirror/commands';
import { syntaxTree } from '@codemirror/language';
import { Prec, type EditorState } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import { dailyDateKey } from './daily-note';
import { firstDateYear, lastDateYear, lineNoteDates, noteDateLabel, type NoteDate } from './note-date';

const literalNodes = new Set(['InlineCode', 'FencedCode', 'CodeBlock', 'Link', 'Autolink', 'URL', 'WikiLink',
  'WikiCard', 'Image', 'Table', 'Frontmatter', 'HTMLBlock', 'HTMLTag', 'Comment', 'InlineMath', 'BlockMath']);
const civilKey = (date: Date) => date.toISOString().slice(0, 10);
const monthLabel = new Intl.DateTimeFormat('zh-CN', { timeZone: 'UTC', year: 'numeric', month: 'long' });
const fullDateLabel = new Intl.DateTimeFormat('zh-CN', { timeZone: 'UTC', dateStyle: 'full' });
const firstDay = Date.UTC(firstDateYear, 0, 1), lastDay = Date.UTC(lastDateYear, 11, 31);

function titleLine(state: EditorState) {
  const first = syntaxTree(state).topNode.firstChild;
  const start = first?.name === 'Frontmatter' ? state.doc.lineAt(first.to).number + 1 : 1;
  if (start > state.doc.lines) return 0;
  let number = start;
  for (const text of state.doc.iterLines(start)) {
    if (text.trim()) return number;
    number++;
  }
  return 0;
}

function lineDates(state: EditorState, number: number, title: number) {
  if (number === title) return [];
  const line = state.doc.line(number), tree = syntaxTree(state);
  if (tree.length < line.to) return [];
  return lineNoteDates(line.text, line.from).filter(date => {
    for (let node = tree.resolveInner(date.from, 1); node.parent; node = node.parent) {
      if (literalNodes.has(node.name)) return false;
    }
    return true;
  });
}

function showDatePicker(anchor: HTMLElement, date: Date, choose: (date: Date) => void, restore: () => void, closed: () => void) {
  const doc = anchor.ownerDocument, panel = doc.createElement('div');
  panel.className = 'note-date-picker'; panel.popover = 'auto'; panel.role = 'dialog'; panel.ariaLabel = '选择日期';
  panel.innerHTML = '<header><button type="button" aria-label="上个月">‹</button><strong aria-live="polite"></strong><button type="button" aria-label="下个月">›</button></header><div class="note-date-week" aria-hidden="true"><span>一</span><span>二</span><span>三</span><span>四</span><span>五</span><span>六</span><span>日</span></div><div class="note-date-days" role="group" aria-label="日期"></div>';
  const [previous, next] = panel.querySelectorAll<HTMLButtonElement>('header button');
  const heading = panel.querySelector('strong')!, days = panel.querySelector('.note-date-days')!;
  let cursor = date;
  const today = dailyDateKey(new Date());
  const events = new AbortController();

  function close(restoreFocus = true) {
    if (!panel.isConnected) return;
    events.abort();
    panel.hidePopover(); panel.remove();
    closed();
    if (restoreFocus) restore();
  }
  function render(focus = false) {
    const year = cursor.getUTCFullYear(), month = cursor.getUTCMonth();
    const first = new Date(Date.UTC(year, month, 1));
    const start = 1 - (first.getUTCDay() + 6) % 7;
    heading.textContent = monthLabel.format(cursor);
    previous.disabled = year === firstDateYear && month === 0;
    next.disabled = year === lastDateYear && month === 11;
    let active: HTMLButtonElement | undefined;
    days.replaceChildren(...Array.from({ length: 42 }, (_, index) => {
      const day = new Date(Date.UTC(year, month, start + index));
      const button = doc.createElement('button');
      button.type = 'button'; button.textContent = String(day.getUTCDate());
      button.ariaLabel = fullDateLabel.format(day);
      button.ariaPressed = String(+day === +date);
      if (civilKey(day) === today) button.setAttribute('aria-current', 'date');
      button.toggleAttribute('data-adjacent', day.getUTCMonth() !== month);
      button.disabled = +day < firstDay || +day > lastDay;
      button.tabIndex = +day === +cursor ? 0 : -1;
      if (button.tabIndex === 0) active = button;
      button.onclick = () => { close(false); choose(day); };
      return button;
    }));
    if (focus) active?.focus({ preventScroll: true });
  }
  function step(months: number) {
    const target = new Date(Date.UTC(cursor.getUTCFullYear(), cursor.getUTCMonth() + months, 1));
    if (+target < firstDay || +target > lastDay) return;
    const length = new Date(Date.UTC(target.getUTCFullYear(), target.getUTCMonth() + 1, 0)).getUTCDate();
    target.setUTCDate(Math.min(cursor.getUTCDate(), length));
    cursor = target; render();
  }
  previous.onclick = () => step(-1); next.onclick = () => step(1);
  panel.onkeydown = event => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return; }
    if (!(event.target instanceof HTMLButtonElement) || !days.contains(event.target)) return;
    const delta = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[event.key];
    if (delta === undefined) return;
    event.preventDefault();
    const day = new Date(+cursor);
    day.setUTCDate(day.getUTCDate() + delta);
    if (+day >= firstDay && +day <= lastDay) { cursor = day; render(true); }
  };
  panel.addEventListener('toggle', event => {
    if ((event as ToggleEvent).newState === 'closed') close(false);
  }, { signal: events.signal });
  const place = () => {
    if (!anchor.isConnected) { close(false); return; }
    const box = anchor.getBoundingClientRect(), width = panel.offsetWidth, height = panel.offsetHeight;
    panel.style.left = `${Math.max(8, Math.min(box.left, innerWidth - width - 8))}px`;
    panel.style.top = `${Math.max(8, box.bottom + height <= innerHeight - 14 ? box.bottom + 6 : box.top - height - 6)}px`;
  };
  window.addEventListener('resize', place, { signal: events.signal });
  anchor.closest('.cm-scroller')?.addEventListener('scroll', place, { signal: events.signal, passive: true });
  doc.body.append(panel); render(); panel.showPopover(); place();
  panel.querySelector<HTMLButtonElement>('.note-date-days button[tabindex="0"]')?.focus({ preventScroll: true });
  return close;
}

class DateChip extends WidgetType {
  constructor(readonly match: NoteDate, readonly label: string, readonly tone: string) { super(); }
  eq(other: DateChip) {
    return this.match.from === other.match.from && +this.match.date === +other.match.date && this.label === other.label && this.tone === other.tone;
  }
  toDOM(view: EditorView) {
    const chip = view.dom.ownerDocument.createElement(view.state.readOnly ? 'span' : 'button');
    chip.className = 'note-date-chip'; chip.dataset.tone = this.tone;
    chip.textContent = this.label; chip.title = civilKey(this.match.date);
    if (chip instanceof HTMLButtonElement) {
      chip.type = 'button'; chip.ariaHasPopup = 'dialog'; chip.ariaLabel = `${this.label}，${chip.title}，更改日期`;
      chip.onmousedown = event => { if (event.button === 0) event.preventDefault(); };
      chip.onclick = () => { view.plugin(dates)?.openPicker(chip, this.match); };
    }
    return chip;
  }
}

const dates = ViewPlugin.fromClass(class {
  title: number;
  decorations: DecorationSet;
  closePicker?: (restoreFocus?: boolean) => void;
  day = dailyDateKey(new Date());
  timer = 0;
  constructor(view: EditorView) {
    this.title = titleLine(view.state);
    this.decorations = this.render(view);
    this.watchDay(view);
  }
  watchDay(view: EditorView) {
    const now = new Date(), midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
    this.timer = window.setTimeout(() => {
      view.dispatch({});
      this.watchDay(view);
    }, +midnight - +now + 50);
  }
  render(view: EditorView) {
    const { state } = view, now = new Date(), numbers = new Set<number>();
    const ranges = [];
    for (const range of view.visibleRanges) {
      const last = state.doc.lineAt(range.to).number;
      for (let number = state.doc.lineAt(range.from).number; number <= last; number++) {
        if (numbers.has(number)) continue;
        numbers.add(number);
        for (const match of lineDates(state, number, this.title)) {
          const { from, to } = match;
          const editing = state.selection.ranges.some(range => range.from < to && range.to > from);
          const { label, tone } = noteDateLabel(match.date, now);
          ranges.push((editing ? Decoration.mark({ class: 'note-date-source' }) : Decoration.replace({ widget: new DateChip(match, label, tone) })).range(from, to));
        }
      }
    }
    return Decoration.set(ranges, true);
  }
  openPicker(chip: HTMLElement, match: NoteDate) {
    const view = EditorView.findFromDOM(chip);
    if (!view || view.state.readOnly || view.compositionStarted) return;
    const doc = view.state.doc;
    if (doc.sliceString(match.from, match.to) !== civilKey(match.date)) return;
    this.closePicker?.(false);
    this.closePicker = showDatePicker(chip, match.date, date => {
      if (!view.dom.isConnected || view.state.doc !== doc || view.state.readOnly || view.compositionStarted) return;
      if (+date !== +match.date) view.dispatch({
        changes: { from: match.from, to: match.to, insert: civilKey(date) }, selection: { anchor: match.to },
        annotations: isolateHistory.of('full'), userEvent: 'input.date',
      });
      view.focus();
    }, () => view.focus(), () => { this.closePicker = undefined; });
  }
  update(update: ViewUpdate) {
    const parsed = update.docChanged || syntaxTree(update.startState) !== syntaxTree(update.state);
    if (parsed) this.title = titleLine(update.state);
    if (update.docChanged) this.closePicker?.(false);
    const day = dailyDateKey(new Date());
    if (parsed || update.selectionSet || update.viewportChanged || this.day !== day) {
      this.day = day;
      this.decorations = this.render(update.view);
    }
  }
  destroy() { this.closePicker?.(false); window.clearTimeout(this.timer); }
}, { decorations: plugin => plugin.decorations });

function deleteChip(view: EditorView, forward: boolean, target: EventTarget | null) {
  const { state } = view, { main, ranges } = state.selection;
  if (state.readOnly || view.compositionStarted || !main.empty || ranges.length !== 1 || target !== view.contentDOM) return false;
  const chip = lineDates(state, state.doc.lineAt(main.head).number, view.plugin(dates)?.title ?? 0)
    .find(match => (forward ? match.from : match.to) === main.head);
  if (!chip) return false;
  view.dispatch({ changes: { from: chip.from, to: chip.to }, userEvent: forward ? 'delete.forward' : 'delete.backward',
    annotations: isolateHistory.of('full'), scrollIntoView: true });
  return true;
}

export const noteDateChips = [dates, Prec.high(EditorView.domEventHandlers({
  keydown(event, view) {
    return !event.isComposing && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey &&
      (event.key === 'Backspace' || event.key === 'Delete') && deleteChip(view, event.key === 'Delete', event.target);
  },
  beforeinput(event, view) {
    return !event.isComposing && (event.inputType === 'deleteContentBackward' || event.inputType === 'deleteContentForward') &&
      deleteChip(view, event.inputType === 'deleteContentForward', event.target);
  },
}))];

export function closeNoteDatePicker(view: EditorView) { view.plugin(dates)?.closePicker?.(false); }
