import { syntaxTree } from "@codemirror/language";
import {
  EditorSelection,
  EditorState,
  Facet,
  Prec,
  StateEffect,
  StateField,
  type Transaction,
} from "@codemirror/state";
import { keymap, type EditorView } from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin";

export interface MarkraDocumentLinkItem {
  readonly detail?: string;
  readonly href: string;
  readonly id: string;
  readonly keywords?: readonly string[];
  readonly label: string;
  readonly markdown?: string;
}

export interface MarkraDocumentLinksContext {
  readonly query: string;
  readonly state: EditorState;
  readonly view: EditorView;
}

export interface MarkraDocumentLinkAction extends MarkraDocumentLinkItem {
  run: () => boolean;
}

export interface MarkraDocumentLinksState {
  readonly from: number | null;
  readonly items: readonly MarkraDocumentLinkAction[];
  readonly open: boolean;
  readonly query: string;
  readonly selectedIndex: number;
  readonly to: number | null;
}

export interface DocumentLinksPluginOptions {
  readonly items:
    | readonly MarkraDocumentLinkItem[]
    | ((context: MarkraDocumentLinksContext) => readonly MarkraDocumentLinkItem[]);
}

interface CompletionRange {
  from: number;
  query: string;
  to: number;
}

interface InternalDocumentLinksState {
  active: CompletionRange | null;
  selectedIndex: number;
  suppressed: CompletionRange | null;
}

type DocumentLinksEffect =
  | { type: "close" }
  | { index: number; type: "select" };

interface SyntaxNodeLike {
  name: string;
  parent: SyntaxNodeLike | null;
}

const updateDocumentLinks = StateEffect.define<DocumentLinksEffect>();
const documentLinksOptions = Facet.define<
  DocumentLinksPluginOptions,
  DocumentLinksPluginOptions | null
>({
  combine: (values) => values[0] ?? null,
});

function isInsideCodeBlock(state: EditorState, position: number) {
  let node: SyntaxNodeLike | null = syntaxTree(state).resolve(position, -1);
  while (node) {
    if (node.name === "FencedCode" || node.name === "CodeBlock") return true;
    node = node.parent;
  }
  return false;
}

function completionRangeFromState(state: EditorState): CompletionRange | null {
  if (state.facet(EditorState.readOnly) || state.selection.ranges.length !== 1) {
    return null;
  }
  const selection = state.selection.main;
  if (!selection.empty || isInsideCodeBlock(state, selection.head)) return null;

  const line = state.doc.lineAt(selection.head);
  const beforeCursor = state.sliceDoc(line.from, selection.head);
  const match = /\[\[([^\]\n]*)$/u.exec(beforeCursor);
  if (!match) return null;
  const typed = match[0] ?? "";

  return {
    from: selection.head - typed.length,
    query: match[1] ?? "",
    to: selection.head,
  };
}

function sameRange(left: CompletionRange | null, right: CompletionRange | null) {
  return Boolean(
    left &&
      right &&
      left.from === right.from &&
      left.query === right.query &&
      left.to === right.to,
  );
}

function effectFrom(transaction: Transaction) {
  return transaction.effects.find((effect) => effect.is(updateDocumentLinks))
    ?.value;
}

const documentLinksField = StateField.define<InternalDocumentLinksState>({
  create(state) {
    return {
      active: completionRangeFromState(state),
      selectedIndex: 0,
      suppressed: null,
    };
  },
  update(previous, transaction) {
    const effect = effectFrom(transaction);
    const active = completionRangeFromState(transaction.state);

    if (effect?.type === "close") {
      return { active: null, selectedIndex: 0, suppressed: active };
    }
    if (sameRange(active, previous.suppressed)) {
      return { active: null, selectedIndex: 0, suppressed: previous.suppressed };
    }

    return {
      active,
      selectedIndex:
        effect?.type === "select"
          ? effect.index
          : sameRange(active, previous.active)
            ? previous.selectedIndex
            : 0,
      suppressed: null,
    };
  },
});

function internalState(view: EditorView) {
  return view.state.field(documentLinksField, false);
}

function normalizedSearchText(value: string) {
  return value.normalize("NFKC").toLocaleLowerCase().replace(/\s+/gu, "");
}

function documentItems(view: EditorView, active: CompletionRange) {
  const options = view.state.facet(documentLinksOptions);
  if (!options) return [];

  let source: readonly MarkraDocumentLinkItem[];
  try {
    source =
      typeof options.items === "function"
        ? options.items({ query: active.query, state: view.state, view })
        : options.items;
  } catch {
    return [];
  }

  const query = normalizedSearchText(active.query);
  const ids = new Set<string>();
  return source.filter((item) => {
    if (!item.id.trim() || !item.label.trim() || !item.href.trim()) return false;
    if (ids.has(item.id)) return false;
    ids.add(item.id);
    if (!query) return true;
    return [
      item.id,
      item.label,
      item.detail ?? "",
      item.href,
      ...(item.keywords ?? []),
    ].some((candidate) => normalizedSearchText(candidate).includes(query));
  });
}

function escapedLabel(label: string) {
  return label.replace(/[\\\[\]]/gu, "\\$&");
}

function escapedHref(href: string) {
  return href
    .replace(/\s/gu, (character) => encodeURIComponent(character))
    .replace(/[\\()]/gu, "\\$&");
}

function markdownFor(item: MarkraDocumentLinkItem) {
  return item.markdown ?? `[${escapedLabel(item.label)}](${escapedHref(item.href)})`;
}

function moveSelection(view: EditorView, amount: -1 | 1) {
  const completion = getMarkraDocumentLinksState(view);
  if (!completion.open || completion.items.length === 0) return false;
  const index =
    (completion.selectedIndex + amount + completion.items.length) %
    completion.items.length;
  view.dispatch({
    effects: updateDocumentLinks.of({ index, type: "select" }),
  });
  return true;
}

function runSelectedItem(view: EditorView) {
  return runMarkraDocumentLink(view);
}

const documentLinksKeymap = Prec.highest(
  keymap.of([
    { key: "ArrowDown", run: (view) => moveSelection(view, 1) },
    { key: "ArrowUp", run: (view) => moveSelection(view, -1) },
    { key: "Enter", run: runSelectedItem },
    { key: "Tab", run: runSelectedItem },
    { key: "Escape", run: closeMarkraDocumentLinks },
  ]),
);

export function documentLinksPlugin(options: DocumentLinksPluginOptions) {
  return defineMarkraPlugin({
    id: "markra.document-links",
    extension: [
      documentLinksOptions.of(options),
      documentLinksField,
      documentLinksKeymap,
    ],
  });
}

export function getMarkraDocumentLinksState(
  view: EditorView,
): MarkraDocumentLinksState {
  const state = internalState(view);
  if (!state?.active) {
    return {
      from: null,
      items: [],
      open: false,
      query: "",
      selectedIndex: 0,
      to: null,
    };
  }

  const { active } = state;
  const source = documentItems(view, active);
  const selectedIndex = Math.min(
    state.selectedIndex,
    Math.max(source.length - 1, 0),
  );
  const items = source.map((item) => ({
    ...item,
    run: () => runMarkraDocumentLink(view, item.id),
  }));

  return {
    from: active.from,
    items,
    open: true,
    query: active.query,
    selectedIndex,
    to: active.to,
  };
}

export function closeMarkraDocumentLinks(view: EditorView) {
  if (!internalState(view)?.active) return false;
  view.dispatch({ effects: updateDocumentLinks.of({ type: "close" }) });
  return true;
}

export function runMarkraDocumentLink(view: EditorView, itemId?: string) {
  const completion = getMarkraDocumentLinksState(view);
  if (!completion.open || completion.from === null || completion.to === null) {
    return false;
  }
  const item = itemId
    ? completion.items.find((candidate) => candidate.id === itemId)
    : completion.items[completion.selectedIndex];
  if (!item) return false;
  const markdown = markdownFor(item);
  if (!markdown) return false;

  view.dispatch({
    changes: { from: completion.from, insert: markdown, to: completion.to },
    effects: updateDocumentLinks.of({ type: "close" }),
    selection: EditorSelection.cursor(completion.from + markdown.length),
    userEvent: "input.complete",
  });
  view.focus();
  return true;
}
