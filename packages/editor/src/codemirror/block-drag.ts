import { syntaxTree } from "@codemirror/language";
import {
  EditorSelection,
  EditorState,
  type ChangeSpec,
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
import { readCodeMirrorFrontmatter } from "./frontmatter-preview.ts";
import { defineMarkraPlugin } from "./plugin.ts";
import { openMarkraSlashMenu } from "./slash-menu.ts";

export interface CodeMirrorBlockRange {
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

const blockDragMime = "application/x-markra-codemirror-block";
const defaultLabels: CodeMirrorBlockDragLabels = {
  addBlock: "Add block below",
  dragBlock: "Drag block",
};

export function readCodeMirrorBlockRanges(
  state: CodeMirrorState,
): CodeMirrorBlockRange[] {
  const ranges: CodeMirrorBlockRange[] = [];
  const frontmatter = readCodeMirrorFrontmatter(state.doc.toString());
  if (frontmatter) {
    ranges.push({
      from: state.doc.lineAt(frontmatter.from).from,
      name: `Frontmatter:${frontmatter.kind}`,
      to: state.doc.lineAt(frontmatter.to).to,
    });
  }

  let node = syntaxTree(state).topNode.firstChild;
  while (node) {
    const next = node.nextSibling;
    if (!frontmatter || node.from >= frontmatter.to) {
      const from = state.doc.lineAt(node.from).from;
      const to = state.doc.lineAt(node.to).to;
      const previous = ranges.at(-1);
      if (to > from && (!previous || from >= previous.to)) {
        ranges.push({ from, name: node.name, to });
      }
    }
    node = next;
  }
  return ranges;
}

function blockByFrom(state: CodeMirrorState, from: number) {
  return readCodeMirrorBlockRanges(state).find((range) => range.from === from) ?? null;
}

export function moveCodeMirrorBlock(
  view: CodeMirrorView,
  sourceFrom: number,
  targetFrom: number,
  side: CodeMirrorBlockDropSide,
) {
  if (view.state.facet(EditorState.readOnly)) return false;
  const blocks = readCodeMirrorBlockRanges(view.state);
  const sourceIndex = blocks.findIndex((block) => block.from === sourceFrom);
  const targetIndex = blocks.findIndex((block) => block.from === targetFrom);
  if (sourceIndex < 0 || targetIndex < 0 || sourceIndex === targetIndex) return false;
  if (side === "before" && sourceIndex + 1 === targetIndex) return false;
  if (side === "after" && sourceIndex - 1 === targetIndex) return false;

  const source = blocks[sourceIndex];
  const target = blocks[targetIndex];
  if (!source || !target) return false;
  const sourceMarkdown = view.state.sliceDoc(source.from, source.to);
  const previous = blocks[sourceIndex - 1];
  const next = blocks[sourceIndex + 1];
  const deletion = next
    ? { from: source.from, to: next.from }
    : previous
      ? { from: previous.to, to: view.state.doc.length }
      : null;
  if (!deletion) return false;

  const insertPosition = side === "before" ? target.from : target.to;
  if (insertPosition > deletion.from && insertPosition < deletion.to) return false;
  const prefix = side === "after" ? "\n\n" : "";
  const suffix = side === "before" ? "\n\n" : "";
  const inserted = `${prefix}${sourceMarkdown}${suffix}`;
  const changes: ChangeSpec[] = [
    { from: deletion.from, to: deletion.to },
    { from: insertPosition, insert: inserted },
  ];
  const changeSet = view.state.changes(changes);
  const insertedFrom = changeSet.mapPos(insertPosition, -1) + prefix.length;
  view.dispatch({
    changes: changeSet,
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
      toolbar.dataset.dragging = "true";
    });
    drag.addEventListener("dragend", () => {
      delete toolbar.dataset.dragging;
    });
    toolbar.append(add, drag);
    return toolbar;
  }
}

function blockDecorations(
  state: CodeMirrorState,
  labels: CodeMirrorBlockDragLabels,
): DecorationSet {
  if (state.facet(EditorState.readOnly)) return Decoration.none;
  const decorations = readCodeMirrorBlockRanges(state).flatMap((block) => [
    Decoration.line({
      attributes: { "data-markra-block-from": String(block.from) },
    }).range(block.from),
    Decoration.widget({
      side: -1,
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

function dropTarget(event: DragEvent, view: CodeMirrorView) {
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
    return { from, side } as const;
  }
  try {
    const position = view.posAtCoords({ x: event.clientX, y: event.clientY });
    if (position === null) return null;
    const block = readCodeMirrorBlockRanges(view.state).find(
      (candidate) => position >= candidate.from && position <= candidate.to,
    );
    return block ? { from: block.from, side: "after" as const } : null;
  } catch {
    return null;
  }
}

function draggedBlockFrom(event: DragEvent) {
  const value = event.dataTransfer?.getData(blockDragMime) ?? "";
  const from = Number(value);
  return Number.isInteger(from) ? from : null;
}

class BlockDragViewPlugin {
  decorations: DecorationSet;

  constructor(view: CodeMirrorView, readonly labels: CodeMirrorBlockDragLabels) {
    this.decorations = blockDecorations(view.state, labels);
  }

  update(update: ViewUpdate) {
    if (
      update.docChanged ||
      update.startState.readOnly !== update.state.readOnly
    ) {
      this.decorations = blockDecorations(update.state, this.labels);
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
          if (draggedBlockFrom(event) === null || !dropTarget(event, view)) {
            return false;
          }
          event.preventDefault();
          if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
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
          );
          if (!handled) return false;
          event.preventDefault();
          return true;
        },
      }),
      blockDragTheme,
    ],
  });
}
