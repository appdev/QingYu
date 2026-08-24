import { syntaxTree } from "@codemirror/language";
import {StateEffect, StateField, type EditorState, type Range} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type EditorView as CodeMirrorView,
} from "@codemirror/view";
import {
  createMarkraMathMacros,
  isMarkraMathMacroDefinitionSource,
  renderMarkraMathToString,
  type MarkraMathKind,
  type MarkraMathMacros,
} from "../math-render";
import { defineMarkraPlugin } from "./plugin";
import {selectionRevealsRange} from "./policy";
import {syntaxTreeChanged, transactionChangesStayAfter} from "./changes";
import {codeMirrorVimModeChangedEffect} from "./vim";

export interface CodeMirrorMathRange {
  readonly from: number;
  readonly kind: MarkraMathKind;
  readonly source: string;
  readonly tex: string;
  readonly to: number;
}

interface SourceRange {
  readonly from: number;
  readonly to: number;
}

const codeNodeNames = new Set(["CodeBlock", "FencedCode", "InlineCode"]);

function isEscaped(source: string, index: number) {
  let count = 0;
  for (let cursor = index - 1; cursor >= 0 && source[cursor] === "\\"; cursor -= 1) {
    count += 1;
  }
  return count % 2 === 1;
}

function overlaps(range: SourceRange, other: SourceRange) {
  return range.from < other.to && range.to > other.from;
}

function insideAnyRange(from: number, to: number, ranges: readonly SourceRange[]) {
  return ranges.some((range) => overlaps({ from, to }, range));
}

function codeRanges(state: EditorState) {
  const ranges: SourceRange[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (codeNodeNames.has(node.name)) ranges.push({ from: node.from, to: node.to });
    },
  });
  return ranges;
}

function findClosingDelimiter(
  source: string,
  from: number,
  delimiter: string,
  blocked: readonly SourceRange[],
) {
  let cursor = from;
  while (cursor < source.length) {
    const match = source.indexOf(delimiter, cursor);
    if (match < 0) return null;
    if (!isEscaped(source, match) && !insideAnyRange(match, match + delimiter.length, blocked)) {
      return match;
    }
    cursor = match + delimiter.length;
  }
  return null;
}

function displayMathRanges(source: string, blocked: readonly SourceRange[]) {
  const ranges: CodeMirrorMathRange[] = [];
  const delimiters = [
    { close: "$$", open: "$$" },
    { close: String.raw`\]`, open: String.raw`\[` },
  ] as const;

  for (const { close, open } of delimiters) {
    let cursor = 0;
    while (cursor < source.length) {
      const from = source.indexOf(open, cursor);
      if (from < 0) break;
      if (isEscaped(source, from) || insideAnyRange(from, from + open.length, blocked)) {
        cursor = from + open.length;
        continue;
      }

      const closeFrom = findClosingDelimiter(
        source,
        from + open.length,
        close,
        blocked,
      );
      if (closeFrom === null) break;

      const to = closeFrom + close.length;
      const range = {
        from,
        kind: "display" as const,
        source: source.slice(from, to),
        tex: source.slice(from + open.length, closeFrom).trim(),
        to,
      };
      ranges.push(range);
      blocked = [...blocked, range];
      cursor = to;
    }
  }

  return ranges;
}

function inlineDollarRanges(source: string, blocked: readonly SourceRange[]) {
  const ranges: CodeMirrorMathRange[] = [];
  let cursor = 0;

  while (cursor < source.length) {
    const from = source.indexOf("$", cursor);
    if (from < 0) break;
    const afterOpen = source[from + 1];
    if (
      source[from + 1] === "$" ||
      source[from - 1] === "$" ||
      isEscaped(source, from) ||
      !afterOpen ||
      /\s/u.test(afterOpen) ||
      insideAnyRange(from, from + 1, blocked)
    ) {
      cursor = from + 1;
      continue;
    }

    let closeFrom = from + 1;
    while (closeFrom < source.length) {
      closeFrom = source.indexOf("$", closeFrom);
      if (closeFrom < 0 || source.slice(from, closeFrom).includes("\n")) break;
      const beforeClose = source[closeFrom - 1];
      if (
        source[closeFrom + 1] !== "$" &&
        source[closeFrom - 1] !== "$" &&
        !isEscaped(source, closeFrom) &&
        beforeClose &&
        !/\s/u.test(beforeClose) &&
        !insideAnyRange(closeFrom, closeFrom + 1, blocked)
      ) {
        const to = closeFrom + 1;
        ranges.push({
          from,
          kind: "inline",
          source: source.slice(from, to),
          tex: source.slice(from + 1, closeFrom),
          to,
        });
        cursor = to;
        break;
      }
      closeFrom += 1;
    }

    if (closeFrom < 0 || source.slice(from, closeFrom).includes("\n")) cursor = from + 1;
  }

  return ranges;
}

function inlineHugoRanges(source: string, blocked: readonly SourceRange[]) {
  const ranges: CodeMirrorMathRange[] = [];
  const open = String.raw`\(`;
  const close = String.raw`\)`;
  let cursor = 0;

  while (cursor < source.length) {
    const from = source.indexOf(open, cursor);
    if (from < 0) break;
    if (insideAnyRange(from, from + open.length, blocked)) {
      cursor = from + open.length;
      continue;
    }
    const closeFrom = source.indexOf(close, from + open.length);
    if (
      closeFrom < 0 ||
      source.slice(from, closeFrom).includes("\n") ||
      insideAnyRange(closeFrom, closeFrom + close.length, blocked)
    ) {
      cursor = from + open.length;
      continue;
    }

    const to = closeFrom + close.length;
    ranges.push({
      from,
      kind: "inline",
      source: source.slice(from, to),
      tex: source.slice(from + open.length, closeFrom),
      to,
    });
    cursor = to;
  }

  return ranges;
}

export function findCodeMirrorMathRanges(state: EditorState) {
  const source = state.doc.toString();
  const code = codeRanges(state);
  const display = displayMathRanges(source, code);
  const blocked = [...code, ...display];
  const inline = [
    ...inlineDollarRanges(source, blocked),
    ...inlineHugoRanges(source, blocked),
  ];
  return [...display, ...inline].sort((left, right) => left.from - right.from);
}

function activateMath(view: CodeMirrorView, range: CodeMirrorMathRange) {
  const offset = range.source.startsWith("$$") || range.source.startsWith(String.raw`\[`)
    ? 2
    : 1;
  view.dispatch({ selection: { anchor: Math.min(range.to - 1, range.from + offset) } });
  view.focus();
}

class MathWidget extends WidgetType {
  constructor(
    readonly range: CodeMirrorMathRange,
    readonly html: string,
    readonly className: string,
  ) {
    super();
  }

  get estimatedHeight() {
    if (this.range.kind !== "display") return -1;
    return Math.max(48, this.range.source.split("\n").length * 26);
  }

  eq(other: MathWidget) {
    return (
      other.range.source === this.range.source &&
      other.html === this.html &&
      other.className === this.className
    );
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const element = view.dom.ownerDocument.createElement("span");
    element.className = this.className;
    element.innerHTML = this.html;
    if (this.range.kind === "display") {
      const bases = element.querySelectorAll(".katex-html > .base");
      const lastBase = bases[bases.length - 1];
      if (lastBase) {
        const balance = view.dom.ownerDocument.createElement("span");
        balance.className = "fn__flex-1";
        lastBase.after(balance);
      }
    }
    const hasRenderError = element.querySelector(".katex-error") !== null;
    element.dataset.appearanceState = hasRenderError ? "error" : "ready";
    if (hasRenderError) element.setAttribute("aria-invalid", "true");
    element.tabIndex = 0;
    element.setAttribute("role", "button");
    element.setAttribute("aria-label", "Edit math source");
    const activate = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      activateMath(view, this.range);
    };
    element.addEventListener("mousedown", activate);
    element.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") activate(event);
    });
    return element;
  }
}

class MacroFoldWidget extends WidgetType {
  constructor(readonly range: CodeMirrorMathRange) {
    super();
  }

  get estimatedHeight() {
    return this.range.source.includes("\n") ? 32 : -1;
  }

  eq(other: MacroFoldWidget) {
    return other.range.source === this.range.source;
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const button = view.dom.ownerDocument.createElement("button");
    button.className = "markra-math-macro-fold";
    button.type = "button";
    button.textContent = String.raw`\newcommand …`;
    button.addEventListener("mousedown", (event) => {
      event.preventDefault();
      event.stopPropagation();
      activateMath(view, this.range);
    });
    return button;
  }
}

function addMathReplacement(
  ranges: Range<Decoration>[],
  range: CodeMirrorMathRange,
  widget: WidgetType,
) {
  ranges.push(Decoration.replace({block: range.source.includes("\n"), widget}).range(range.from, range.to));
}

function renderMath(
  range: CodeMirrorMathRange,
  macros: MarkraMathMacros,
) {
  return renderMarkraMathToString(range.tex, range.kind, macros);
}

interface MathDecorationState {
  readonly decorations: DecorationSet;
  readonly entries: readonly MathRenderEntry[];
  readonly context: MathPreviewContext;
  readonly lastRangeTo: number;
}

interface MathRenderEntry {
  readonly html: string;
  readonly macroDefinitionOnly: boolean;
  readonly range: CodeMirrorMathRange;
}

interface MathPreviewContext {
  readonly focused: boolean;
  readonly vimNormalMode: boolean;
}

function buildMathRenderEntries(state: EditorState): MathRenderEntry[] {
  const macros = createMarkraMathMacros();
  return findCodeMirrorMathRanges(state).map((range) => ({
    html: renderMath(range, macros),
    macroDefinitionOnly: range.kind === "display" && isMarkraMathMacroDefinitionSource(range.tex),
    range,
  }));
}

function buildMathDecorations(state: EditorState, entries: readonly MathRenderEntry[], context: MathPreviewContext) {
  const ranges: Range<Decoration>[] = [];

  for (const {html, macroDefinitionOnly, range} of entries) {
    const active = selectionRevealsRange(state, context.focused, context.vimNormalMode, range.from, range.to);

    if (macroDefinitionOnly) {
      if (active) continue;
      addMathReplacement(ranges, range, new MacroFoldWidget(range));
      continue;
    }

    if (active) {
      if (range.kind === "display") {
        ranges.push(
          Decoration.widget({
            block: range.source.includes("\n"),
            side: 1,
            widget: new MathWidget(
              range,
              html,
              "markra-math-render markra-math-render-display markra-math-render-active-preview",
            ),
          }).range(range.to),
        );
      }
      continue;
    }

    addMathReplacement(ranges, range, new MathWidget(
      range, html, `markra-math-render markra-math-render-${range.kind}`,
    ));
  }

  return Decoration.set(ranges, true);
}

function createMathDecorationState(state: EditorState, context: MathPreviewContext): MathDecorationState {
  const entries = buildMathRenderEntries(state);

  return {
    context,
    decorations: buildMathDecorations(state, entries, context),
    entries,
    lastRangeTo: Math.max(-1, ...entries.map(({range}) => range.to)),
  };
}

const mathTheme = EditorView.baseTheme({
  ".markra-math-render": {cursor: "text"},
});

const setMathPreviewFocusedEffect = StateEffect.define<boolean>();

function createMathPreviewExtension() {
  const initialContext = {focused: true, vimNormalMode: false};
  const field = StateField.define<MathDecorationState>({
    create: (state) => createMathDecorationState(state, initialContext),
    update(previous, transaction) {
      const focusEffect = transaction.effects.find((effect) => effect.is(setMathPreviewFocusedEffect));
      const vimEffect = transaction.effects.find((effect) => effect.is(codeMirrorVimModeChangedEffect));
      const context = {
        focused: focusEffect?.value ?? previous.context.focused,
        vimNormalMode: vimEffect?.value ?? previous.context.vimNormalMode,
      };
      if (transactionChangesStayAfter(transaction, previous.lastRangeTo, (source) =>
        ["$", "\\", "`", "~", "\n"].some((marker) => source.includes(marker)))) {
        return {...previous, context, decorations: previous.decorations.map(transaction.changes)};
      }
      if (transaction.docChanged || syntaxTreeChanged(transaction.startState, transaction.state)) {
        return createMathDecorationState(transaction.state, context);
      }
      if (context.focused === previous.context.focused && context.vimNormalMode === previous.context.vimNormalMode &&
        transaction.selection === undefined) return previous;
      return {...previous, context, decorations: buildMathDecorations(transaction.state, previous.entries, context)};
    },
    provide: (mathField) => EditorView.decorations.from(mathField, (value) => value.decorations),
  });
  const mounted = new WeakSet<CodeMirrorView>();
  const syncFocus = (view: CodeMirrorView, focused: boolean) => {
    if (!mounted.has(view) || view.compositionStarted) return;
    const current = view.state.field(field, false);
    if (current && current.context.focused !== focused) {
      const selection = view.state.selection;
      view.dispatch({effects: setMathPreviewFocusedEffect.of(focused), selection});
    }
  };
  return [
    field,
    ViewPlugin.define((view) => {
      mounted.add(view);
      queueMicrotask(() => syncFocus(view, view.hasFocus));
      return {destroy: () => mounted.delete(view)};
    }),
    EditorView.domEventHandlers({
      blur: (_event, view) => (syncFocus(view, false), false),
      focus: (_event, view) => {
        window.setTimeout(() => syncFocus(view, true));
        return false;
      },
      compositionend: (_event, view) => (syncFocus(view, view.hasFocus), false),
    }),
  ];
}

export function mathPreviewPlugin() {
  return defineMarkraPlugin({
    id: "markra.math-preview",
    extension: [
      ...createMathPreviewExtension(),
      mathTheme,
    ],
  });
}
