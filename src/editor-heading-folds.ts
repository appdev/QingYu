import { translatedLabel } from './ui-language';
import { codeFolding, ensureSyntaxTree, foldEffect, foldedRanges, syntaxTree, unfoldAll, unfoldEffect } from '@codemirror/language';
import { EditorState, Prec, StateEffect } from '@codemirror/state';
import { Decoration, EditorView, keymap, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from '@codemirror/view';
import type { Tree } from '@lezer/common';

type Section = { from: number; to: number; line: number; level: number; key: string; complete: boolean };
const heading = /^ {0,3}(#{1,3})[ \t]+(\S.*)$/u;
export const reloadHeadingFolds = StateEffect.define<null>();

function sections(state: EditorState, tree = syntaxTree(state), complete = tree.length === state.doc.length) {
  const result: Section[] = [], stack: Section[] = [];
  const occurrences = new Map<string, number>();
  let end = 0;
  const lastContentEnd = () => {
    let line = state.doc.lineAt(Math.max(0, end - 1));
    while (line.number > 1 && !line.text.trim()) line = state.doc.line(line.number - 1);
    return line.to;
  };
  for (let node = tree.topNode.firstChild; node; node = node.nextSibling) {
    if (/^ATXHeading[1-3]$/u.test(node.name)) {
      const line = state.doc.lineAt(node.from);
      const match = heading.exec(line.text);
      if (match) {
        const level = match[1].length;
        if (stack.length && stack.at(-1)!.level >= level) {
          const to = lastContentEnd();
          while (stack.length && stack.at(-1)!.level >= level) {
            Object.assign(stack.pop()!, { to, complete: true });
          }
        }
        const title = `${level} ${match[2].trimEnd()}`;
        const index = occurrences.get(title) ?? 0;
        occurrences.set(title, index + 1);
        const section = { from: line.to, to: line.to, line: line.from, level, key: `${title} ${index}`, complete: false };
        stack.push(section);
        result.push(section);
      }
    }
    end = node.to;
  }
  if (stack.length) {
    const to = lastContentEnd();
    for (const section of stack) Object.assign(section, { to, complete });
  }
  return result.filter(section => section.to > section.from);
}

function foldAt(state: EditorState, boundary: number) {
  let range: { from: number; to: number } | undefined;
  foldedRanges(state).between(boundary, boundary, (from, to) => { if (from === boundary) range = { from, to }; });
  return range;
}

async function parsedSections(view: EditorView, current: () => boolean) {
  const doc = view.state.doc;
  while (current() && view.state.doc === doc && view.dom.isConnected) {
    const tree = ensureSyntaxTree(view.state, doc.length, 20);
    if (tree) return sections(view.state, tree, true);
    await new Promise(resolve => setTimeout(resolve, 0));
  }
  return [];
}

function collapse(view: EditorView, ranges: Section[]) {
  if (!ranges.length) return;
  const selection = view.state.selection.main;
  const focus = view.root.activeElement;
  const inside = focus && view.contentDOM.contains(focus);
  const positions = [selection.from, selection.to];
  if (inside && focus !== view.contentDOM) positions.push(view.posAtDOM(focus));
  const hidden = ranges.find(({ from, to }) => positions.some(pos => pos > from && pos < to));
  view.dispatch({ effects: ranges.map(range => foldEffect.of(range)), selection: hidden ? { anchor: hidden.from } : undefined });
  if (inside && !focus.isConnected) view.focus();
}

export async function foldAllHeadings(view: EditorView, expanded: boolean, current: () => boolean) {
  if (view.compositionStarted) return;
  if (expanded) { unfoldAll(view); return; }
  const doc = view.state.doc;
  const ranges = await parsedSections(view, current);
  if (current() && doc === view.state.doc && !view.compositionStarted) collapse(view, ranges.filter(range => !foldAt(view.state, range.from)));
}

export function revealHeadingSection(view: EditorView, position: number) {
  const effects: StateEffect<{ from: number; to: number }>[] = [];
  foldedRanges(view.state).between(position, position, (from, to) => {
    if (from < position && to >= position) effects.push(unfoldEffect.of({ from, to }));
  });
  if (effects.length) view.dispatch({ effects });
}

class HeadingFoldButton extends WidgetType {
  constructor(readonly section: Section, readonly collapsed: boolean, readonly more = false) { super(); }
  eq(other: HeadingFoldButton) {
    return this.section.from === other.section.from && this.section.to === other.section.to &&
      this.section.complete === other.section.complete && this.collapsed === other.collapsed && this.more === other.more;
  }
  toDOM(view: EditorView) {
    const button = view.dom.ownerDocument.createElement('button');
    button.type = 'button';
    button.className = this.more ? 'qingyu-heading-more' : 'qingyu-heading-fold';
    if (this.more) button.textContent = '…';
    else button.ariaExpanded = String(!this.collapsed);
    button.ariaLabel = button.title = this.collapsed ? translatedLabel('editor.expandSection') : translatedLabel('收起章节');
    button.onmousedown = event => event.preventDefault();
    button.onclick = async event => {
      event.preventDefault();
      event.stopPropagation();
      if (view.compositionStarted) return;
      const existing = foldAt(view.state, this.section.from);
      if (existing) { view.dispatch({ effects: unfoldEffect.of(existing) }); return; }
      const known = this.section;
      const doc = view.state.doc;
      const ranges = known.complete ? [known] : await parsedSections(view, () => view.state.doc === doc);
      const current = ranges.find(section => section.from === known.from);
      if (view.dom.isConnected && doc === view.state.doc && current && !view.compositionStarted) collapse(view, [current]);
    };
    return button;
  }
}

async function hashedKeys(sectionsToHash: Section[]) {
  if (!globalThis.crypto?.subtle) return [];
  const encoder = new TextEncoder();
  return Promise.all(sectionsToHash.map(async section => {
    const hash = new Uint8Array(await crypto.subtle.digest('SHA-256', encoder.encode(section.key)));
    return Array.from(hash.slice(0, 10), byte => byte.toString(16).padStart(2, '0')).join('');
  }));
}

export function headingFolds(noteId: () => string | undefined) {
  return [codeFolding({ placeholderDOM(view) {
    // The control belongs to the title line even when CodeMirror splits the replacement off it.
    const placeholder = view.dom.ownerDocument.createElement('span');
    placeholder.hidden = true;
    return placeholder;
  } }), EditorView.atomicRanges.of(view => foldedRanges(view.state)),
  Prec.high(keymap.of([{ key: 'Enter', run(view) {
    const selection = view.state.selection.main;
    if (!selection.empty || view.state.readOnly || view.compositionStarted) return false;
    const range = foldAt(view.state, selection.head);
    if (!range) return false;
    view.dispatch({ changes: { from: range.to, insert: '\n' }, selection: { anchor: range.to + 1 }, userEvent: 'input', scrollIntoView: true });
    return true;
  } }])), EditorState.transactionExtender.of(transaction => {
    const folded = foldedRanges(transaction.startState);
    if (!transaction.docChanged || !folded.size) return null;
    const effects: StateEffect<{ from: number; to: number }>[] = [];
    folded.between(0, transaction.startState.doc.length, (from, to) => {
      const mapped = { from: transaction.changes.mapPos(from, 1), to: transaction.changes.mapPos(to, -1) };
      const line = transaction.newDoc.lineAt(mapped.from);
      if (mapped.from >= mapped.to) return;
      if (line.to !== mapped.from || !heading.test(line.text) || transaction.changes.touchesRange(from + 1, to - 1)) {
        effects.push(unfoldEffect.of(mapped));
      }
    });
    return effects.length ? { effects } : null;
  }), ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    private headings: Section[];
    private revision = 0;
    private disposed = false;
    private storageKey?: string;

    constructor(view: EditorView) {
      this.headings = sections(view.state);
      this.decorations = this.render(view);
      const id = noteId();
      if (id) this.storageKey = `qingyu.heading-folds.${id}`;
      void this.restore(view);
    }

    private async restore(view: EditorView) {
      if (!this.storageKey) return;
      try {
        const stored: unknown = JSON.parse(localStorage.getItem(this.storageKey) ?? 'null');
        if (!Array.isArray(stored) || !stored.length) return;
        const revision = this.revision;
        const doc = view.state.doc;
        const current = () => !this.disposed && this.revision === revision && view.state.doc === doc;
        await Promise.resolve();
        const options = await parsedSections(view, current);
        const keys = await hashedKeys(options);
        if (current()) collapse(view, options.filter((_, index) => keys[index] && stored.includes(keys[index])));
      } catch { /* Fold preferences can be unavailable without affecting the note. */ }
    }

    private async save(view: EditorView) {
      if (!this.storageKey) return;
      const revision = ++this.revision;
      try {
        const keys = await hashedKeys(this.headings.filter(section => foldAt(view.state, section.from)));
        if (revision !== this.revision) return;
        if (keys.length) localStorage.setItem(this.storageKey, JSON.stringify(keys));
        else localStorage.removeItem(this.storageKey);
      } catch { /* Fold preferences can be unavailable without affecting the note. */ }
    }

    private render(view: EditorView): DecorationSet {
      const head = view.state.doc.lineAt(view.state.selection.main.head).from;
      return Decoration.set(this.headings.flatMap(section => {
        if (!view.visibleRanges.some(range => section.line >= range.from && section.line <= range.to)) return [];
        const collapsed = Boolean(foldAt(view.state, section.from));
        const toggles = [Decoration.line({ class: 'qingyu-fold-heading', attributes: { 'data-fold-caret': String(head === section.line) } }).range(section.line),
          Decoration.widget({ side: -1, widget: new HeadingFoldButton(section, collapsed) }).range(section.line)];
        if (collapsed) toggles.push(Decoration.widget({ side: -1, widget: new HeadingFoldButton(section, true, true) }).range(section.from));
        return toggles;
      }), true);
    }

    update(update: ViewUpdate) {
      const parsed = update.docChanged || syntaxTree(update.startState) !== syntaxTree(update.state);
      const foldsChanged = foldedRanges(update.startState) !== foldedRanges(update.state);
      if (parsed) this.headings = sections(update.state);
      if (parsed || foldsChanged || update.selectionSet || update.viewportChanged) this.decorations = this.render(update.view);
      if (update.transactions.some(transaction => transaction.effects.some(effect => effect.is(reloadHeadingFolds)))) {
        this.revision++;
        void this.restore(update.view);
      } else if (foldsChanged || update.docChanged && foldedRanges(update.state).size) void this.save(update.view);
    }

    destroy() { this.disposed = true; }
  }, { decorations: plugin => plugin.decorations })];
}
