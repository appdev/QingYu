import { syntaxTree } from "@codemirror/language";
import {
  EditorSelection,
  Prec,
  StateEffect,
  StateField,
  type Range,
  type EditorState,
  type Transaction,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  keymap,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type EditorView as CodeMirrorView,
} from "@codemirror/view";
import {
  highlightMarkraCode,
  normalizeMarkraCodeLanguage,
} from "../code-support.ts";
import {
  formatMermaidError,
  isMermaidLanguage,
  mermaidThemeFromElement,
  renderMermaidToSvg,
} from "../mermaid.ts";
import { syntaxTreeChanged } from "./changes.ts";
import { defineMarkraPlugin } from "./plugin.ts";
import {
  createMediaViewerEnlargeIcon,
  openMediaViewer,
  type MediaViewerHandle,
} from "./media-viewer.ts";
import {
  markraRenderer,
  type MarkraRendererContext,
  type MarkraSyntaxNode,
} from "./renderers.ts";

export interface CodeBlockHighlightSpan {
  readonly className: string;
  readonly from: number;
  readonly to: number;
}

export interface CodeBlockHighlightContext {
  readonly code: string;
  readonly language: string;
  readonly state: EditorState;
  readonly view: CodeMirrorView;
}

export interface CodeBlockPreviewPluginOptions {
  highlight?: (
    context: CodeBlockHighlightContext,
  ) => readonly CodeBlockHighlightSpan[];
  labels?: Partial<CodeBlockPreviewLabels>;
  showLineNumbers?: boolean;
  renderMermaid?: (
    context: CodeBlockMermaidContext,
  ) => Promise<string | HTMLElement>;
}

export interface CodeBlockMermaidContext {
  readonly language: string;
  readonly source: string;
  readonly theme: string;
  readonly view: CodeMirrorView;
}

// QingYu keeps its legacy diagram formats on the same block widget as Mermaid.
function isPreviewDiagram(language: string) {
  return isMermaidLanguage(language) || /^(markmap|plantuml)$/i.test(language);
}

export interface CodeBlockPreviewLabels {
  readonly mermaidDiagram: string;
  readonly mermaidError: string;
}

interface CodeBlockParts {
  code: string;
  codeNode: MarkraSyntaxNode | null;
  hasClosingFence: boolean;
  language: string;
  languageLabel: string;
}

const defaultLabels: CodeBlockPreviewLabels = {
  mermaidDiagram: "Mermaid diagram",
  mermaidError: "Unable to render Mermaid diagram",
};

const codeBlockTheme = EditorView.baseTheme({
  ".cm-markra-code-line": {
    backgroundColor: "color-mix(in srgb, currentColor 5%, transparent)",
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    paddingLeft: "0.85em",
    paddingRight: "0.85em",
  },
  ".cm-markra-code-content-line": {
    position: "relative",
  },
  ".cm-markra-code-language": {
    position: "absolute",
    right: "14px",
    top: "0",
    pointerEvents: "none",
  },
  ".cm-markra-code-content-line[data-code-line-number]::before": {
    color: "color-mix(in srgb, currentColor 38%, transparent)",
    content: "attr(data-code-line-number)",
    display: "inline-block",
    marginRight: "1em",
    minWidth: "2ch",
    textAlign: "right",
    userSelect: "none",
  },
  ".cm-markra-code-exit-wrap": {
    display: "inline-block",
    height: "100%",
    width: "100%",
  },
  ".cm-markra-code-exit": {
    cursor: "text",
    display: "inline-block",
    height: "100%",
    width: "100%",
  },
  ".cm-markra-code-source-line": {
    color: "color-mix(in srgb, currentColor 72%, transparent)",
  },
  ".markra-mermaid-render": {
    background: "color-mix(in srgb, currentColor 3%, transparent)",
    border: "1px solid color-mix(in srgb, currentColor 12%, transparent)",
    borderRadius: "0.45em",
    cursor: "text",
    display: "block",
    margin: "0.5em 0",
    minHeight: "3.5em",
    overflow: "auto",
    padding: "0.85em",
    position: "relative",
    textAlign: "center",
  },
  // An inline-block avoids the empty line boxes WebKit can produce around
  // CodeMirror's widget buffers while still letting the preview fill the row.
  ".markra-code-block[data-mermaid-mode='preview']": {
    boxSizing: "border-box",
    display: "inline-block",
    margin: "0",
    maxWidth: "100%",
    verticalAlign: "top",
    width: "100%",
  },
  ".markra-code-block[data-mermaid-mode='preview'] .markra-mermaid-render": {
    background: "transparent",
    border: "0",
    margin: "0",
    padding: "0",
  },
  ".markra-mermaid-render svg": {
    height: "auto",
    maxWidth: "100%",
  },
  ".markra-mermaid-render-invalid": {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
    fontSize: "0.86em",
    maxHeight: "18em",
    overflow: "auto",
    overflowWrap: "anywhere",
    textAlign: "left",
    whiteSpace: "pre-wrap",
  },
});

class CodeLanguageWidget extends WidgetType {
  constructor(readonly language: string) {
    super();
  }

  eq(other: CodeLanguageWidget) {
    return this.language === other.language;
  }

  toDOM(view: CodeMirrorView) {
    const label = view.dom.ownerDocument.createElement("span");
    label.className = "cm-markra-code-language";
    label.textContent = this.language;
    return label;
  }
}

function moveSelectionAfterCodeBlock(
  view: CodeMirrorView,
  requestedAfterFence: number,
  removeLine?: { from: number; to: number },
) {
  const afterFence = Math.min(requestedAfterFence, view.state.doc.length);
  const hasFollowingLineBreak =
    view.state.sliceDoc(afterFence, afterFence + 1) === "\n";
  const canMaterializeLine = !hasFollowingLineBreak && !view.state.readOnly;
  const changes = view.state.changes([
    ...(removeLine ? [removeLine] : []),
    ...(canMaterializeLine ? [{ from: afterFence, insert: "\n" }] : []),
  ]);

  // The closing fence is visually folded. Materialize/select the line after
  // it so clicks in the visual gap can never append code inside the fence.
  view.dispatch({
    changes,
    scrollIntoView: true,
    selection: EditorSelection.cursor(
      changes.mapPos(afterFence, 1) + (hasFollowingLineBreak ? 1 : 0),
    ),
  });
  view.focus();
}

class CodeBlockExitWidget extends WidgetType {
  constructor(readonly afterFence: number) {
    super();
  }

  eq(other: CodeBlockExitWidget) {
    return this.afterFence === other.afterFence;
  }

  ignoreEvent() {
    return true;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const wrapper = document.createElement("span");
    const exit = document.createElement("span");
    wrapper.className = "cm-markra-code-exit-wrap";
    exit.className = "cm-markra-code-exit";
    exit.setAttribute("aria-hidden", "true");

    exit.addEventListener("mousedown", (event) => {
      if (event.button !== 0 || view.state.readOnly) return;
      event.preventDefault();
      event.stopPropagation();
      moveSelectionAfterCodeBlock(view, this.afterFence);
    });
    wrapper.append(exit);
    return wrapper;
  }
}

interface MermaidPreviewRuntime {
  mediaViewer: MediaViewerHandle | null;
  observer: MutationObserver | null;
  renderToken: number;
}

const mermaidPreviewRuntimes = new WeakMap<
  HTMLElement,
  MermaidPreviewRuntime
>();

function removeEmptyMermaidLabels(preview: HTMLElement) {
  for (const emptyLabel of preview.querySelectorAll(
    'foreignObject[width="0"][height="0"]',
  )) {
    const label = emptyLabel.parentElement;
    if (
      label?.classList.contains("label") &&
      label.childElementCount === 1 &&
      !label.textContent?.trim()
    ) {
      label.remove();
      continue;
    }
    emptyLabel.remove();
  }
}

class MermaidPreviewWidget extends WidgetType {
  constructor(
    readonly sourceOffset: number,
    readonly labels: CodeBlockPreviewLabels,
    readonly renderMermaid: NonNullable<CodeBlockPreviewPluginOptions["renderMermaid"]>,
    readonly source: string,
    readonly language: string,
  ) {
    super();
  }

  eq(other: MermaidPreviewWidget) {
    return (
      this.source === other.source &&
      this.language === other.language &&
      this.sourceOffset === other.sourceOffset
    );
  }

  ignoreEvent() {
    return true;
  }

  private closeViewer(runtime: MermaidPreviewRuntime) {
    runtime.mediaViewer?.close({ restoreFocus: false });
    runtime.mediaViewer = null;
  }

  private openZoom(
    runtime: MermaidPreviewRuntime,
    view: CodeMirrorView,
    preview: HTMLElement,
    trigger: HTMLButtonElement,
  ) {
    const sourceSvg = preview.querySelector("svg");
    if (!sourceSvg) return;
    this.closeViewer(runtime);
    runtime.mediaViewer = openMediaViewer({
      labels: {
        close: "Close enlarged Mermaid diagram",
        dialog: "Enlarged Mermaid diagram",
        enterFullscreen: "Enter full screen",
        exitFullscreen: "Exit full screen",
        reset: "Reset Mermaid diagram view",
        viewport: "Mermaid diagram viewport",
        zoomIn: "Zoom in Mermaid diagram",
        zoomOut: "Zoom out Mermaid diagram",
      },
      media: sourceSvg,
      mount: view.dom.closest(".markdown-paper") ?? view.dom.ownerDocument.body,
      restoreFocus: trigger,
    });
  }

  private appendZoomButton(
    runtime: MermaidPreviewRuntime,
    view: CodeMirrorView,
    preview: HTMLElement,
    wrapper: HTMLElement,
  ) {
    if (!preview.querySelector("svg")) return;
    wrapper.querySelector(".markra-mermaid-zoom-button")?.remove();
    const button = view.dom.ownerDocument.createElement("button");
    button.type = "button";
    button.className = "markra-mermaid-zoom-button";
    button.ariaLabel = "Enlarge Mermaid diagram";
    button.title = "Enlarge Mermaid diagram";
    button.append(createMediaViewerEnlargeIcon(
      view.dom.ownerDocument,
      "markra-mermaid-zoom-icon",
    ));
    button.addEventListener("mousedown", (event) => event.stopPropagation());
    button.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      this.openZoom(runtime, view, preview, button);
    });
    wrapper.append(button);
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const wrapper = document.createElement("div");
    const preview = document.createElement("div");
    const runtime: MermaidPreviewRuntime = {
      mediaViewer: null,
      observer: null,
      renderToken: 0,
    };
    mermaidPreviewRuntimes.set(wrapper, runtime);
    wrapper.className = "markra-code-block";
    wrapper.dataset.mermaidMode = "preview";
    preview.className = "markra-mermaid-render";
    preview.tabIndex = 0;
    preview.ariaLabel = this.labels.mermaidDiagram;
    preview.setAttribute("aria-busy", "true");

    const revealSource = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      let anchor = this.sourceOffset;
      try {
        // The widget may move when text is inserted before an unchanged
        // diagram. Resolve its current document position from the reused DOM.
        anchor = view.posAtDOM(wrapper) + this.sourceOffset;
      } catch {
        // Fall back to the creation position if the DOM was already detached.
      }
      view.dispatch({
        scrollIntoView: true,
        selection: { anchor },
      });
      view.focus();
    };
    preview.addEventListener("click", revealSource);
    preview.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") revealSource(event);
    });

    const render = () => {
      runtime.renderToken += 1;
      const token = runtime.renderToken;
      const theme = mermaidThemeFromElement(preview);
      preview.setAttribute("aria-busy", "true");
      this.renderMermaid({ source: this.source, language: this.language, theme, view })
        .then((diagram) => {
          if (token !== runtime.renderToken) return;
          this.closeViewer(runtime);
          if (typeof diagram === "string") preview.innerHTML = diagram;
          else preview.replaceChildren(diagram);
          preview.classList.remove("markra-mermaid-render-invalid");
          delete preview.dataset.error;
          removeEmptyMermaidLabels(preview);
          this.appendZoomButton(runtime, view, preview, wrapper);
          preview.setAttribute("aria-busy", "false");
          view.requestMeasure();
        })
        .catch((error: unknown) => {
          if (token !== runtime.renderToken) return;
          this.closeViewer(runtime);
          wrapper.querySelector(".markra-mermaid-zoom-button")?.remove();
          preview.textContent = formatMermaidError(error, this.labels.mermaidError);
          preview.classList.add("markra-mermaid-render-invalid");
          preview.dataset.error = "true";
          preview.setAttribute("aria-busy", "false");
          view.requestMeasure();
        });
    };
    render();

    const MutationObserverConstructor = document.defaultView?.MutationObserver;
    if (MutationObserverConstructor) {
      runtime.observer = new MutationObserverConstructor(render);
      const options = {
        attributeFilter: ["data-editor-theme", "data-theme"],
        attributes: true,
      };
      const paper = view.dom.closest(".markdown-paper");
      if (paper) runtime.observer.observe(paper, options);
      runtime.observer.observe(document.documentElement, options);
    }
    wrapper.append(preview);
    return wrapper;
  }

  destroy(dom: HTMLElement) {
    const runtime = mermaidPreviewRuntimes.get(dom);
    if (!runtime) return;

    runtime.renderToken += 1;
    this.closeViewer(runtime);
    runtime.observer?.disconnect();
    runtime.observer = null;
    mermaidPreviewRuntimes.delete(dom);
  }
}

function codeBlockParts(
  state: EditorState,
  node: MarkraSyntaxNode,
): CodeBlockParts {
  const codeNode = node.getChild("CodeText");
  const infoNode = node.getChild("CodeInfo");
  const info = infoNode
    ? state.sliceDoc(infoNode.from, infoNode.to).trim()
    : "";
  const rawLanguage = info.split(/\s+/u)[0] ?? "";
  return {
    code: codeNode ? state.sliceDoc(codeNode.from, codeNode.to) : "",
    codeNode,
    hasClosingFence: node.getChildren("CodeMark").length > 1,
    language: normalizeMarkraCodeLanguage(rawLanguage),
    languageLabel: rawLanguage,
  };
}

const setMermaidPreviewFocusedEffect = StateEffect.define<boolean>();

interface MermaidPreviewState {
  readonly blocks: readonly MermaidPreviewBlock[];
  readonly decorations: DecorationSet;
  readonly focused: boolean;
}

interface MermaidPreviewBlock {
  readonly language: string;
  readonly from: number;
  readonly source: string;
  readonly sourceOffset: number;
  readonly to: number;
}

function mermaidSourceRevealed(
  state: EditorState,
  from: number,
  to: number,
  focused: boolean,
) {
  if (!focused) return false;
  return state.selection.ranges.some((selection) =>
    selection.empty
      ? selection.head > from && selection.head <= to
      : selection.anchor > from && selection.anchor <= to
  );
}

function readMermaidPreviewBlocks(state: EditorState) {
  const blocks: MermaidPreviewBlock[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.type.name !== "FencedCode") return;
      const parts = codeBlockParts(state, node.node as MarkraSyntaxNode);
      if (
        !parts.codeNode ||
        !parts.code.trim() ||
        !isPreviewDiagram(parts.language)
      ) {
        return;
      }
      blocks.push({
        from: node.from,
        language: parts.language,
        source: parts.code,
        sourceOffset: parts.codeNode.from - node.from,
        to: node.to,
      });
    },
  });
  return blocks;
}

function mermaidPreviewDecorationsFromBlocks(
  state: EditorState,
  focused: boolean,
  blocks: readonly MermaidPreviewBlock[],
  labels: CodeBlockPreviewLabels,
  renderMermaid: NonNullable<CodeBlockPreviewPluginOptions["renderMermaid"]>,
) {
  const previews: Range<Decoration>[] = [];
  for (const block of blocks) {
    if (mermaidSourceRevealed(state, block.from, block.to, focused)) continue;
    // Replace the complete fence with one block widget. Splitting the preview
    // across a first-line widget and hidden source lines makes CodeMirror
    // remount the expensive SVG while it reconciles the block's height map.
    previews.push(
      Decoration.replace({
        block: true,
        widget: new MermaidPreviewWidget(
          block.sourceOffset,
          labels,
          renderMermaid,
          block.source,
          block.language,
        ),
      }).range(block.from, block.to),
    );
  }
  return Decoration.set(previews, true);
}

function createMermaidPreviewState(
  state: EditorState,
  focused: boolean,
  labels: CodeBlockPreviewLabels,
  renderMermaid: NonNullable<CodeBlockPreviewPluginOptions["renderMermaid"]>,
): MermaidPreviewState {
  const blocks = readMermaidPreviewBlocks(state);
  return {
    blocks,
    decorations: mermaidPreviewDecorationsFromBlocks(
      state,
      focused,
      blocks,
      labels,
      renderMermaid,
    ),
    focused,
  };
}

function mapMermaidPreviewBlocks(
  blocks: readonly MermaidPreviewBlock[],
  transaction: Transaction,
) {
  return blocks.map((block) => ({
    ...block,
    from: transaction.changes.mapPos(block.from, 1),
    to: transaction.changes.mapPos(block.to, -1),
  }));
}

function changesTouchMermaidBlocks(
  transaction: Transaction,
  blocks: readonly MermaidPreviewBlock[],
) {
  let touched = false;
  transaction.changes.iterChangedRanges((fromA, toA) => {
    touched ||= blocks.some((block) =>
      fromA === toA
        ? fromA > block.from && fromA < block.to
        : fromA < block.to && toA > block.from
    );
  });
  return touched;
}

function changesMayAffectMermaidFences(transaction: Transaction) {
  let mayAffect = false;
  transaction.changes.iterChanges(
    (fromA, toA, fromB, _toB, inserted) => {
      if (fromA < toA || /[`~\r\n]/u.test(inserted.toString())) {
        mayAffect = true;
        return;
      }
      const line = transaction.state.doc.lineAt(fromB);
      mayAffect ||= /^\s*(?:`{3,}|~{3,})/u.test(line.text);
    },
  );
  return mayAffect;
}

function revealedMermaidBlocksKey(
  state: EditorState,
  focused: boolean,
  blocks: readonly MermaidPreviewBlock[],
) {
  return blocks.map((block) =>
    mermaidSourceRevealed(state, block.from, block.to, focused) ? "1" : "0"
  ).join("");
}

function createMermaidPreviewField(
  labels: CodeBlockPreviewLabels,
  renderMermaid: NonNullable<CodeBlockPreviewPluginOptions["renderMermaid"]>,
) {
  const field = StateField.define<MermaidPreviewState>({
    create(state) {
      return createMermaidPreviewState(
        state,
        true,
        labels,
        renderMermaid,
      );
    },
    update(previous, transaction) {
      // A long document can finish parsing in a later, document-neutral
      // transaction. Rebuild so newly discovered Mermaid fences get previews.
      const treeChanged = syntaxTreeChanged(
        transaction.startState,
        transaction.state,
      );
      const focusEffect = transaction.effects.find((effect) =>
        effect.is(setMermaidPreviewFocusedEffect)
      );
      const focused = focusEffect?.value ?? previous.focused;
      if (!transaction.docChanged) {
        if (treeChanged) {
          return createMermaidPreviewState(
            transaction.state,
            focused,
            labels,
            renderMermaid,
          );
        }
        const revealChanged =
          revealedMermaidBlocksKey(
            transaction.startState,
            previous.focused,
            previous.blocks,
          ) !== revealedMermaidBlocksKey(
            transaction.state,
            focused,
            previous.blocks,
          );
        return {
          ...previous,
          decorations: revealChanged
            ? mermaidPreviewDecorationsFromBlocks(
                transaction.state,
                focused,
                previous.blocks,
                labels,
                renderMermaid,
              )
            : previous.decorations,
          focused,
        };
      }

      if (
        treeChanged ||
        changesTouchMermaidBlocks(transaction, previous.blocks) ||
        changesMayAffectMermaidFences(transaction)
      ) {
        return createMermaidPreviewState(
          transaction.state,
          focused,
          labels,
          renderMermaid,
        );
      }

      const blocks = mapMermaidPreviewBlocks(previous.blocks, transaction);
      const revealChanged = revealedMermaidBlocksKey(
        transaction.startState,
        previous.focused,
        previous.blocks,
      ) !== revealedMermaidBlocksKey(
        transaction.state,
        focused,
        blocks,
      );
      return {
        blocks,
        decorations: revealChanged
          ? mermaidPreviewDecorationsFromBlocks(
              transaction.state,
              focused,
              blocks,
              labels,
              renderMermaid,
            )
          : previous.decorations.map(transaction.changes),
        focused,
      };
    },
    provide: (mermaidField) => Prec.highest(
      EditorView.decorations.from(
        mermaidField,
        (value) => value.decorations,
      ),
    ),
  });
  const mountedViews = new WeakSet<CodeMirrorView>();
  const lifecycle = ViewPlugin.define((view) => {
    mountedViews.add(view);
    return {
      destroy() {
        mountedViews.delete(view);
      },
    };
  });

  const syncFocusedState = (view: CodeMirrorView) => {
    // Focus events can fire inside toolbar and widget handlers that already
    // dispatch a transaction. Defer this independent UI state update so it
    // cannot interrupt the originating command or its React subscribers.
    queueMicrotask(() => {
      if (!mountedViews.has(view)) return;
      const focused = view.hasFocus;
      const previewState = view.state.field(field, false);
      if (!previewState || previewState.focused === focused) return;
      view.dispatch({ effects: setMermaidPreviewFocusedEffect.of(focused) });
    });
  };

  return [
    field,
    lifecycle,
    EditorView.domEventHandlers({
      blur(_event, view) {
        syncFocusedState(view);
      },
      focus(_event, view) {
        syncFocusedState(view);
      },
    }),
  ];
}

function normalizeHighlights(
  spans: readonly CodeBlockHighlightSpan[],
  codeLength: number,
) {
  return spans.flatMap((span) => {
    const className = span.className.trim();
    if (
      !className ||
      !Number.isInteger(span.from) ||
      !Number.isInteger(span.to) ||
      span.from < 0 ||
      span.from >= span.to ||
      span.to > codeLength
    ) {
      return [];
    }
    return [{ ...span, className }];
  });
}

function lineIntersects(
  line: Readonly<{ from: number; to: number }>,
  range: Readonly<{ from: number; to: number }>,
) {
  return line.from < range.to && line.to >= range.from;
}

function fencedCodeAtPosition(state: EditorState, position: number) {
  let node: ReturnType<typeof syntaxTree>["topNode"] | null =
    syntaxTree(state).resolveInner(position, -1);
  while (node) {
    if (node.name === "FencedCode") return node as MarkraSyntaxNode;
    node = node.parent;
  }
  return null;
}

export function fencedCodeTextAt(state: EditorState, from: number) {
  const node = fencedCodeAtPosition(state, Math.min(state.doc.length, from + 1));
  if (node?.from !== from) return;
  return node.getChildren("CodeText").map(part => state.sliceDoc(part.from, part.to)).join("");
}

function fencedCodeAt(view: CodeMirrorView) {
  return fencedCodeAtPosition(
    view.state,
    view.state.selection.main.head,
  );
}

function selectCurrentCodeBlockContent(view: CodeMirrorView) {
  const node = fencedCodeAt(view);
  const code = node?.getChild("CodeText");
  if (!code) return false;
  const selection = view.state.selection.main;
  if (selection.from === code.from && selection.to === code.to) return false;

  view.dispatch({
    scrollIntoView: true,
    selection: EditorSelection.range(code.from, code.to),
  });
  return true;
}

function unwrapCodeBlockBackward(view: CodeMirrorView) {
  if (
    view.state.readOnly ||
    view.state.selection.ranges.some((selection) => !selection.empty)
  ) {
    return false;
  }

  const unwrapped = view.state.selection.ranges.map((selection) => {
    const node = fencedCodeAtPosition(view.state, selection.head);
    if (!node) return null;
    const parts = codeBlockParts(view.state, node);
    if (!parts.codeNode || selection.head !== parts.codeNode.from) return null;

    const openingLine = view.state.doc.lineAt(node.from);
    const firstCodeLine = view.state.doc.lineAt(parts.codeNode.from);
    const changes = [{
      from: openingLine.from,
      to: firstCodeLine.from,
    }];
    if (parts.hasClosingFence) {
      const closingLine = view.state.doc.lineAt(node.to);
      changes.push({
        // Delete the separator before the closing fence so unwrapping keeps
        // exactly the paragraph break that originally followed the block.
        from: closingLine.from - 1,
        to: closingLine.to,
      });
    }
    return { changes, cursor: openingLine.from };
  });
  if (unwrapped.some((candidate) => candidate === null)) return false;

  const codeBlocks = unwrapped.filter((candidate) => candidate !== null);
  const changeSet = view.state.changes(
    codeBlocks.flatMap((codeBlock) => codeBlock.changes),
  );

  view.dispatch({
    changes: changeSet,
    scrollIntoView: true,
    selection: EditorSelection.create(
      codeBlocks.map((codeBlock) =>
        EditorSelection.cursor(changeSet.mapPos(codeBlock.cursor, 1))
      ),
      view.state.selection.mainIndex,
    ),
    userEvent: "delete.backward",
  });
  view.focus();
  return true;
}

function handleCodeBlockEnter(view: CodeMirrorView) {
  const selection = view.state.selection.main;
  if (!selection.empty || view.state.readOnly || view.compositionStarted) return false;
  const node = fencedCodeAt(view);
  if (!node) return false;
  const parts = codeBlockParts(view.state, node);
  const cursorLine = view.state.doc.lineAt(selection.head);
  const openingMark = parts.hasClosingFence
    ? undefined
    : node.getChildren("CodeMark")[0];
  const openingLine = openingMark
    ? view.state.doc.lineAt(openingMark.from)
    : undefined;
  const closingFence = openingMark && openingLine
    ? `${view.state.sliceDoc(
        openingLine.from,
        openingMark.from,
      )}${view.state.sliceDoc(openingMark.from, openingMark.to)}`
    : undefined;

  if (
    openingLine &&
    closingFence &&
    cursorLine.number === openingLine.number &&
    selection.head === cursorLine.to
  ) {
    // Pair on Enter so an info string such as ```sh can still be typed before
    // the editor creates the content line and matching closing fence.
    view.dispatch({
      changes: {
        from: cursorLine.to,
        insert: `\n\n${closingFence}`,
      },
      scrollIntoView: true,
      selection: EditorSelection.cursor(cursorLine.to + 1),
    });
    view.focus();
    return true;
  }

  if (cursorLine.text.trim()) return false;

  if (!parts.hasClosingFence) {
    if (
      cursorLine.number !== view.state.doc.lines ||
      selection.head !== cursorLine.to ||
      !closingFence
    ) {
      return false;
    }

    view.dispatch({
      changes: {
        from: cursorLine.from,
        insert: `${closingFence}\n`,
        to: cursorLine.to,
      },
      scrollIntoView: true,
      selection: EditorSelection.cursor(
        cursorLine.from + closingFence.length + 1,
      ),
    });
    view.focus();
    return true;
  }

  const closingLine = view.state.doc.lineAt(node.to);
  if (cursorLine.number + 1 !== closingLine.number) return false;

  moveSelectionAfterCodeBlock(view, node.to, { from: cursorLine.from, to: closingLine.from });
  return true;
}

function exitMermaidSource(view: CodeMirrorView) {
  const node = fencedCodeAt(view);
  if (!node) return false;
  const parts = codeBlockParts(view.state, node);
  if (!isPreviewDiagram(parts.language)) return false;

  view.dispatch({
    selection: EditorSelection.cursor(
      Math.min(view.state.doc.length, node.to + 1),
    ),
  });
  return true;
}

const codeBlockKeymap = Prec.high(
  keymap.of([
    { key: "Backspace", run: unwrapCodeBlockBackward },
    { key: "Enter", run: handleCodeBlockEnter },
    { key: "Mod-a", run: selectCurrentCodeBlockContent },
    { key: "Escape", run: exitMermaidSource },
  ]),
);

const codeBlockPointerHandlers = EditorView.domEventHandlers({
  mousedown(event, view) {
    if (event.button !== 0 || !(event.target instanceof Element)) return false;
    const closingLine = event.target.closest<HTMLElement>(
      ".cm-markra-code-closing-line",
    );
    const rawAfterFence = closingLine?.dataset.codeBlockEnd;
    if (!closingLine || rawAfterFence === undefined) return false;
    const afterFence = Number(rawAfterFence);
    if (!Number.isInteger(afterFence)) return false;

    event.preventDefault();
    event.stopPropagation();
    moveSelectionAfterCodeBlock(view, afterFence);
    return true;
  },
});

export function codeBlockPreviewPlugin(
  options: CodeBlockPreviewPluginOptions = {},
) {
  const labels = { ...defaultLabels, ...options.labels };
  const showLineNumbers = options.showLineNumbers ?? true;
  const highlight = options.highlight ?? ((context: CodeBlockHighlightContext) =>
    highlightMarkraCode(context.language, context.code));
  const renderMermaid = options.renderMermaid ?? ((context: CodeBlockMermaidContext) =>
    renderMermaidToSvg(context.source, {
      idPrefix: "markra-codemirror-mermaid",
      theme: context.theme,
    }));
  const highlightCache: Array<{
    code: string;
    highlighted: readonly CodeBlockHighlightSpan[];
    language: string;
  }> = [];
  let cachedCodeCharacters = 0;
  const maxCachedCodeBlocks = 16;
  const maxCachedCodeCharacters = 1_000_000;

  const highlightsFor = (
    context: MarkraRendererContext,
    parts: CodeBlockParts,
  ) => {
    if (!parts.codeNode) return [];
    const cachedIndex = highlightCache.findIndex(
      (entry) =>
        entry.language === parts.language && entry.code === parts.code,
    );
    const cached = highlightCache[cachedIndex];
    if (cached) {
      highlightCache.splice(cachedIndex, 1);
      highlightCache.push(cached);
      return cached.highlighted;
    }

    let highlighted: readonly CodeBlockHighlightSpan[] = [];
    try {
      highlighted = normalizeHighlights(
        highlight({
          code: parts.code,
          language: parts.language,
          state: context.state,
          view: context.view,
        }),
        parts.code.length,
      );
    } catch {
      highlighted = [];
    }

    // Edits outside a fenced block create a new EditorState while leaving its
    // source unchanged. Cache by the actual highlighter inputs so that common
    // typing does not synchronously re-highlight untouched blocks.
    highlightCache.push({
      code: parts.code,
      highlighted,
      language: parts.language,
    });
    cachedCodeCharacters += parts.code.length;
    while (
      highlightCache.length > maxCachedCodeBlocks ||
      cachedCodeCharacters > maxCachedCodeCharacters
    ) {
      const removed = highlightCache.shift();
      cachedCodeCharacters -= removed?.code.length ?? 0;
    }
    return highlighted;
  };

  return defineMarkraPlugin({
    id: "markra.code-block-preview",
    extension: [
      createMermaidPreviewField(labels, renderMermaid),
      markraRenderer({
        id: "markra.code-block-preview",
        nodeNames: ["FencedCode"],
        scope: "visible-range",
        render(context) {
          const { node, state, visibleRange } = context;
          const parts = codeBlockParts(state, node);
          const firstLine = state.doc.lineAt(node.from);
          const lastLine = state.doc.lineAt(node.to);
          if (
            !parts.hasClosingFence &&
            firstLine.number === lastLine.number
          ) {
            // Keep a newly typed fence visible until Enter pairs it. Folding
            // its only line would leave a zero-height block with no caret.
            return false;
          }
          const revealed = context.revealed("line");
          // A Mermaid source selection must not collapse as soon as dragging
          // makes it non-empty. Anchor-only matching preserves drags that
          // start inside the block without revealing source for selections
          // that merely pass over the preview from outside.
          const selectionAnchoredInside =
            context.view.hasFocus &&
            state.selection.ranges.some(
              (selection) =>
                !selection.empty &&
                selection.anchor >= node.from &&
                selection.anchor <= node.to,
            );
          const diagram = isPreviewDiagram(parts.language);
          const sourceRevealed = !state.readOnly && (diagram
            ? mermaidSourceRevealed(state, node.from, node.to, context.view.hasFocus)
            : revealed || selectionAnchoredInside);
          if (
            !sourceRevealed &&
            parts.codeNode &&
            parts.code.trim() &&
            diagram
          ) {
            return false;
          }
          const visibleFrom = Math.max(node.from, visibleRange.from);
          const visibleTo = Math.min(node.to, visibleRange.to);
          if (visibleFrom >= visibleTo) return false;

          const firstVisibleLine = state.doc.lineAt(visibleFrom).number;
          const lastVisibleLine = state.doc.lineAt(visibleTo - 1).number;
          let codeLineNumber = 0;
          for (
            let lineNumber = firstVisibleLine;
            lineNumber <= lastVisibleLine;
            lineNumber += 1
          ) {
            const line = state.doc.line(lineNumber);
            const opening = line.number === firstLine.number;
            const closing = parts.hasClosingFence && line.number === lastLine.number;
            const codeContentLine = !opening && !closing;
            const roleClass = opening ? "cm-markra-code-header-line"
              : closing ? "cm-markra-code-closing-line" : "cm-markra-code-content-line";
            const openEdge = !parts.hasClosingFence && line.number === lastLine.number;
            if (codeContentLine) codeLineNumber += 1;
            context.add(
              Decoration.line({
                attributes: {
                  "data-code-block-from": String(node.from),
                  ...(codeContentLine && showLineNumbers ? { "data-code-line-number": String(codeLineNumber) } : {}),
                  ...(opening && !sourceRevealed ? { "data-code-block-start": String(node.from) } : {}),
                  ...(closing && !sourceRevealed ? { "data-code-block-end": String(node.to) } : {}),
                },
                class: `cm-markra-code-line ${roleClass}${openEdge ? " cm-markra-code-open-edge" : ""}`,
              }).range(line.from),
            );
          }

          if (!sourceRevealed && lineIntersects(firstLine, visibleRange)) {
            context.add(Decoration.replace({}).range(node.from, firstLine.to));
          }
          if (
            !sourceRevealed &&
            parts.hasClosingFence &&
            lineIntersects(lastLine, visibleRange)
          ) {
            const closingMark = node.getChildren("CodeMark").at(-1)!;
            context.add(
              Decoration.replace({
                widget: new CodeBlockExitWidget(node.to),
              }).range(closingMark.from, node.to),
            );
          }

          if (parts.codeNode) {
            if (!sourceRevealed && parts.languageLabel && parts.codeNode.from >= visibleFrom && parts.codeNode.from < visibleTo) {
              context.add(Decoration.widget({ widget: new CodeLanguageWidget(parts.languageLabel), side: 1 }).range(parts.codeNode.from));
            }
            const contentFrom = Math.max(
              parts.codeNode.from,
              visibleRange.from,
            );
            const contentTo = Math.min(parts.codeNode.to, visibleRange.to);
            if (contentFrom < contentTo) {
              context.add(
                Decoration.mark({ class: "cm-markra-code-content" }).range(
                  contentFrom,
                  contentTo,
                ),
              );
            }

            for (const span of highlightsFor(context, parts)) {
              const from = Math.max(
                parts.codeNode.from + span.from,
                visibleRange.from,
              );
              const to = Math.min(
                parts.codeNode.from + span.to,
                visibleRange.to,
              );
              if (from >= to) continue;
              context.add(
                Decoration.mark({
                  class: `cm-markra-code-token ${span.className}`,
                }).range(from, to),
              );
            }
          }

          return false;
        },
      }),
      codeBlockKeymap,
      codeBlockPointerHandlers,
      codeBlockTheme,
    ],
  });
}
