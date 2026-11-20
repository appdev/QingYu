import { EditorSelection, EditorState, Transaction } from '@codemirror/state';
import { isolateHistory } from '@codemirror/commands';
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import type { EditorView } from '@codemirror/view';
import type { SyntaxNode } from '@lezer/common';
import { paneTableComment } from './vendor/markra/markdown/table-fragment';
import { parseMarkdownCalloutMarker } from './vendor/markra/shared/markdown-callout';
import { checklistToggleEvent } from './vendor/markra/editor/codemirror/tasks';
import { insertToday } from './note-date';

export type Format = 'bold' | 'italic' | 'underline' | 'strike' | 'highlight' | 'title' | 'heading' | 'subheading' | 'body' | 'quote' | 'task' | 'code' | 'link' | 'table' | 'bullet' | 'list' | 'ordered' | 'callout' | 'codeblock' | 'divider' | 'date';
export type PanelFormat = Format | 'indent' | 'outdent';

const inlineNodes = {
  StrongEmphasis: 'bold', Emphasis: 'italic', Strikethrough: 'strike', Highlight: 'highlight',
} as const;
const listPrefix = /^([\t ]*)(?:[-+*]|\d{1,9}[.)])[\t ]+/u;
const indivisibleBlocks = new Set(['Table', 'FencedCode', 'CodeBlock', 'SetextHeading', 'Blockquote', 'HTMLBlock', 'Frontmatter']);

export function moveItemTransaction(state: EditorState, up: boolean) {
  const { doc, selection } = state;
  if (state.readOnly || selection.ranges.length !== 1) return null;
  const { from, to, anchor, head, empty } = selection.main;
  const first = doc.lineAt(from).number;
  const last = doc.lineAt(empty ? to : to - 1).number;
  const tree = ensureSyntaxTree(state, doc.line(Math.min(doc.lines, last + 1)).to, 10);
  if (!tree || first === 1) return null;

  const tableAt = (node: SyntaxNode) => {
    while (node.parent && node.name !== 'Table') node = node.parent;
    return node.name === 'Table' ? node : null;
  };
  const tableMetadata = (position: number) => {
    const number = doc.lineAt(position).number;
    const previous = number > 1 ? doc.line(number - 1) : null;
    return previous && paneTableComment.test(previous.text) ? previous : null;
  };
  const blockAt = (number: number, maxDepth = Infinity) => {
    const line = doc.line(number);
    let block: { first: number; last: number; depth: number; list?: number } = { first: number, last: number, depth: 0 };
    if (number < doc.lines && paneTableComment.test(line.text)) {
      const table = tableAt(tree.resolve(doc.line(number + 1).from, 1));
      if (table) return { ...block, last: doc.lineAt(table.to - 1).number };
    }
    for (let node = tree.resolve(line.from + Math.max(0, line.text.search(/\S/u)), 1); node.parent; node = node.parent) {
      const item = node.name === 'ListItem';
      if (!item && !indivisibleBlocks.has(node.name)) continue;
      let depth = 0;
      for (let parent: SyntaxNode | null = node.parent; parent; parent = parent.parent) if (parent.name === 'ListItem') depth++;
      if (depth > maxDepth) continue;
      const metadata = node.name === 'Table' ? tableMetadata(node.from) : null;
      block = { first: metadata?.number ?? doc.lineAt(node.from).number, last: doc.lineAt(node.to - 1).number, depth, list: item ? node.parent.from : undefined };
      if (item) break;
    }
    return block;
  };

  const start = blockAt(first), end = blockAt(last);
  const line = up ? start.first - 1 : end.last + 1;
  if (start.first < 2 || line < 2 || line > doc.lines || line === doc.lines && !doc.line(line).length) return null;
  const other = blockAt(line, start.depth);
  if (other.first < 2 || (up ? other.last >= start.first : other.first <= end.last)) return null;
  if (start.depth && other.list !== start.list) return null;

  const movedFrom = doc.line(start.first).from, movedTo = doc.line(end.last).to;
  const otherFrom = doc.line(other.first).from, otherTo = doc.line(other.last).to;
  const changeFrom = Math.min(movedFrom, otherFrom), changeTo = Math.max(movedTo, otherTo);
  const firstText = doc.sliceString(movedFrom, movedTo), secondText = doc.sliceString(otherFrom, otherTo);
  const shift = up ? -secondText.length - 1 : secondText.length + 1;
  const map = (pos: number) => Math.min(doc.length, pos + shift);
  const edit = state.update({
    changes: { from: changeFrom, to: changeTo, insert: up ? firstText + '\n' + secondText : secondText + '\n' + firstText },
    selection: EditorSelection.range(map(anchor), map(head)), scrollIntoView: true,
    userEvent: 'input.moveItem', annotations: isolateHistory.of('full'),
  });

  // A table may absorb an adjacent paragraph when its separating blank moves.
  const tables: { from: number; to: number }[] = [];
  const lastTouched = doc.lineAt(changeTo).number;
  const following = doc.line(Math.min(doc.lines, lastTouched + 1));
  tree.iterate({ from: doc.line(Math.max(1, doc.lineAt(changeFrom).number - 1)).from, to: following.to,
    enter(node) { if (node.name === 'Table') tables.push({ from: node.from, to: node.to }); },
  });
  if (tables.length) {
    const nextTree = ensureSyntaxTree(edit.state, following.to, 10);
    if (!nextTree) return null;
    for (const table of tables) {
      const delta = table.from >= movedFrom && table.to <= movedTo ? shift
        : table.from >= otherFrom && table.to <= otherTo ? (up ? 1 : -1) * (firstText.length + 1) : 0;
      const node = tableAt(nextTree.resolve(table.from + delta, 1));
      if (node?.from !== table.from + delta || node.to !== table.to + delta) return null;
      const metadata = tableMetadata(table.from);
      if (metadata && edit.newDoc.sliceString(metadata.from + delta, metadata.to + delta) !== metadata.text) return null;
    }
  }
  return edit;
}

export function editorItemMove(view: EditorView, up: boolean) {
  if (view.compositionStarted || view.root.activeElement?.closest('.cm-markra-table, .markra-table-edge-input')) return null;
  return moveItemTransaction(view.state, up);
}

export function moveEditorItem(view: EditorView, up: boolean) {
  const edit = editorItemMove(view, up);
  if (!edit) return false;
  view.dispatch(edit);
  view.focus();
  return true;
}

function codeBlockAt(state: EditorState) {
  const { head } = state.selection.main;
  const line = state.doc.lineAt(head);
  const tree = ensureSyntaxTree(state, line.to, 10) ?? syntaxTree(state);
  for (let node: ReturnType<typeof tree.resolve> | null = tree.resolve(line.from, 1); node; node = node.parent) {
    if (node.name === 'FencedCode' || node.name === 'CodeBlock') return node;
  }
  return null;
}

export function paragraphFormat(state: EditorState, inCode = Boolean(codeBlockAt(state))): Format {
  const line = state.doc.lineAt(state.selection.main.head);
  const text = line.text.trimStart();
  if (!text || inCode) return 'body';
  const heading = /^#{1,6}(?=\s|$)/.exec(text)?.[0].length;
  if (heading) return heading === 1 ? 'title' : heading === 2 ? 'heading' : 'subheading';
  if (text.startsWith('>')) {
    for (let number = line.number; number > 0; number--) {
      const quote = /^[ \t]{0,3}(?:>[ \t]*)+(.*)$/u.exec(state.doc.line(number).text);
      if (!quote) break;
      if (parseMarkdownCalloutMarker(quote[1])) return 'callout';
    }
    return 'quote';
  }
  if (/^[-*+]\s+\[[ xX]\](?:\s|$)/.test(text)) return 'task';
  if (/^-\s/.test(text)) return 'list';
  if (/^[*+]\s/.test(text)) return 'bullet';
  if (/^\d+[.)]\s/.test(text)) return 'ordered';
  for (const previous of state.doc.iterLines(1, line.number)) {
    if (previous.trim()) return 'body';
  }
  return 'title';
}

export function formatPanelState(state: EditorState) {
  const code = codeBlockAt(state);
  const paragraph = paragraphFormat(state, Boolean(code));
  const active = new Set<Format>([code ? 'codeblock' : paragraph]);
  if (['bullet', 'list', 'ordered', 'quote', 'task', 'callout'].includes(paragraph) && !code) active.add('body');
  const { from, to, empty } = state.selection.main;
  const line = state.doc.lineAt(from);
  const tree = syntaxTree(state);
  if (!code && to <= line.to) {
    for (const side of [-1, 1] as const) {
      for (let node: ReturnType<typeof tree.resolve> | null = tree.resolve(from, side); node; node = node.parent) {
        const format = inlineNodes[node.name as keyof typeof inlineNodes];
        if (!format) continue;
        if (from === node.from && to === node.to || node.firstChild && node.lastChild
          && node.firstChild.to <= from && to <= node.lastChild.from) active.add(format);
      }
    }
    for (const match of line.text.matchAll(/<u>(.*?)<\/u>/giu)) {
      const start = line.from + match.index;
      const end = start + match[0].length;
      if (from === start && to === end || start + 3 <= from && to <= end - 4) active.add('underline');
    }
    if (empty) {
      const before = state.sliceDoc(Math.max(line.from, from - 2), from);
      const after = state.sliceDoc(from, Math.min(line.to, from + 2));
      for (const [mark, style] of [['**', 'bold'], ['*', 'italic'], ['~~', 'strike'], ['==', 'highlight']] as const) {
        if (style === 'italic' && active.has('bold')) continue;
        if (before.endsWith(mark) && after.startsWith(mark)) active.add(style);
      }
    }
  }
  const indent = code ? undefined : listPrefix.exec(line.text)?.[1];
  return { active, canIndent: indent !== undefined, canOutdent: Boolean(indent), inCode: Boolean(code), isTitle: paragraph === 'title' };
}

export function formatPanelTransaction(state: EditorState, action: PanelFormat): Transaction {
  const { from, to } = state.selection.main;
  const first = state.doc.lineAt(from), last = state.doc.lineAt(to > from ? to - 1 : to);
  if (action === 'indent' || action === 'outdent') {
    const tree = ensureSyntaxTree(state, last.to, 10) ?? syntaxTree(state);
    const changes = [];
    for (let number = first.number; number <= last.number; number++) {
      const line = state.doc.line(number);
      const prefix = listPrefix.exec(line.text);
      if (!prefix || tree.resolve(line.from + prefix[1].length, 1).name !== 'ListMark') continue;
      const end = line.from + (action === 'outdent' ? Math.min(2, prefix[1].length) : 0);
      changes.push({ from: line.from, to: end, insert: action === 'indent' ? '  ' : '' });
    }
    return state.update({ changes, userEvent: 'input.format', scrollIntoView: true });
  }
  const block = codeBlockAt(state);
  if (action === 'codeblock') {
    const fences = block?.getChildren('CodeMark') ?? [];
    const opening = fences[0], closing = fences[1];
    if (block && !opening) {
      const changes = [];
      const last = state.doc.lineAt(block.to);
      for (let n = state.doc.lineAt(block.from).number; n <= last.number; n++) {
        const line = state.doc.line(n);
        const indent = /^(?: {1,4}|\t)/u.exec(line.text)?.[0] ?? '';
        if (indent) changes.push({ from: line.from, to: line.from + indent.length });
      }
      return state.update({ changes, userEvent: 'input.format', scrollIntoView: true });
    }
    if (opening) {
      const open = state.doc.lineAt(opening.from);
      const close = closing && state.doc.lineAt(closing.from);
      const openEnd = Math.min(state.doc.length, open.to + 1);
      const changes = [{ from: open.from, to: close?.from === openEnd ? close.to : openEnd }];
      if (close && close.from > openEnd) changes.push({ from: close.from - 1, to: close.to });
      return state.update({ changes, userEvent: 'input.format', scrollIntoView: true });
    }
    const changes = state.changes([
      { from: first.from, insert: '```\n' },
      { from: last.to, insert: '\n```' },
    ]);
    const map = (position: number) => first.from === last.to ? first.from + 4
      : changes.mapPos(position, position === last.to ? -1 : 1);
    const { anchor, head } = state.selection.main;
    return state.update({ changes, selection: EditorSelection.range(map(anchor), map(head)), userEvent: 'input.format', scrollIntoView: true });
  }
  if (block && ['title', 'heading', 'subheading', 'body', 'bullet', 'list', 'ordered', 'quote'].includes(action)) {
    const unwrapped = formatPanelTransaction(state, 'codeblock');
    const formatted = formatTransaction(unwrapped.state, action);
    return state.update({ changes: unwrapped.changes.compose(formatted.changes), selection: formatted.newSelection, userEvent: 'input.format', scrollIntoView: true });
  }
  const paragraph = paragraphFormat(state, Boolean(block));
  if (from === to && action === paragraph && ['title', 'heading', 'subheading', 'body'].includes(action)) return state.update({});
  return formatTransaction(state, from === to && action === paragraph && ['bullet', 'list', 'ordered', 'quote'].includes(action) ? 'body' : action);
}

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
  if (action === 'date') return insertToday(state);
  const range = state.selection.main;
  const selected = state.sliceDoc(range.from, range.to);
  if (action === 'link') {
    const insert = `[${selected || '链接'}](https://)`;
    return state.update({ changes: { from: range.from, to: range.to, insert },
      selection: { anchor: range.from + insert.length - 1 }, scrollIntoView: true, userEvent: 'input.format' });
  }
  const wrappers: Partial<Record<Format, [string, string]>> = {
    bold: ['**', '**'], italic: ['*', '*'], underline: ['<u>', '</u>'], strike: ['~~', '~~'], highlight: ['==', '=='],
    code: ['`', '`'],
  };
  const wrapper = wrappers[action];
  if (wrapper) {
    const [left, right] = wrapper;
    const atCaret = range.empty && action !== 'code';
    if (atCaret && left === right) {
      const marker = syntaxTree(state).resolve(range.head, 1);
      if (marker.name.endsWith('Mark') && marker.from === range.head && marker.parent?.to === marker.to
        && state.sliceDoc(marker.from, marker.to) === right) {
        return state.update({ selection: EditorSelection.cursor(marker.to, 1), scrollIntoView: true });
      }
    }
    const target = atCaret ? state.wordAt(range.head) ?? range : range;
    const openFrom = Math.max(0, target.from - left.length);
    const closeTo = Math.min(state.doc.length, target.to + right.length);
    if ((atCaret || action === 'underline') && state.sliceDoc(openFrom, target.from).toLowerCase() === left
      && state.sliceDoc(target.to, closeTo).toLowerCase() === right) {
      return state.update({
        changes: { from: openFrom, to: closeTo, insert: state.sliceDoc(target.from, target.to) },
        selection: EditorSelection.range(range.anchor - left.length, range.head - left.length),
        annotations: Transaction.userEvent.of('input.format'),
      });
    }
    return state.update({
      changes: { from: target.from, to: target.to, insert: left + state.sliceDoc(target.from, target.to) + right },
      selection: EditorSelection.range(range.anchor + left.length, range.head + left.length),
      scrollIntoView: true,
      annotations: Transaction.userEvent.of('input.format'),
    });
  }
  if (action === 'table') {
    const value = '| 标题 | 标题 |\n| --- | --- |\n| 内容 | 内容 |';
    const prefix = state.doc.lineAt(range.from).from === range.from ? '' : '\n';
    return state.update({ changes: { from: range.from, to: range.to, insert: prefix + value }, scrollIntoView: true });
  }
  if (action === 'codeblock' || action === 'divider' || action === 'callout') {
    const value = action === 'divider' ? '\n---\n' : action === 'callout'
      ? `\n> [!NOTE]\n${selected.split('\n').map(line => `> ${line}`).join('\n')}\n`
      : `\n\`\`\`\n${selected}\n\`\`\`\n`;
    return state.update({ changes: { from: range.from, to: range.to, insert: value }, scrollIntoView: true, annotations: Transaction.userEvent.of('input.format') });
  }
  const prefix = { title: '# ', heading: '## ', subheading: '### ', body: '', quote: '> ', task: '- [ ] ', bullet: '* ', list: '- ', ordered: '1. ' }[action as 'title' | 'heading' | 'subheading' | 'body' | 'quote' | 'task' | 'bullet' | 'list' | 'ordered'];
  const start = state.doc.lineAt(range.from).number;
  const end = state.doc.lineAt(range.to > range.from ? range.to - 1 : range.to).number;
  const changes = [];
  for (let n = start; n <= end; n++) {
    const line = state.doc.line(n);
    const indent = line.text.match(/^[\t ]*/)?.[0].length ?? 0;
    const start = line.from + indent;
    const before = line.text.slice(indent).match(/^(?:#{1,6}[\t ]+|>[\t ]?|(?:[-*+]|\d+[.)])[\t ]+(?:\[[ xX]\][\t ]+)?)?/u)?.[0] ?? '';
    if (before !== prefix) changes.push({ from: start, to: start + before.length, insert: prefix });
  }
  const first = changes[0];
  const selection = action === 'task' && range.empty && first && range.head >= first.from && range.head <= first.to
    ? EditorSelection.cursor(first.from + prefix.length) : undefined;
  return state.update({ changes, selection, scrollIntoView: true, annotations: Transaction.userEvent.of('input.format') });
}

export function taskMarker(state: EditorState, lineNumber: number) {
  if (state.readOnly || lineNumber < 1 || lineNumber > state.doc.lines) return null;
  const line = state.doc.line(lineNumber);
  const match = line.text.match(/^[\t >]*(?:[-*+]|\d+[.)])\s+\[([ xX])\]/);
  if (!match) return null;
  const from = line.from + match[0].lastIndexOf('[');
  const node = ensureSyntaxTree(state, line.to, 10)?.resolveInner(from, 1);
  return node?.name === 'TaskMarker' && node.from === from ? { from, checked: match[1] !== ' ' } : null;
}

export function toggleTask(state: EditorState, lineNumber: number) {
  const marker = taskMarker(state, lineNumber);
  if (!marker) return null;
  return state.update({
    changes: { from: marker.from + 1, to: marker.from + 2, insert: marker.checked ? ' ' : 'x' },
    annotations: isolateHistory.of('before'),
    userEvent: checklistToggleEvent,
  });
}

export const checklistItemPattern = /^([\t ]*)[-*+][\t ]+\[([ xX])\](?=[\t ]|$)/;

export function untickChecklist(state: EditorState, at = state.selection.main.head) {
  const { doc } = state;
  const here = doc.lineAt(at);
  if (!checklistItemPattern.test(here.text)) return null;
  let first = here.number, last = here.number;
  while (first > 1 && checklistItemPattern.test(doc.line(first - 1).text)) first--;
  while (last < doc.lines && checklistItemPattern.test(doc.line(last + 1).text)) last++;
  const tree = ensureSyntaxTree(state, doc.line(last).to, 10);
  if (!tree) return null;
  const changes = [];
  for (let number = first; number <= last; number++) {
    const line = doc.line(number);
    const match = checklistItemPattern.exec(line.text)!;
    const bullet = line.from + match[1].length;
    const marker = tree.resolveInner(bullet, 1);
    if (marker.name !== 'ListMark' || marker.from !== bullet) {
      if (number === here.number) return null;
      if (number < here.number) { changes.length = 0; continue; }
      break;
    }
    if (match[2] !== ' ') {
      const from = line.from + match[0].length - 2;
      changes.push({ from, to: from + 1, insert: ' ' });
    }
  }
  return changes.length ? state.update({ changes, userEvent: 'input.task.untickAll', annotations: isolateHistory.of('full') }) : null;
}
