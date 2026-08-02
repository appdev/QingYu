import {
  language,
  syntaxTree,
  syntaxTreeAvailable,
} from "@codemirror/language";
import {
  EditorSelection,
  EditorState,
  type EditorState as CodeMirrorState,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type EditorView as CodeMirrorView,
  type ViewUpdate,
} from "@codemirror/view";
import { readMarkdownFrontmatter } from "@markra/markdown";
import { defineMarkraPlugin } from "./plugin.ts";
import { openMarkraSlashMenu } from "./slash-menu.ts";

export interface CodeMirrorBlockRange {
  readonly depth?: number;
  readonly from: number;
  readonly name: string;
  readonly to: number;
}

export interface CodeMirrorBlockDragLabels {
  readonly addBlock: string;
  readonly dragBlock: string;
}

export interface CodeMirrorBlockDragPluginOptions {
  labels?: Partial<CodeMirrorBlockDragLabels>;
}

export type CodeMirrorBlockDropSide = "after" | "before";

interface CodeMirrorBlockReadRange {
  readonly from: number;
  readonly to: number;
}

const blockDragMime = "application/x-markra-codemirror-block";
const pointerDragThreshold = 4;
const defaultLabels: CodeMirrorBlockDragLabels = {
  addBlock: "Add block below",
  dragBlock: "Drag block",
};
const completeSyntaxTrees = new WeakMap<
  CodeMirrorState,
  ReturnType<typeof syntaxTree>
>();

function completeSyntaxTree(state: CodeMirrorState) {
  const cached = completeSyntaxTrees.get(state);
  if (cached) return cached;

  const published = syntaxTree(state);
  if (
    published.length >= state.doc.length &&
    syntaxTreeAvailable(state)
  ) {
    completeSyntaxTrees.set(state, published);
    return published;
  }

  const parser = state.facet(language)?.parser;
  const tree = parser
    ? parser.parse(state.doc.toString())
    : published;
  completeSyntaxTrees.set(state, tree);
  return tree;
}

function readCodeMirrorBlockRangesIn(
  state: CodeMirrorState,
  readRanges: readonly CodeMirrorBlockReadRange[],
  tree: ReturnType<typeof syntaxTree>,
): CodeMirrorBlockRange[] {
  const ranges: CodeMirrorBlockRange[] = [];
  const decoratedStarts = new Set<number>();
  const includesStart = (from: number) => readRanges.some(
    (range) => from >= range.from && from <= range.to,
  );
  const appendRange = (range: CodeMirrorBlockRange) => {
    if (decoratedStarts.has(range.from) || !includesStart(range.from)) return;
    decoratedStarts.add(range.from);
    ranges.push(range);
  };
  const frontmatterResult = readMarkdownFrontmatter(state.doc.toString());
  const frontmatter = frontmatterResult.status === "valid"
    ? frontmatterResult.range
    : null;
  if (frontmatter) {
    appendRange({
      from: state.doc.lineAt(frontmatter.from).from,
      name: `Frontmatter:${frontmatter.kind}`,
      to: state.doc.lineAt(frontmatter.to).to,
    });
  }

  for (const readRange of readRanges) {
    tree.iterate({
      from: readRange.from,
      to: readRange.to,
      enter(node) {
        if (frontmatter && node.from < frontmatter.to) return;
        if (node.name === "ListItem") {
          let depth = 0;
          let item = node.node;
          while (true) {
            const list = item.parent;
            if (
              list?.name !== "BulletList" &&
              list?.name !== "OrderedList"
            ) {
              return;
            }
            const owner = list.parent;
            if (!owner || owner.parent === null) break;
            if (owner.name !== "ListItem") return;
            depth += 1;
            item = owner;
          }
          const line = state.doc.lineAt(node.from);
          appendRange({
            depth,
            from: line.from,
            name: node.name,
            to: state.doc.lineAt(node.to).to,
          });
          return;
        }

        const parent = node.node.parent;
        if (
          parent?.parent === null &&
          node.name !== "BulletList" &&
          node.name !== "OrderedList"
        ) {
          const from = state.doc.lineAt(node.from).from;
          const to = state.doc.lineAt(node.to).to;
          if (to > from) appendRange({ from, name: node.name, to });
        }
      }
    });
  }

  const visitedEmptyLines = new Set<number>();
  for (const readRange of readRanges) {
    const firstLine = state.doc.lineAt(readRange.from).number;
    const lastLine = state.doc.lineAt(readRange.to).number;
    for (
      let lineNumber = firstLine;
      lineNumber <= lastLine;
      lineNumber += 1
    ) {
      if (
        lineNumber <= 1 ||
        lineNumber >= state.doc.lines ||
        visitedEmptyLines.has(lineNumber)
      ) {
        continue;
      }
      visitedEmptyLines.add(lineNumber);
      const line = state.doc.line(lineNumber);
      if (
        line.length === 0 &&
        state.doc.line(lineNumber - 1).length === 0 &&
        state.doc.line(lineNumber + 1).length === 0
      ) {
        appendRange({ from: line.from, name: "EmptyLine", to: line.to });
      }
    }
  }

  return ranges.sort((left, right) => left.from - right.from || left.to - right.to);
}

export function readCodeMirrorBlockRanges(
  state: CodeMirrorState,
): CodeMirrorBlockRange[] {
  return readCodeMirrorBlockRangesIn(
    state,
    [{ from: 0, to: state.doc.length }],
    completeSyntaxTree(state),
  );
}

function blockByFrom(state: CodeMirrorState, from: number) {
  return readCodeMirrorBlockRanges(state).find((range) => range.from === from) ?? null;
}

function minimalDocumentChange(current: string, next: string) {
  let from = 0;
  while (
    from < current.length &&
    from < next.length &&
    current[from] === next[from]
  ) {
    from += 1;
  }

  let currentTo = current.length;
  let nextTo = next.length;
  while (
    currentTo > from &&
    nextTo > from &&
    current[currentTo - 1] === next[nextTo - 1]
  ) {
    currentTo -= 1;
    nextTo -= 1;
  }

  return {
    from,
    insert: next.slice(from, nextTo),
    to: currentTo,
  };
}

function listMarkerMatch(state: CodeMirrorState, block: CodeMirrorBlockRange) {
  return /^(\s*)([-+*]|\d+[.)])(\s+)/u.exec(
    state.doc.lineAt(block.from).text,
  );
}

function markdownColumnWidth(value: string) {
  let column = 0;
  for (const character of value) {
    column += character === "\t" ? 4 - column % 4 : 1;
  }
  return column;
}

function findLastListBlockAtDepth(
  blocks: readonly CodeMirrorBlockRange[],
  depth: number,
) {
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (block?.name === "ListItem" && block.depth === depth) return block;
  }
  return undefined;
}

function normalizedListDrop(
  state: CodeMirrorState,
  blocks: readonly CodeMirrorBlockRange[],
  source: CodeMirrorBlockRange,
  target: CodeMirrorBlockRange,
  side: CodeMirrorBlockDropSide,
  requestedDepth: number,
) {
  const stationary = blocks.filter(
    (block) => block.from < source.from || block.from >= source.to,
  );
  const targetIndex = stationary.findIndex((block) => block.from === target.from);
  const previous = side === "after"
    ? target
    : stationary[targetIndex - 1] ?? null;
  // Horizontal pointer movement can request a level whose parent does not
  // exist. Clamp it here so the moved `-` remains a parsed list marker.
  const maximumDepth = previous?.name === "ListItem"
    ? (previous.depth ?? 0) + 1
    : target.name === "ListItem"
      ? target.depth ?? 0
      : 0;
  const depth = Math.min(Math.max(0, requestedDepth), maximumDepth);

  const sameLevel = [target, previous].find(
    (block) => block?.name === "ListItem" && block.depth === depth,
  );
  if (sameLevel) {
    return {
      depth,
      indentation: listMarkerMatch(state, sameLevel)?.[1] ?? "",
    };
  }

  const contextEnd = side === "after" ? targetIndex : targetIndex - 1;
  const preceding = stationary.slice(0, contextEnd + 1);
  const parent = findLastListBlockAtDepth(preceding, depth - 1);
  const parentMarker = parent ? listMarkerMatch(state, parent) : null;
  if (parentMarker) {
    return {
      depth,
      // Ordered markers need wider child indentation than `- `, so align to
      // the parent's actual content column instead of assuming two spaces.
      // Markdown expands tabs to four-column stops, so string length would
      // under-indent pasted tab-indented lists and break their parsed depth.
      indentation: " ".repeat(markdownColumnWidth(parentMarker[0])),
    };
  }

  const reference = findLastListBlockAtDepth(preceding, depth);
  return {
    depth,
    indentation: reference
      ? listMarkerMatch(state, reference)?.[1] ?? "  ".repeat(depth)
      : "  ".repeat(depth),
  };
}

export function moveCodeMirrorBlock(
  view: CodeMirrorView,
  sourceFrom: number,
  targetFrom: number,
  side: CodeMirrorBlockDropSide,
  targetDepth?: number,
) {
  if (view.state.facet(EditorState.readOnly)) return false;
  const blocks = readCodeMirrorBlockRanges(view.state);
  const source = blocks.find((block) => block.from === sourceFrom);
  const target = blocks.find((block) => block.from === targetFrom);
  if (!source || !target) return false;
  if (
    source.from === target.from ||
    (target.from > source.from && target.from < source.to)
  ) {
    return false;
  }

  const document = view.state.doc.toString();
  const movingIntoList = source.name !== "ListItem" && target.name === "ListItem";
  const drop = targetDepth !== undefined &&
      (source.name === "ListItem" || movingIntoList)
    ? normalizedListDrop(
        view.state,
        blocks,
        source,
        target,
        side,
        targetDepth,
      )
    : null;
  let sourceMarkdown = document.slice(source.from, source.to);
  if (source.name === "ListItem" && drop) {
    const sourceIndentation = /^[\t ]*/u.exec(sourceMarkdown)?.[0] ?? "";
    const indentationDelta = markdownColumnWidth(drop.indentation) -
      markdownColumnWidth(sourceIndentation);
    if (indentationDelta !== 0) {
      sourceMarkdown = sourceMarkdown
        .split("\n")
        .map((line) => {
          const indentation = /^[\t ]*/u.exec(line)?.[0] ?? "";
          const nextIndentation = Math.max(
            0,
            markdownColumnWidth(indentation) + indentationDelta,
          );
          return `${" ".repeat(nextIndentation)}${line.slice(indentation.length)}`;
        })
        .join("\n");
    }
  }
  if (movingIntoList) {
    const targetLine = view.state.doc.lineAt(target.from).text;
    const marker = /^(\s*)([-+*]|\d+[.)])\s+/u.exec(targetLine);
    if (marker) {
      const indentation = drop?.indentation ?? marker[1] ?? "";
      const sourceMarker = marker[2] ?? "-";
      const prefix = `${indentation}${sourceMarker} `;
      const continuation = `${indentation}${" ".repeat(sourceMarker.length + 1)}`;
      sourceMarkdown = sourceMarkdown
        .split("\n")
        .map((line, index) => index === 0 ? `${prefix}${line}` : `${continuation}${line}`)
        .join("\n");
    }
  }

  let deletionFrom = source.from;
  let deletionTo = source.to;
  if (deletionTo < document.length) {
    while (document[deletionTo] === "\n") deletionTo += 1;
  } else {
    while (deletionFrom > 0 && document[deletionFrom - 1] === "\n") {
      deletionFrom -= 1;
    }
  }
  const targetPosition = side === "before" ? target.from : target.to;
  if (targetPosition > deletionFrom && targetPosition < deletionTo) return false;

  const withoutSource = document.slice(0, deletionFrom) + document.slice(deletionTo);
  const mappedTarget = targetPosition <= deletionFrom
    ? targetPosition
    : targetPosition - (deletionTo - deletionFrom);
  const tight = (source.name === "ListItem" || movingIntoList) &&
    target.name === "ListItem";
  const requiredBreaks = tight ? 1 : 2;
  let leftBreaks = 0;
  for (let index = mappedTarget - 1; index >= 0 && withoutSource[index] === "\n"; index -= 1) {
    leftBreaks += 1;
  }
  let rightBreaks = 0;
  for (
    let index = mappedTarget;
    index < withoutSource.length && withoutSource[index] === "\n";
    index += 1
  ) {
    rightBreaks += 1;
  }
  const prefix = mappedTarget > 0
    ? "\n".repeat(Math.max(0, requiredBreaks - leftBreaks))
    : "";
  const suffix = mappedTarget < withoutSource.length
    ? "\n".repeat(Math.max(0, requiredBreaks - rightBreaks))
    : "";
  const inserted = `${prefix}${sourceMarkdown}${suffix}`;
  const nextDocument = withoutSource.slice(0, mappedTarget) +
    inserted +
    withoutSource.slice(mappedTarget);
  if (nextDocument === document) return false;
  const insertedFrom = mappedTarget + prefix.length;
  // A single transaction keeps whitespace normalization and the move in one
  // undo step, including when a paragraph becomes a list item.
  view.dispatch({
    // Keep the unchanged prefix and suffix so CodeMirror can incrementally
    // preserve the parsed list tree in long documents after a nested move.
    changes: minimalDocumentChange(document, nextDocument),
    scrollIntoView: true,
    selection: EditorSelection.cursor(insertedFrom),
    userEvent: "move",
  });
  view.focus();
  return true;
}

export function addCodeMirrorBlockBelow(
  view: CodeMirrorView,
  blockFrom: number,
) {
  if (view.state.facet(EditorState.readOnly)) return false;
  const block = blockByFrom(view.state, blockFrom);
  if (!block) return false;
  view.dispatch({
    changes: { from: block.to, insert: "\n\n" },
    selection: EditorSelection.cursor(block.to + 1),
    scrollIntoView: true,
    userEvent: "input",
  });
  openMarkraSlashMenu(view);
  return true;
}

function blockControl(
  document: Document,
  label: string,
  className: string,
) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  button.ariaLabel = label;
  button.title = label;
  button.addEventListener("mousedown", (event) => {
    event.stopPropagation();
  });
  return button;
}

class BlockToolbarWidget extends WidgetType {
  constructor(
    readonly blockFrom: number,
    readonly labels: CodeMirrorBlockDragLabels,
  ) {
    super();
  }

  eq(other: BlockToolbarWidget) {
    return this.blockFrom === other.blockFrom &&
      JSON.stringify(this.labels) === JSON.stringify(other.labels);
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const toolbar = document.createElement("span");
    const add = blockControl(
      document,
      this.labels.addBlock,
      "markra-block-tool-button markra-block-add-button",
    );
    const drag = blockControl(
      document,
      this.labels.dragBlock,
      "markra-block-tool-button markra-block-drag-handle",
    );
    toolbar.className = "cm-markra-block-toolbar markra-block-toolbar";
    toolbar.dataset.blockFrom = String(this.blockFrom);
    for (let index = 0; index < 6; index += 1) {
      const dot = document.createElement("span");
      dot.className = "markra-block-drag-dot";
      drag.append(dot);
    }
    drag.draggable = true;
    add.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      addCodeMirrorBlockBelow(view, this.blockFrom);
    });
    drag.addEventListener("dragstart", (event) => {
      event.dataTransfer?.setData(blockDragMime, String(this.blockFrom));
      if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
      drag.dataset.dragging = "true";
      startBlockDragUi(view, this.blockFrom, event);
    });
    drag.addEventListener("dragend", () => {
      delete drag.dataset.dragging;
      clearBlockDragUi(view);
    });
    // Button drags do not reliably emit the HTML5 drag lifecycle in every
    // WebView, so pointer events provide the primary cross-platform path.
    drag.addEventListener("pointerdown", (event) => {
      startPointerBlockDrag(view, this.blockFrom, drag, event);
    });
    toolbar.append(add, drag);
    return toolbar;
  }
}

function blockDecorations(
  view: CodeMirrorView,
  labels: CodeMirrorBlockDragLabels,
): DecorationSet {
  const { state } = view;
  if (state.facet(EditorState.readOnly)) return Decoration.none;
  const decorations = readCodeMirrorBlockRangesIn(
    state,
    view.visibleRanges,
    syntaxTree(state),
  ).flatMap((block) => [
    Decoration.line({
      attributes: { "data-markra-block-from": String(block.from) },
    }).range(block.from),
    Decoration.widget({
      // Block tools must be the outermost start-of-line widget. Heading-level
      // controls also use side -1, and their negative gutter margin would
      // otherwise pull the drag handle back over the H1-H6 button.
      side: -2,
      widget: new BlockToolbarWidget(block.from, labels),
    }).range(block.from),
  ]);
  return Decoration.set(decorations, true);
}

function eventElement(event: Event) {
  return event.target instanceof Element
    ? event.target
    : event.target instanceof Node
      ? event.target.parentElement
      : null;
}

function dropTarget(event: MouseEvent, view: CodeMirrorView) {
  const element = eventElement(event)?.closest<HTMLElement>(
    "[data-markra-block-from], [data-block-from]",
  );
  const from = Number(
    element?.dataset.markraBlockFrom ?? element?.dataset.blockFrom,
  );
  if (Number.isInteger(from)) {
    const rect = element?.getBoundingClientRect();
    const side = rect && event.clientY < rect.top + rect.height / 2
      ? "before"
      : "after";
    const currentDepth = Number(element?.dataset.listDepth);
    const pointerDepth = rect && Number.isInteger(currentDepth)
      ? Math.max(0, Math.round((event.clientX - rect.left - 22) / 22))
      : undefined;
    return {
      depth: pointerDepth,
      element: element ?? null,
      from,
      side,
    } as const;
  }
  try {
    const position = view.posAtCoords({ x: event.clientX, y: event.clientY });
    if (position === null) return null;
    const block = readCodeMirrorBlockRangesIn(
      view.state,
      view.visibleRanges,
      syntaxTree(view.state),
    ).find(
      (candidate) => position >= candidate.from && position <= candidate.to,
    );
    return block
      ? { depth: block.depth, element: null, from: block.from, side: "after" as const }
      : null;
  } catch {
    return null;
  }
}

interface BlockDragUi {
  readonly ghost: HTMLElement;
  readonly indicator: HTMLElement;
  readonly source: HTMLElement | null;
}

const blockDragUi = new WeakMap<CodeMirrorView, BlockDragUi>();

function clearBlockDragUi(view: CodeMirrorView) {
  const ui = blockDragUi.get(view);
  if (!ui) return;
  ui.source?.classList.remove("markra-block-drag-source");
  ui.indicator.remove();
  ui.ghost.remove();
  view.dom.removeAttribute("data-dragging");
  view.dom.ownerDocument.documentElement.removeAttribute(
    "data-markra-block-dragging",
  );
  blockDragUi.delete(view);
}

function startBlockDragUi(
  view: CodeMirrorView,
  sourceFrom: number,
  event: MouseEvent,
) {
  clearBlockDragUi(view);
  const source = view.dom.querySelector<HTMLElement>(
    `.cm-line[data-markra-block-from="${sourceFrom}"]`,
  );
  const indicator = view.dom.ownerDocument.createElement("span");
  const ghost = view.dom.ownerDocument.createElement("span");
  indicator.className = "markra-block-drop-indicator";
  indicator.dataset.show = "false";
  ghost.className = "markra-block-drag-ghost";
  ghost.dataset.show = "true";
  ghost.textContent = source?.textContent?.trim() || "Markdown block";
  ghost.style.left = `${event.clientX + 12}px`;
  ghost.style.top = `${event.clientY + 12}px`;
  ghost.style.transform = "translate(0, 0)";
  view.dom.append(indicator, ghost);
  source?.classList.add("markra-block-drag-source");
  view.dom.dataset.dragging = "true";
  view.dom.ownerDocument.documentElement.dataset.markraBlockDragging = "true";
  if ("dataTransfer" in event) {
    (event as DragEvent).dataTransfer?.setDragImage?.(ghost, 12, 12);
  }
  blockDragUi.set(view, { ghost, indicator, source });
}

function updateBlockDragUi(
  view: CodeMirrorView,
  target: NonNullable<ReturnType<typeof dropTarget>>,
  event: MouseEvent,
) {
  const ui = blockDragUi.get(view);
  if (!ui || !target.element) return;
  const rect = target.element.getBoundingClientRect();
  ui.indicator.style.left = `${rect.left}px`;
  ui.indicator.style.top = `${target.side === "before" ? rect.top : rect.bottom}px`;
  ui.indicator.style.width = `${rect.width}px`;
  ui.indicator.dataset.show = "true";
  ui.ghost.style.left = `${event.clientX + 12}px`;
  ui.ghost.style.top = `${event.clientY + 12}px`;

  const scroll = view.dom.closest<HTMLElement>(".paper-scroll");
  if (!scroll) return;
  const scrollRect = scroll.getBoundingClientRect();
  if (event.clientY < scrollRect.top + 48) scroll.scrollTop -= 18;
  if (event.clientY > scrollRect.bottom - 48) scroll.scrollTop += 18;
}

function startPointerBlockDrag(
  view: CodeMirrorView,
  sourceFrom: number,
  handle: HTMLElement,
  event: PointerEvent,
) {
  if (event.button !== 0 || view.state.facet(EditorState.readOnly)) return;
  event.preventDefault();
  event.stopPropagation();

  const document = view.dom.ownerDocument;
  const pointerId = event.pointerId;
  const originX = event.clientX;
  const originY = event.clientY;
  let dragging = false;

  const cleanup = () => {
    document.removeEventListener("pointermove", handlePointerMove, true);
    document.removeEventListener("pointerup", handlePointerUp, true);
    document.removeEventListener("pointercancel", handlePointerCancel, true);
    delete handle.dataset.dragging;
  };
  const handlePointerMove = (moveEvent: PointerEvent) => {
    if (moveEvent.pointerId !== pointerId) return;
    if (!dragging) {
      const distance = Math.hypot(
        moveEvent.clientX - originX,
        moveEvent.clientY - originY,
      );
      if (distance < pointerDragThreshold) return;
      dragging = true;
      handle.dataset.dragging = "true";
      startBlockDragUi(view, sourceFrom, moveEvent);
    }

    const target = dropTarget(moveEvent, view);
    if (target) updateBlockDragUi(view, target, moveEvent);
    moveEvent.preventDefault();
  };
  const handlePointerUp = (upEvent: PointerEvent) => {
    if (upEvent.pointerId !== pointerId) return;
    cleanup();
    if (!dragging) return;

    const target = dropTarget(upEvent, view);
    if (target) {
      moveCodeMirrorBlock(
        view,
        sourceFrom,
        target.from,
        target.side,
        target.depth,
      );
    }
    clearBlockDragUi(view);
    upEvent.preventDefault();
    upEvent.stopPropagation();
  };
  const handlePointerCancel = (cancelEvent: PointerEvent) => {
    if (cancelEvent.pointerId !== pointerId) return;
    cleanup();
    clearBlockDragUi(view);
  };

  document.addEventListener("pointermove", handlePointerMove, true);
  document.addEventListener("pointerup", handlePointerUp, true);
  document.addEventListener("pointercancel", handlePointerCancel, true);
}

function draggedBlockFrom(event: DragEvent) {
  const value = event.dataTransfer?.getData(blockDragMime) ?? "";
  if (value === "") return null;
  const from = Number(value);
  return Number.isInteger(from) ? from : null;
}

class BlockDragViewPlugin {
  decorations: DecorationSet;
  tree: ReturnType<typeof syntaxTree>;

  constructor(view: CodeMirrorView, readonly labels: CodeMirrorBlockDragLabels) {
    this.tree = syntaxTree(view.state);
    this.decorations = blockDecorations(view, labels);
  }

  update(update: ViewUpdate) {
    const tree = syntaxTree(update.state);
    if (
      update.docChanged ||
      update.startState.readOnly !== update.state.readOnly ||
      update.viewportChanged ||
      tree !== this.tree
    ) {
      this.tree = tree;
      this.decorations = blockDecorations(update.view, this.labels);
    }
  }
}

const blockDragTheme = EditorView.baseTheme({
  ".cm-markra-block-toolbar": {
    display: "inline-flex",
    gap: "0.15em",
    marginInlineStart: "-3.2em",
    marginInlineEnd: "0.45em",
    opacity: "0.15",
    verticalAlign: "middle",
  },
  ".cm-line:hover > .cm-markra-block-toolbar, .cm-markra-block-toolbar:focus-within": {
    opacity: "1",
  },
  ".cm-markra-block-toolbar > button": {
    background: "transparent",
    border: "0",
    color: "inherit",
    cursor: "pointer",
    padding: "0 0.15em",
  },
  ".cm-markra-block-toolbar > .markra-block-drag-handle": {
    cursor: "grab",
  },
});

export function codeMirrorBlockDragPlugin(
  options: CodeMirrorBlockDragPluginOptions = {},
) {
  const labels = { ...defaultLabels, ...options.labels };
  return defineMarkraPlugin({
    id: "markra.block-drag",
    extension: [
      ViewPlugin.define(
        (view) => new BlockDragViewPlugin(view, labels),
        { decorations: (plugin) => plugin.decorations },
      ),
      EditorView.domEventHandlers({
        dragover(event, view) {
          const target = dropTarget(event, view);
          if (draggedBlockFrom(event) === null || !target) {
            return false;
          }
          event.preventDefault();
          if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
          updateBlockDragUi(view, target, event);
          return true;
        },
        drop(event, view) {
          const sourceFrom = draggedBlockFrom(event);
          const target = dropTarget(event, view);
          if (sourceFrom === null || !target) return false;
          const handled = moveCodeMirrorBlock(
            view,
            sourceFrom,
            target.from,
            target.side,
            target.depth,
          );
          clearBlockDragUi(view);
          if (!handled) return false;
          event.preventDefault();
          return true;
        },
      }),
      blockDragTheme,
    ],
  });
}
