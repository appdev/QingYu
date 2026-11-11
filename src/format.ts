import { EditorSelection, EditorState, Transaction } from '@codemirror/state';

export type Format = 'bold' | 'italic' | 'strike' | 'heading' | 'quote' | 'task' | 'code' | 'link' | 'table';

export function replaceSource(state: EditorState, source: string) {
  const current = state.doc.toString();
  if (current === source) return null;
  let from = 0;
  while (from < current.length && from < source.length && current[from] === source[from]) from++;
  if (from > 0 && /[\uDC00-\uDFFF]/.test(current[from] ?? source[from] ?? '')) from--;
  let oldEnd = current.length;
  let newEnd = source.length;
  while (oldEnd > from && newEnd > from && current[oldEnd - 1] === source[newEnd - 1]) { oldEnd--; newEnd--; }
  if (/[\uDC00-\uDFFF]/.test(current[oldEnd] ?? '')) { oldEnd++; newEnd++; }
  return state.update({ changes: { from, to: oldEnd, insert: source.slice(from, newEnd) }, annotations: Transaction.userEvent.of('input.rich') });
}

export function formatTransaction(state: EditorState, action: Format) {
  const range = state.selection.main;
  const selected = state.sliceDoc(range.from, range.to);
  const wrappers: Partial<Record<Format, [string, string]>> = {
    bold: ['**', '**'], italic: ['*', '*'], strike: ['~~', '~~'],
    code: ['`', '`'], link: ['[', '](https://)'],
  };
  const wrapper = wrappers[action];
  if (wrapper) {
    const [left, right] = wrapper;
    return state.update({
      changes: { from: range.from, to: range.to, insert: left + selected + right },
      selection: EditorSelection.range(range.from + left.length, range.from + left.length + selected.length),
      scrollIntoView: true,
      annotations: Transaction.userEvent.of('input.format'),
    });
  }
  if (action === 'table') {
    const value = '| 标题 | 标题 |\n| --- | --- |\n| 内容 | 内容 |';
    const prefix = state.doc.lineAt(range.from).from === range.from ? '' : '\n';
    return state.update({ changes: { from: range.from, to: range.to, insert: prefix + value }, scrollIntoView: true });
  }
  const prefix = { heading: '## ', quote: '> ', task: '- [ ] ' }[action as 'heading' | 'quote' | 'task'];
  const start = state.doc.lineAt(range.from).number;
  const end = state.doc.lineAt(range.to > range.from ? range.to - 1 : range.to).number;
  const changes = [];
  for (let n = start; n <= end; n++) changes.push({ from: state.doc.line(n).from, insert: prefix });
  return state.update({ changes, scrollIntoView: true, annotations: Transaction.userEvent.of('input.format') });
}

export function toggleTask(state: EditorState, lineNumber: number) {
  if (lineNumber < 1 || lineNumber > state.doc.lines) return null;
  const line = state.doc.line(lineNumber);
  const match = line.text.match(/^[\t >]*(?:[-*+]|\d+[.)])\s+\[([ xX])\]/);
  if (!match) return null;
  const position = line.from + match[0].lastIndexOf('[') + 1;
  return state.update({ changes: { from: position, to: position + 1, insert: match[1] === ' ' ? 'x' : ' ' }, annotations: Transaction.userEvent.of('input.task') });
}
