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
  type MarkraMathMacros,
} from "../math-render";
import {findMarkraMathRanges, type MarkraMathRange, type MarkraSourceRange} from "../math-syntax";
import { defineMarkraPlugin } from "./plugin";
import {selectionRevealsRange} from "./policy";
import {syntaxTreeChanged, transactionChangesStayAfter} from "./changes";
import {codeMirrorVimModeChangedEffect} from "./vim";

export type CodeMirrorMathRange = MarkraMathRange;

const codeNodeNames = new Set(["CodeBlock", "FencedCode", "InlineCode"]);

function codeRanges(state: EditorState) {
  const ranges: MarkraSourceRange[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (codeNodeNames.has(node.name)) ranges.push({ from: node.from, to: node.to });
    },
  });
  return ranges;
}

export function findCodeMirrorMathRanges(state: EditorState) {
  return findMarkraMathRanges(state.doc.toString(), codeRanges(state));
}

function activateMath(
  view: CodeMirrorView,
  range: CodeMirrorMathRange,
  direction: "backward" | "forward" = "forward",
) {
  const offset = range.source.startsWith("$$") || range.source.startsWith(String.raw`\[`)
    ? 2
    : 1;
  const openingBreak = range.source.slice(offset).match(/^\r?\n/u)?.[0].length ?? 0;
  const closingStart = Math.max(offset, range.source.length - offset);
  const closingBreak = range.source.slice(0, closingStart).match(/\r?\n$/u)?.[0].length ?? 0;
  const contentFrom = range.from + offset + openingBreak;
  const contentTo = range.from + closingStart - closingBreak;
  const anchor = direction === "forward"
    ? Math.min(contentTo, contentFrom)
    : Math.max(contentFrom, contentTo);
  view.dispatch({ selection: { anchor }, scrollIntoView: true });
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
    visualBlocks: [{
      read(state) {
        return findCodeMirrorMathRanges(state)
          .filter((range) => range.kind === "display")
          .map((range) => ({
            from: range.from,
            to: range.to,
            enter(view: CodeMirrorView, direction: "backward" | "forward") {
              activateMath(view, range, direction);
              return true;
            },
          }));
      },
    }],
    extension: [
      ...createMathPreviewExtension(),
      mathTheme,
    ],
  });
}
