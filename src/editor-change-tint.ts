import { StateEffect, StateField, type EditorState, type Extension } from '@codemirror/state';
import { Decoration, EditorView, ViewPlugin, type DecorationSet } from '@codemirror/view';
import { replaceSource } from './format';
import { diffVersionLines } from './note-version-diff';

// 对应 Pinto 的 ChangeTint：笔记被编辑器之外的来源改动后，改过的行在文字后面铺一层强调色，
// 左边画一条竖线。开始打字、撤销或离开这篇笔记时消失。

type Marked = { lines: readonly number[]; before: string; who: string };
const markEffect = StateEffect.define<Marked | null>();
const linePrefix = /^[\t ]*(?:(?:[-*+]|\d+[.)])[\t ]+(?:\[[ xX]\][\t ]+)?|#{1,6}[\t ]+|>[\t ]?)?/u;

/** `after` 里有、`before` 里没有的行，行号从 1 开始。空行不算。 */
export function changedLines(before: string, after: string) {
  const lines: number[] = [];
  let number = 0;
  for (const line of diffVersionLines(before, after)) {
    if (line.change === 'removed') continue;
    number++;
    if (line.change === 'added' && line.text.trim()) lines.push(number);
  }
  return lines;
}

function decorate(state: EditorState, lines: readonly number[]) {
  const ranges = [];
  for (const number of lines) {
    if (number > state.doc.lines) continue;
    const line = state.doc.line(number);
    const from = line.from + (linePrefix.exec(line.text)?.[0].length ?? 0);
    const to = line.from + line.text.trimEnd().length;
    ranges.push(Decoration.line({ class: 'cm-change-line' }).range(line.from));
    if (to > from) ranges.push(Decoration.mark({ class: 'cm-change-tint' }).range(from, to));
  }
  return Decoration.set(ranges, true);
}

const tintField = StateField.define<{ marks: DecorationSet; info: Marked | null }>({
  create: () => ({ marks: Decoration.none, info: null }),
  update(value, transaction) {
    const effect = transaction.effects.find(item => item.is(markEffect));
    if (effect) return effect.value ? { marks: decorate(transaction.state, effect.value.lines), info: effect.value } : { marks: Decoration.none, info: null };
    return transaction.docChanged && value.info ? { marks: Decoration.none, info: null } : value;
  },
  provide: field => EditorView.decorations.from(field, value => value.marks),
});

const receipt = ViewPlugin.define(view => {
  let pill: HTMLElement | undefined, shown: Marked | null = null;
  function sync() {
    const { info } = view.state.field(tintField);
    if (info === shown) return;
    shown = info;
    pill?.remove();
    pill = undefined;
    if (!info) return;
    const doc = view.dom.ownerDocument;
    pill = doc.createElement('div');
    pill.className = 'editor-change-receipt';
    const text = pill.appendChild(doc.createElement('span'));
    text.setAttribute('role', 'status');
    text.textContent = `${info.who}修改了 ${info.lines.length} 行`;
    const undo = pill.appendChild(doc.createElement('button'));
    undo.type = 'button';
    undo.textContent = '撤销';
    undo.onpointerdown = event => event.preventDefault();
    undo.onclick = () => {
      if (view.state.readOnly || view.compositionStarted) return;
      // 当作一次普通编辑写回去，保存和撤销栈都照常工作。
      const restore = replaceSource(view.state, info.before);
      if (restore) view.dispatch(restore);
      else view.dispatch({ effects: markEffect.of(null) });
      view.focus();
    };
    view.dom.append(pill);
  }
  sync();
  return { update: sync, destroy() { pill?.remove(); } };
});

export const changeTint: Extension = [tintField, receipt];

/** 正文刚从 `before` 变成现在的内容：标出改动的行。没有可标的行时什么也不做。 */
export function markChanges(view: EditorView, before: string, who = '外部') {
  const lines = changedLines(before, view.state.doc.toString());
  view.dispatch({ effects: markEffect.of(lines.length ? { lines, before, who } : null) });
  return lines.length;
}
