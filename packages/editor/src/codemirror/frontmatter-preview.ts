import type { Range } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type EditorView as CodeMirrorView,
  type ViewUpdate,
} from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin.ts";

export type CodeMirrorFrontmatterKind = "json" | "toml" | "yaml";

export interface CodeMirrorFrontmatterRange {
  readonly content: string;
  readonly from: number;
  readonly kind: CodeMirrorFrontmatterKind;
  readonly source: string;
  readonly to: number;
}

function findJsonObjectEnd(source: string, start: number) {
  let depth = 0;
  let escaped = false;
  let inString = false;

  for (let index = start; index < source.length; index += 1) {
    const character = source[index];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (character === "\\") {
        escaped = true;
      } else if (character === '"') {
        inString = false;
      }
      continue;
    }
    if (character === '"') {
      inString = true;
    } else if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
  }
  return null;
}

function readJsonFrontmatter(source: string, from: number) {
  if (source[from] !== "{") return null;
  const to = findJsonObjectEnd(source, from);
  if (to === null) return null;

  let after = to;
  while (source[after] === " " || source[after] === "\t") after += 1;
  if (after < source.length && source[after] !== "\n" && source[after] !== "\r") {
    return null;
  }

  const json = source.slice(from, to);
  try {
    const value = JSON.parse(json) as unknown;
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  } catch {
    return null;
  }
  return {
    content: json,
    from,
    kind: "json" as const,
    source: json,
    to,
  };
}

function readFencedFrontmatter(source: string, from: number) {
  const opening = /^(---|\+\+\+)[ \t]*(?:\r?\n|$)/u.exec(source.slice(from));
  const delimiter = opening?.[1];
  if (!opening || !delimiter) return null;

  const contentFrom = from + opening[0].length;
  const closingPattern = new RegExp(`^${delimiter.replace(/\+/gu, "\\+")}[ \\t]*$`, "gmu");
  closingPattern.lastIndex = contentFrom;
  const closing = closingPattern.exec(source);
  if (!closing) return null;

  const to = closing.index + closing[0].length;
  return {
    content: source.slice(contentFrom, closing.index).replace(/\r?\n$/u, ""),
    from,
    kind: delimiter === "---" ? "yaml" as const : "toml" as const,
    source: source.slice(from, to),
    to,
  };
}

export function readCodeMirrorFrontmatter(source: string) {
  const from = source.charCodeAt(0) === 0xfeff ? 1 : 0;
  return readFencedFrontmatter(source, from) ?? readJsonFrontmatter(source, from);
}

function selectionTouches(
  view: CodeMirrorView,
  range: CodeMirrorFrontmatterRange,
) {
  return (
    view.hasFocus &&
    view.state.selection.ranges.some((selection) =>
      selection.empty
        ? selection.head > range.from && selection.head < range.to
        : selection.from < range.to && selection.to > range.from,
    )
  );
}

class FrontmatterWidget extends WidgetType {
  constructor(readonly range: CodeMirrorFrontmatterRange) {
    super();
  }

  eq(other: FrontmatterWidget) {
    return other.range.source === this.range.source && other.range.kind === this.range.kind;
  }

  ignoreEvent() {
    return false;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const root = document.createElement("pre");
    const label = document.createElement("span");
    const code = document.createElement("code");

    root.className = "cm-markra-frontmatter markra-frontmatter";
    root.dataset.frontmatterKind = this.range.kind;
    root.dataset.type = "frontmatter";
    root.tabIndex = 0;
    root.setAttribute("role", "button");
    root.setAttribute("aria-label", `Edit ${this.range.kind.toUpperCase()} frontmatter`);
    label.className = "cm-markra-frontmatter-label";
    label.textContent = this.range.kind.toUpperCase();
    code.textContent = this.range.content;
    root.append(label, code);

    const activate = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      view.dispatch({
        selection: { anchor: Math.min(this.range.to - 1, this.range.from + 1) },
        scrollIntoView: true,
      });
      view.focus();
    };
    root.addEventListener("mousedown", activate);
    root.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") activate(event);
    });
    return root;
  }
}

function buildFrontmatterDecorations(view: CodeMirrorView) {
  const range = readCodeMirrorFrontmatter(view.state.doc.toString());
  if (!range || selectionTouches(view, range)) return Decoration.none;

  const decorations: Range<Decoration>[] = [];
  const firstLine = view.state.doc.lineAt(range.from);
  const lastLine = view.state.doc.lineAt(range.to);
  decorations.push(
    Decoration.replace({ widget: new FrontmatterWidget(range) }).range(
      range.from,
      Math.min(firstLine.to, range.to),
    ),
  );
  for (let lineNumber = firstLine.number + 1; lineNumber <= lastLine.number; lineNumber += 1) {
    const line = view.state.doc.line(lineNumber);
    const segmentTo = Math.min(line.to, range.to);
    if (line.from >= segmentTo) continue;
    if (segmentTo === line.to) {
      decorations.push(
        Decoration.line({ class: "cm-markra-frontmatter-hidden-line" }).range(
          line.from,
        ),
      );
    } else {
      decorations.push(Decoration.replace({}).range(line.from, segmentTo));
    }
  }
  return Decoration.set(decorations, true);
}

const frontmatterTheme = EditorView.baseTheme({
  ".cm-markra-frontmatter-hidden-line": {
    display: "none",
  },
  ".cm-markra-frontmatter": {
    background: "color-mix(in srgb, currentColor 4%, transparent)",
    border: "1px solid color-mix(in srgb, currentColor 14%, transparent)",
    borderRadius: "0.5em",
    display: "block",
    margin: "0.5em 0 1em",
    overflowX: "auto",
    padding: "0.75em 0.9em",
  },
  ".cm-markra-frontmatter-label": {
    display: "block",
    fontFamily: "system-ui, sans-serif",
    fontSize: "0.72em",
    fontWeight: "650",
    marginBottom: "0.4em",
    opacity: "0.55",
  },
});

export function frontmatterPreviewPlugin() {
  return defineMarkraPlugin({
    id: "markra.frontmatter-preview",
    extension: [
      ViewPlugin.fromClass(
        class {
          decorations: DecorationSet;

          constructor(view: CodeMirrorView) {
            this.decorations = buildFrontmatterDecorations(view);
          }

          update(update: ViewUpdate) {
            if (
              update.docChanged ||
              update.selectionSet ||
              update.focusChanged ||
              update.viewportChanged
            ) {
              this.decorations = buildFrontmatterDecorations(update.view);
            }
          }
        },
        { decorations: (plugin) => plugin.decorations },
      ),
      frontmatterTheme,
    ],
  });
}
