import { translatedLabel } from '../../../../ui-language';
import {
  activeTableCell,
  clearTableSelection,
  createTableCaretHost as createVisualTableCaretHost,
  isTableCaretPlaceholder as visualTableCellHasPlaceholderBreak,
  placeTableCellCaret as placeVisualTableCellCaret,
  revealTableCellHorizontally,
  tableCellCaretOffset,
  tableCaretPlaceholder as TABLE_CARET_PLACEHOLDER,
} from "./table-selection.ts";
import { isolateHistory, redo, undo } from "@codemirror/commands";
import { paneTableComment } from "../../markdown/table-fragment.ts";
import { markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { EditorState, Facet, Prec, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  keymap,
  ViewPlugin,
  WidgetType,
  type ViewUpdate,
  type EditorView as CodeMirrorView,
} from "@codemirror/view";
import { createCellTableControls, createTableControls, defaultTableLabels, type TableControlLabels } from "./table-controls.ts";
import { layoutTableColumns, tableDisplayChanged } from "./table-layout.ts";
import type { MarkraSourceRange } from "../math-syntax.ts";
import {
  renderInlineMarkdown,
  serializeInlineMarkdown,
  type InlineMarkdownImageDetails,
  type InlineMarkdownRenderOptions,
} from "./inline-markdown.ts";
import type { ImagePreviewPluginOptions } from "./image.ts";
import {
  openMarkraLinkSource,
  type LinksPluginOptions,
} from "./links.ts";
import { wikiLinkAvailability } from "./wiki-link.ts";
import { defineMarkraPlugin } from "./plugin.ts";
import { getMarkraRenderers, markraRenderer } from "./renderers.ts";
import { codeMirrorSearchHighlights } from "./search.ts";

export type CodeMirrorTableAlignment = "center" | "left" | "right" | null;
export type CodeMirrorTableWidthMode = "auto" | "even";

export interface CodeMirrorTableShape {
  readonly alignments: readonly CodeMirrorTableAlignment[];
  readonly columnCount: number;
}

export interface TablePreviewPluginOptions {
  getDocumentKey?: () => string | null | undefined;
  images?: ImagePreviewPluginOptions;
  labels?: Partial<TablePreviewLabels>;
  links?: LinksPluginOptions;
  widthMode?: CodeMirrorTableWidthMode;
}

export type TablePreviewLabels = TableControlLabels;

type TableMenuScope = "cell" | "row" | "column" | "table";
type TableLinePick = { axis: "row" | "column"; index: number };

function tableHistoryKey(event: KeyboardEvent) {
  const key = event.key.toLowerCase();
  if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
  if (key === "z") return event.shiftKey ? "redo" : "undo";
  if (key === "y" && !event.metaKey && !event.shiftKey) return "redo";
}

type TableMenuCommands = {
  clipboard: { text: string; replace: (text: string) => void };
  choices?: { options: readonly string[]; value: string; set: (value: string) => void };
  insertRowBefore?: () => void;
  insertRowAfter: () => void;
  insertColumnBefore: () => void;
  insertColumnAfter: () => void;
  deleteRow?: () => void;
  deleteColumn?: () => void;
  deleteTable: () => void;
};
export const tableContextMenu = Facet.define<(position: { x: number; y: number; bounds?: DOMRectReadOnly; cell: HTMLTableCellElement; tableSelection?: MarkraSourceRange }, commands: TableMenuCommands, scope: TableMenuScope, restoreFocus: () => void) => void>();
const tableActions = new WeakMap<HTMLTableElement, {
  openMenu: (cell: HTMLTableCellElement, x: number, y: number, scope: TableMenuScope, bounds?: DOMRectReadOnly, capturedRange?: Range | null) => void;
  format: (cell: HTMLTableCellElement, action: string) => void;
  captureInsertion: (cell: HTMLTableCellElement) => (text: (selected: string) => string) => boolean;
}>();

function focusedTable(view: EditorView) {
  const table = view.dom.ownerDocument.activeElement?.closest<HTMLTableElement>("table.cm-markra-table");
  return table && view.dom.contains(table) ? table : null;
}

export function captureActiveTableMenu(view: EditorView, anchor: HTMLElement) {
  const table = focusedTable(view);
  if (!table) return;
  const cell = activeVisualTableCell(view, table);
  const actions = tableActions.get(table);
  if (!cell || !actions) return;
  const doc = view.state.doc;
  const selection = table.ownerDocument.getSelection();
  const range = selection?.rangeCount ? selection.getRangeAt(0).cloneRange() : undefined;
  return () => {
    if (!cell.isConnected || view.state.doc !== doc) return;
    const bounds = anchor.getBoundingClientRect();
    actions.openMenu(cell, bounds.left, bounds.bottom + 4, "table", undefined, range);
  };
}

export function formatActiveTableSelection(view: EditorView, action: string) {
  const table = focusedTable(view);
  if (!table) return false;
  const cell = activeVisualTableCell(view, table);
  if (cell) tableActions.get(table)?.format(cell, action);
  return true;
}

const cellFormatTags = { bold: "strong", italic: "em", underline: "u", strike: "del", highlight: "mark", code: "code" } as const;
type CellFormat = keyof typeof cellFormatTags;
const cellFormatElements = Object.values(cellFormatTags).join(",");

function cellFormattingElement(cell: HTMLTableCellElement, range: Range, tag: string) {
  const start = range.startContainer instanceof Element ? range.startContainer : range.startContainer.parentElement;
  const element = start?.closest(tag);
  return element && cell.contains(element) && element.contains(range.endContainer) ? element : null;
}

function formatCellRange(cell: HTMLTableCellElement, range: Range, tag: string) {
  const existing = cellFormattingElement(cell, range, tag);
  if (existing) {
    existing.replaceWith(...existing.childNodes);
    return;
  }
  const formatted = cell.ownerDocument.createElement(tag);
  const boundaries = [range.startContainer, range.endContainer].flatMap(node => {
    const elements: Element[] = [];
    for (let element = node instanceof Element ? node : node.parentElement; element && element !== cell; element = element.parentElement) {
      if (element.matches(cellFormatElements)) elements.push(element);
    }
    return elements;
  });
  const contents = range.extractContents();
  for (const duplicate of contents.querySelectorAll(tag)) duplicate.replaceWith(...duplicate.childNodes);
  formatted.append(contents);
  range.insertNode(formatted);
  range.selectNodeContents(formatted);
  for (const element of boundaries) {
    if (!element.textContent && !element.childElementCount) element.remove();
  }
}

function cellTextOffset(cell: HTMLTableCellElement, node: Node, offset: number) {
  const prefix = cell.ownerDocument.createRange();
  prefix.selectNodeContents(cell);
  prefix.setEnd(node, offset);
  return prefix.toString().length;
}

function cellRangeAtOffsets(cell: HTMLTableCellElement, from: number, to: number) {
  const range = cell.ownerDocument.createRange();
  range.selectNodeContents(cell);
  const walker = cell.ownerDocument.createTreeWalker(cell, NodeFilter.SHOW_TEXT);
  let startFound = false, offset = 0;
  for (let text = walker.nextNode() as Text | null; text; text = walker.nextNode() as Text | null) {
    const end = offset + text.length;
    if (!startFound && from < end) {
      range.setStart(text, from - offset);
      startFound = true;
    }
    if (startFound && to <= end) {
      range.setEnd(text, to - offset);
      break;
    }
    offset = end;
  }
  return startFound ? range : null;
}

function draftTableLink(cell: HTMLTableCellElement, selected: Range) {
  const clone = cell.cloneNode(true) as HTMLTableCellElement;
  if (visualTableCellHasPlaceholderBreak(clone)) clone.replaceChildren();
  const document = cell.ownerDocument;
  const from = cellTextOffset(cell, selected.startContainer, selected.startOffset);
  const to = cellTextOffset(cell, selected.endContainer, selected.endOffset);
  let range = cellRangeAtOffsets(clone, from, to);
  if (!range) {
    if (from !== to) return;
    range = document.createRange();
    range.selectNodeContents(clone);
    range.collapse(false);
  }
  const label = (range.toString() || "链接").replace(/[\\\[\]]/gu, "\\$&");
  const ending = "](https://)";
  const link = document.createTextNode(`[${label}${ending}`);
  range.deleteContents();
  range.insertNode(link);

  // Count the Markdown of the text before the URL, including any inline format markers.
  range.selectNodeContents(clone);
  range.setEndAfter(link);
  const before = document.createElement("span");
  before.append(range.cloneContents());
  const prefix = serializeInlineMarkdown(before);
  const url = prefix.lastIndexOf(ending);
  const source = serializeInlineMarkdown(clone);
  if (url < 0 || source === visualTableCellSource(cell)) return;
  return { source, caret: url + ending.length - 1 };
}

export function captureTableFormat(view: EditorView) {
  const selection = view.dom.ownerDocument.getSelection();
  const node = selection?.anchorNode;
  const element = node instanceof Element ? node : node?.parentElement;
  const table = focusedTable(view) ?? element?.closest<HTMLTableElement>("table.cm-markra-table");
  if (!table || !view.dom.contains(table)) return null;
  const cell = activeVisualTableCell(view, table);
  const tableFrom = Number(table.closest<HTMLElement>(".cm-markra-table-wrap")?.dataset.tableFrom);
  if (!cell || !Number.isInteger(tableFrom)) return null;
  const tableNode = tableNodeAt(view.state, tableFrom, 1);
  if (!tableNode) return null;
  const preview = tablePreviewAt(view.state, tableNode.headerFrom, tableNode.to);
  const row = Number(cell.dataset.tableRow), column = Number(cell.dataset.tableColumn);
  const header = cell.dataset.tableHeader === "true";
  const source = header ? preview?.header[column] : preview?.rows[row]?.[column];
  if (!source) return null;
  const selector = `[data-table-row="${row}"][data-table-column="${column}"][data-table-header="${header}"]`;
  const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
  const inside = range && cell.contains(range.commonAncestorContainer);
  const from = inside ? cellTextOffset(cell, range.startContainer, range.startOffset) : 0;
  const to = inside ? cellTextOffset(cell, range.endContainer, range.endOffset) : from;
  let clone = cell.cloneNode(true) as HTMLTableCellElement;
  // The grid may be redrawn after a format. Keep the selected offsets in the captured cell.
  const word = inside && from === to ? EditorState.create({ doc: clone.textContent ?? "" }).wordAt(from) : null;
  const selected = (target = clone) => inside && (from !== to || word) ? cellRangeAtOffsets(target, word?.from ?? from, word?.to ?? to) : null;
  let doc = view.state.doc;
  let sourceEnd = source.to;
  const current = () => view.dom.isConnected && view.state.doc === doc;
  const findCell = (grid?: HTMLTableElement | null) => grid?.querySelector<HTMLTableCellElement>(selector);
  const cellElement = () => current() ? findCell(renderedTableAt(view, tableFrom)) : undefined;
  return {
    sourceStart: source.from,
    element: cellElement,
    bounds: () => cellElement()?.getBoundingClientRect(),
    canFormat: () => current() && !view.state.readOnly && Boolean(selected()),
    active: () => {
      const range = selected();
      return range ? (Object.keys(cellFormatTags) as CellFormat[]).filter(action => cellFormattingElement(clone, range, cellFormatTags[action])) : [];
    },
    apply(action: string) {
      if (!current() || view.state.readOnly || view.compositionStarted || !Object.hasOwn(cellFormatTags, action)) return;
      const next = clone.cloneNode(true) as HTMLTableCellElement;
      const range = selected(next);
      if (!range) return;
      const before = serializeInlineMarkdown(clone);
      formatCellRange(next, range, cellFormatTags[action as CellFormat]);
      const markdown = serializeInlineMarkdown(next);
      if (markdown === before) return;
      const edit = view.state.update({
        changes: { from: source.from, to: sourceEnd, insert: markdown },
        annotations: isolateHistory.of("full"),
        userEvent: "input.format",
      });
      if (!edit.docChanged) return;
      view.dispatch(edit);
      clone = next;
      doc = view.state.doc;
      sourceEnd = source.from + markdown.length;
    },
    restore() {
      if (!current()) return false;
      const grid = renderedTableAt(view, tableFrom);
      const target = findCell(grid);
      if (!grid || !target) return false;
      const restored = cellRangeAtOffsets(target, from, to);
      if (selection && restored) {
        // The table owns WebKit's keyboard focus, while the range belongs to the cell.
        grid.focus({ preventScroll: true });
        selection.removeAllRanges();
        selection.addRange(restored);
        revealTableCellHorizontally(target);
      } else placeVisualTableCellCaret(target, from);
      return true;
    },
  };
}

export function captureTableInsertion(view: EditorView) {
  const table = focusedTable(view);
  if (!table) return null;
  const cell = activeVisualTableCell(view, table);
  return cell ? tableActions.get(table)?.captureInsertion(cell) ?? null : null;
}

interface TableCellPreview {
  readonly from: number;
  readonly source: string;
  readonly to: number;
}

type EditVisualTableMathSource = (
  element: HTMLElement,
  markdown: string,
) => void;

const visualTableMathSourceEditors = new WeakMap<
  HTMLTableCellElement,
  EditVisualTableMathSource
>();

interface TablePreview {
  readonly alignments: readonly CodeMirrorTableAlignment[];
  // Include the schema in the table's anchor so adding/removing it can keep the same focus target.
  readonly from: number;
  readonly headerFrom: number;
  readonly header: readonly TableCellPreview[];
  readonly rows: readonly (readonly TableCellPreview[])[];
  readonly to: number;
  readonly types?: readonly string[];
}

interface TableEditingSession {
  readonly column: number;
  readonly header: boolean;
  readonly inlineSourceVisible: boolean;
  readonly mathFrom?: number;
  readonly mathTo?: number;
  readonly row: number;
  readonly tableFrom: number;
}


const tableSizePickerColumns = 8;
const tableSizePickerRows = 10;
const tablePreviewRendererId = "markra.table-preview";
const tableWidthModeStoragePrefix = "markra:table-width-mode";
const tableEditingSessions = new WeakMap<CodeMirrorView, TableEditingSession>();
const tableLinePicks = new WeakMap<HTMLTableElement, TableLinePick>();

export function tablePreviewEnabled(state: EditorState) {
  return getMarkraRenderers(state, "Table").some(
    (renderer) => renderer.id === tablePreviewRendererId,
  );
}

function tableWidthModeStorageKey(documentKey: string | null | undefined) {
  const normalizedKey = documentKey?.trim() || "untitled";
  return `${tableWidthModeStoragePrefix}:${encodeURIComponent(normalizedKey)}`;
}

function readStoredWidthMode(
  document: Document,
  documentKey: string | null | undefined,
  fallback: CodeMirrorTableWidthMode,
) {
  try {
    const stored = document.defaultView?.localStorage.getItem(
      tableWidthModeStorageKey(documentKey),
    );
    return stored === "auto" || stored === "even" ? stored : fallback;
  } catch {
    return fallback;
  }
}

function writeStoredWidthMode(
  document: Document,
  documentKey: string | null | undefined,
  mode: CodeMirrorTableWidthMode,
) {
  try {
    document.defaultView?.localStorage.setItem(
      tableWidthModeStorageKey(documentKey),
      mode,
    );
  } catch {
    // Storage can be unavailable in embedded or privacy-restricted surfaces.
  }
}

function trimCellRange(line: string, from: number, to: number) {
  while (from < to && /\s/u.test(line[from] ?? "")) from += 1;
  while (to > from && /\s/u.test(line[to - 1] ?? "")) to -= 1;
  return { from, to };
}

function tableCells(line: string, lineFrom: number) {
  let rowFrom = line.search(/\S/u);
  if (rowFrom < 0) rowFrom = 0;
  let rowTo = line.length;
  while (rowTo > rowFrom && /\s/u.test(line[rowTo - 1] ?? "")) rowTo -= 1;
  if (line[rowFrom] === "|") rowFrom += 1;
  if (line[rowTo - 1] === "|") rowTo -= 1;

  const cells: TableCellPreview[] = [];
  let cellFrom = rowFrom;
  let escaped = false;
  const pushCell = (cellTo: number) => {
    const trimmed = trimCellRange(line, cellFrom, cellTo);
    const source = line.slice(trimmed.from, trimmed.to);
    cells.push({
      from: lineFrom + trimmed.from,
      source,
      to: lineFrom + trimmed.to,
    });
  };

  for (let cursor = rowFrom; cursor < rowTo; cursor += 1) {
    const character = line[cursor];
    if (character === "|" && !escaped) {
      pushCell(cursor);
      cellFrom = cursor + 1;
      continue;
    }
    escaped = character === "\\" ? !escaped : false;
  }
  pushCell(rowTo);
  return cells;
}

const tableSeparatorPattern = /^:?-+:?$/u;

export function restoreCopiedTableHeader(text: string, before = "", after = "") {
  if (!/^ {0,3}\|[ \t]*:?-+/mu.test(text)) return text;
  const root = markdownLanguage.parser.parse(text).topNode;
  let result = "";
  let copiedTo = 0;
  for (let node = root.firstChild; node; node = node.nextSibling) {
    if (node.name !== "Paragraph") continue;
    const block = text.slice(node.from, node.to);
    const lines = block.split(/\r?\n/u);
    if (lines.length < 2 || !lines.every(line => /^[ \t]*\|.*\|[ \t\r]*$/u.test(line))) continue;
    const separator = tableCells(lines[0]!, 0);
    if (!separator.every(cell => tableSeparatorPattern.test(cell.source))) continue;
    if (!lines.slice(1).every(line => tableCells(line, 0).length === separator.length)) continue;
    const lineStart = text.lastIndexOf("\n", node.from - 1) + 1;
    if (before && !result && !text.slice(0, lineStart).trim()) result += before;
    result += text.slice(copiedTo, node.from) + serializeRow(separator.map(() => ""));
    result += block.match(/\r?\n/u)![0] + text.slice(lineStart, node.from);
    copiedTo = node.from;
    if (after && !node.nextSibling) return result + text.slice(copiedTo) + after;
  }
  return result + text.slice(copiedTo);
}

function alignmentFromSeparator(cell: string): CodeMirrorTableAlignment {
  const source = cell.trim();
  if (!tableSeparatorPattern.test(source)) return null;
  if (source.startsWith(":") && source.endsWith(":")) return "center";
  if (source.endsWith(":")) return "right";
  if (source.startsWith(":")) return "left";
  return null;
}

function tablePreview(
  source: string,
  from: number,
  to: number,
): TablePreview | null {
  const lines = source.split(/\r?\n/u);
  if (lines.length < 2) return null;

  const lineOffsets: number[] = [];
  let offset = 0;
  for (const line of lines) {
    lineOffsets.push(offset);
    offset += line.length + 1;
  }
  const header = tableCells(lines[0] ?? "", from + (lineOffsets[0] ?? 0));
  const separators = tableCells(lines[1] ?? "", from + (lineOffsets[1] ?? 0));
  if (
    header.length === 0 ||
    separators.length !== header.length ||
    separators.some((cell) => !tableSeparatorPattern.test(cell.source.trim()))
  ) {
    return null;
  }

  return {
    alignments: separators.map((cell) => alignmentFromSeparator(cell.source)),
    from,
    headerFrom: from,
    header,
    rows: lines.slice(2).filter((line) => line.trim()).map((line, index) => {
      const cells = tableCells(
        line,
        from + (lineOffsets[index + 2] ?? 0),
      );
      return Array.from({ length: header.length }, (_, column) =>
        cells[column] ?? {
          from: from + (lineOffsets[index + 2] ?? 0) + line.length,
          source: "",
          to: from + (lineOffsets[index + 2] ?? 0) + line.length,
        },
      );
    }),
    to,
  };
}

function commentBeforeTable(state: EditorState, headerFrom: number) {
  const line = state.doc.lineAt(headerFrom);
  if (line.number === 1) return;
  const previous = state.doc.line(line.number - 1);
  const spec = paneTableComment.exec(previous.text)?.[1];
  if (spec !== undefined) return { from: previous.from, spec };
}

function paneColumnName(source: string) {
  return /[;=]|-->/u.test(source) ? "" : source.replaceAll("\\|", "|");
}

function tablePreviewAt(state: EditorState, from: number, to: number) {
  const preview = tablePreview(state.sliceDoc(from, to), from, to);
  const comment = commentBeforeTable(state, from);
  if (!preview || !comment) return preview;
  const entries = comment.spec.split(";").flatMap(part => {
    const at = part.indexOf("=");
    return at < 0 ? [] : [[part.slice(0, at).trim().toLowerCase(), part.slice(at + 1).trim() || "text"] as const];
  });
  const names = preview.header.map(cell => paneColumnName(cell.source).toLowerCase());
  const inOrder = entries.length === names.length && entries.every(([name], index) => name === names[index]);
  const byName = new Map(entries);
  const types = inOrder ? entries.map(([, type]) => type) : names.map(name => byName.get(name) ?? "text");
  return { ...preview, from: comment.from, types };
}

function checkboxOptions(type: string | undefined) {
  const choice = /^choice\s+(.+)$/iu.exec(type ?? "");
  const options = choice?.[1]?.split("|").map(option => option.trim()).filter(Boolean);
  if (options?.[0]?.toLowerCase() === "yes") return { yes: options[0], no: options[1] ?? "No", options };
}

function serializeTableTypes(header: readonly string[], types: readonly string[] | undefined) {
  if (!types?.some(type => type.toLowerCase() !== "text")) return "";
  return `<!-- pane-table: ${header.map((name, index) => `${paneColumnName(name)}=${types[index] ?? "text"}`).join("; ")} -->\n`;
}

export function readCodeMirrorTableShape(source: string): CodeMirrorTableShape | null {
  const preview = tablePreview(source, 0, source.length);
  if (!preview) return null;
  return {
    alignments: preview.alignments,
    columnCount: preview.header.length,
  };
}

function separatorFor(alignment: CodeMirrorTableAlignment) {
  if (alignment === "center") return ":---:";
  if (alignment === "right") return "---:";
  if (alignment === "left") return ":---";
  return "---";
}

function serializeRow(cells: readonly string[]) {
  return `| ${cells.join(" | ")} |`;
}

function serializeTable(
  header: readonly string[],
  rows: readonly (readonly string[])[],
  alignments: readonly CodeMirrorTableAlignment[],
) {
  return [
    serializeRow(header),
    serializeRow(alignments.map(separatorFor)),
    ...rows.map(serializeRow),
  ].join("\n");
}

function replaceTable(
  view: CodeMirrorView,
  preview: TablePreview,
  header: readonly string[],
  rows: readonly (readonly string[])[],
  alignments: readonly CodeMirrorTableAlignment[],
  { focus = false, typing = false, types = preview.types, scrollIntoView = false }:
    { focus?: boolean; typing?: boolean; types?: readonly string[]; scrollIntoView?: boolean } = {},
) {
  if (view.state.readOnly) return false;
  const updateTypes = types !== preview.types || Boolean(types) &&
    (header.length !== preview.header.length || header.some((name, index) => name !== preview.header[index]?.source));
  view.dispatch({
    changes: {
      from: updateTypes ? preview.from : preview.headerFrom,
      insert: (updateTypes ? serializeTableTypes(header, types) : "") + serializeTable(header, rows, alignments),
      to: preview.to,
    },
    annotations: typing ? undefined : isolateHistory.of("full"),
    scrollIntoView,
  });
  if (focus) view.focus();
  return true;
}

function tableValues(preview: TablePreview) {
  return {
    alignments: [...preview.alignments],
    header: preview.header.map((cell) => cell.source),
    rows: preview.rows.map((row) => row.map((cell) => cell.source)),
  };
}

function addRow(view: CodeMirrorView, preview: TablePreview, rowIndex = preview.rows.length, column = 0) {
  const values = tableValues(preview);
  values.rows.splice(rowIndex, 0, Array.from({ length: values.header.length }, () => ""));
  const changed = replaceTable(
    view,
    preview,
    values.header,
    values.rows,
    values.alignments,
  );
  if (changed) focusVisualTableCell(view, preview.from, rowIndex, column, false, 0);
  return changed;
}

function addColumn(view: CodeMirrorView, preview: TablePreview, columnIndex = preview.header.length) {
  const values = tableValues(preview);
  values.header.splice(columnIndex, 0, "");
  values.alignments.splice(columnIndex, 0, null);
  for (const row of values.rows) row.splice(columnIndex, 0, "");
  const types = preview.types?.slice();
  types?.splice(columnIndex, 0, "text");
  const changed = replaceTable(
    view,
    preview,
    values.header,
    values.rows,
    values.alignments,
    { types },
  );
  if (changed) focusVisualTableCell(view, preview.from, -1, columnIndex, true, 0);
  return changed;
}

function alignTable(
  view: CodeMirrorView,
  preview: TablePreview,
  alignment: Exclude<CodeMirrorTableAlignment, null>,
) {
  const values = tableValues(preview);
  values.alignments.fill(alignment);
  return replaceTable(view, preview, values.header, values.rows, values.alignments, { focus: true });
}

function deleteRow(view: CodeMirrorView, preview: TablePreview, rowIndex: number, column = 0) {
  const values = tableValues(preview);
  if (!values.rows.length || rowIndex < -1 || rowIndex >= values.rows.length) return false;
  const header = rowIndex === -1 ? values.rows.shift()! : values.header;
  if (rowIndex >= 0) values.rows.splice(rowIndex, 1);
  const changed = replaceTable(
    view,
    preview,
    header,
    values.rows,
    values.alignments,
  );
  if (changed) {
    const nextRow = Math.min(rowIndex, values.rows.length - 1);
    focusVisualTableCell(
      view,
      preview.from,
      nextRow,
      column,
      nextRow < 0,
      0,
    );
  }
  return changed;
}

function deleteColumn(
  view: CodeMirrorView,
  preview: TablePreview,
  columnIndex: number,
  row = -1,
) {
  const values = tableValues(preview);
  if (values.header.length <= 1 || columnIndex < 0 || columnIndex >= values.header.length) {
    return false;
  }
  values.header.splice(columnIndex, 1);
  values.alignments.splice(columnIndex, 1);
  for (const row of values.rows) row.splice(columnIndex, 1);
  const types = preview.types?.slice();
  types?.splice(columnIndex, 1);
  const changed = replaceTable(
    view,
    preview,
    values.header,
    values.rows,
    values.alignments,
    { types },
  );
  if (changed) {
    focusVisualTableCell(
      view,
      preview.from,
      row,
      Math.min(columnIndex, values.header.length - 1),
      row < 0,
      0,
    );
  }
  return changed;
}

function deleteTable(view: CodeMirrorView, preview: TablePreview) {
  if (view.state.readOnly) return false;
  const consumeNewline = view.state.sliceDoc(preview.to, preview.to + 2) === "\n\n";
  view.dispatch({
    changes: {
      from: preview.from,
      insert: "",
      to: preview.to + (consumeNewline ? 1 : 0),
    },
    selection: { anchor: preview.from },
    scrollIntoView: true,
    userEvent: "input.table.delete",
    annotations: isolateHistory.of("full"),
  });
  view.focus();
  return true;
}

function resizedValues(
  preview: TablePreview,
  columnCount: number,
  totalRowCount: number,
) {
  const values = tableValues(preview);
  const columns = Math.max(
    1,
    Math.min(tableSizePickerColumns, Math.trunc(columnCount)),
  );
  const totalRows = Math.max(
    1,
    Math.min(tableSizePickerRows, Math.trunc(totalRowCount)),
  );
  const header = Array.from(
    { length: columns },
    (_, index) => values.header[index] ?? "",
  );
  const alignments = Array.from(
    { length: columns },
    (_, index) => values.alignments[index] ?? null,
  );
  const rows = Array.from({ length: Math.max(0, totalRows - 1) }, (_, row) =>
    Array.from(
      { length: columns },
      (_, column) => values.rows[row]?.[column] ?? "",
    ));
  return { alignments, header, rows };
}

function resizeTable(
  view: CodeMirrorView,
  preview: TablePreview,
  columns: number,
  rows: number,
) {
  if (view.state.readOnly) return false;
  const values = resizedValues(preview, columns, rows);
  const types = preview.types?.slice(0, values.header.length);
  if (!replaceTable(view, preview, values.header, values.rows, values.alignments, { types, scrollIntoView: true })) return false;
  const lastRow = values.rows.length - 1;
  focusVisualTableCell(
    view,
    preview.from,
    lastRow,
    values.header.length - 1,
    lastRow < 0,
    0,
  );
  return true;
}

function normalizedTableAlignment(preview: TablePreview) {
  const alignments = new Set(
    preview.alignments.map((alignment) => alignment ?? "left"),
  );
  return alignments.size === 1 ? [...alignments][0] : null;
}

function applyWidthModeToTableControls(
  view: CodeMirrorView,
  mode: CodeMirrorTableWidthMode,
) {
  for (const wrapper of view.dom.querySelectorAll<HTMLElement>(
    ".cm-markra-table-wrap[data-table-from]",
  )) {
    wrapper.dataset.widthMode = mode;
    const table = wrapper.querySelector<HTMLTableElement>(".cm-markra-table");
    if (table) {
      table.dataset.widthMode = mode;
      table.classList.toggle("markra-table-width-auto", mode === "auto");
    }
    const button = wrapper.querySelector<HTMLButtonElement>(
      ".markra-table-width-button",
    );
    if (button) {
      button.ariaPressed = String(mode === "auto");
      button.dataset.mode = mode;
    }
  }
}

function visualTableCellSource(cell: HTMLTableCellElement) {
  // Browsers keep a lone <br> as the caret placeholder after the last
  // character is deleted. Persisted line breaks are tagged during rendering,
  // so only the browser-owned placeholder represents an empty cell.
  if (visualTableCellHasPlaceholderBreak(cell)) return "";
  return serializeInlineMarkdown(cell);
}

function replaceVisualTableCell(
  view: CodeMirrorView,
  preview: TablePreview,
  rowIndex: number,
  columnIndex: number,
  header: boolean,
  source: string,
  typing = true,
) {
  const values = tableValues(preview);
  const currentSource = header
    ? values.header[columnIndex]
    : values.rows[rowIndex]?.[columnIndex];
  if (currentSource === source) return false;

  if (header) {
    values.header[columnIndex] = source;
  } else {
    const row = values.rows[rowIndex];
    if (!row) return false;
    row[columnIndex] = source;
  }

  return replaceTable(
    view,
    preview,
    values.header,
    values.rows,
    values.alignments,
    { typing },
  );
}

function activeVisualTableCell(
  view: CodeMirrorView,
  table: HTMLTableElement,
) {
  const active = activeTableCell(table);
  if (active) return active;

  // WebKit can lift the selection to the row/table after deleting the final
  // character. The editing session still identifies the cell that must sync.
  const session = tableEditingSessions.get(view);
  const wrapper = table.closest<HTMLElement>(".cm-markra-table-wrap");
  if (
    !session ||
    wrapper?.dataset.tableFrom !== String(session.tableFrom)
  ) {
    return null;
  }
  return table.querySelector<HTMLTableCellElement>(
    `[data-table-row="${session.row}"]` +
      `[data-table-column="${session.column}"]` +
      `[data-table-header="${String(session.header)}"]`,
  );
}

function repairVisualTableCellSelection(
  view: CodeMirrorView,
  table: HTMLTableElement,
) {
  const cell = activeVisualTableCell(view, table);
  const selectionNode = table.ownerDocument.getSelection()?.anchorNode;
  if (cell && (!selectionNode || !cell.contains(selectionNode))) {
    placeVisualTableCellCaret(cell, tableCellCaretOffset(cell));
  }
}

function stabilizeVisualTableCompositionCaret(
  view: CodeMirrorView,
  table: HTMLTableElement,
) {
  const cell = activeVisualTableCell(view, table);
  if (!cell || visualTableCellSource(cell) !== "") return;

  // An empty Text node is a valid DOM range but WebKit may move IME text to
  // the surrounding table row. A zero-width span keeps the native insertion
  // point inside the empty cell and is stripped when Markdown is serialized.
  const caretHost = createVisualTableCaretHost(cell.ownerDocument);
  cell.replaceChildren(caretHost.host);
  cell.focus();
  const selection = cell.ownerDocument.getSelection();
  if (!selection) return;
  const range = cell.ownerDocument.createRange();
  range.setStart(caretHost.text, TABLE_CARET_PLACEHOLDER.length);
  range.collapse(true);
  selection.removeAllRanges();
  selection.addRange(range);
}

function moveVisualTableCellFocus(
  view: CodeMirrorView,
  preview: TablePreview,
  table: HTMLTableElement,
  backward: boolean,
  cell: HTMLTableCellElement,
) {
  const cells = Array.from(
    table.querySelectorAll<HTMLTableCellElement>("th, td:not([data-table-check])"),
  );
  const target = cells[cells.indexOf(cell) + (backward ? -1 : 1)];
  if (target) placeVisualTableCellCaret(target, tableCellCaretOffset(target));
  else if (!backward) {
    if (preview.types?.every(type => checkboxOptions(type))) leaveTable(view, preview, true);
    else addRow(view, preview);
  }
}

function renderedTableAt(view: CodeMirrorView, from: number) {
  return view.dom.querySelector<HTMLTableElement>(`.cm-markra-table-wrap[data-table-from="${from}"] .cm-markra-table`);
}

function setTableLinePick(table: HTMLTableElement, pick?: TableLinePick) {
  if (!pick && !tableLinePicks.has(table)) return;
  for (const cell of table.querySelectorAll("[data-table-line-picked]")) cell.removeAttribute("data-table-line-picked");
  if (pick) {
    const cells = table.querySelectorAll(`[data-table-${pick.axis}="${pick.axis === "row" ? pick.index - 1 : pick.index}"]`);
    if (!cells.length) pick = undefined;
    else for (const cell of cells) cell.setAttribute("data-table-line-picked", "");
  }
  if (pick) tableLinePicks.set(table, pick);
  else tableLinePicks.delete(table);
  const wrapper = table.closest(".qingyu-table-controls");
  wrapper?.toggleAttribute("data-table-line-pick", Boolean(pick));
  for (const axis of ["row", "column"] as const) {
    wrapper?.querySelector(`.qingyu-table-handle-${axis}`)?.setAttribute("aria-pressed", String(pick?.axis === axis));
  }
}

function restoreTableCaret(view: CodeMirrorView, from: number, row: number, column: number, pick?: TableLinePick) {
  const grid = renderedTableAt(view, from);
  const cells = grid?.rows[Math.min(row, grid.rows.length - 1)]?.cells;
  const target = cells?.[Math.min(column, cells.length - 1)];
  if (!target || !grid) { view.focus(); return; }
  placeVisualTableCellCaret(target, tableCellCaretOffset(target));
  setTableLinePick(grid, pick);
}

function tableSyntaxAt(state: EditorState, position: number, side: 1 | -1) {
  let node: ReturnType<typeof syntaxTree>["topNode"] | null = syntaxTree(state).resolveInner(position, side);
  while (node && node.name !== "Table") node = node.parent;
  return node;
}

function tableNodeAt(state: EditorState, position: number, side: 1 | -1) {
  let table = tableSyntaxAt(state, position, side);
  if (!table) {
    const line = state.doc.lineAt(position);
    if (line.number === state.doc.lines || !paneTableComment.test(line.text)) return null;
    table = tableSyntaxAt(state, state.doc.line(line.number + 1).from, 1);
  }
  if (!table) return null;
  return { from: commentBeforeTable(state, table.from)?.from ?? table.from, headerFrom: table.from, to: table.to };
}

function enterAdjacentTable(view: CodeMirrorView, down: boolean) {
  const { doc, selection, readOnly } = view.state;
  if (readOnly || view.compositionStarted || selection.ranges.length !== 1 || !selection.main.empty) return false;
  const line = doc.lineAt(selection.main.head);
  const nextLine = line.number + (down ? 1 : -1);
  if (nextLine < 1 || nextLine > doc.lines) return false;
  const next = doc.line(nextLine);
  const node = tableNodeAt(view.state, down ? next.from : next.to, down ? 1 : -1);
  if (!node) return false;
  const table = renderedTableAt(view, node.from);
  const cell = table?.rows[down ? 0 : table.rows.length - 1]?.cells[0];
  if (!cell) return false;
  // A wrapped paragraph can still move within the same source line.
  if (line.length && doc.lineAt(view.moveVertically(selection.main, down).head).number === line.number) return false;
  placeVisualTableCellCaret(cell, tableCellCaretOffset(cell));
  return true;
}

function leaveTable(view: CodeMirrorView, preview: Pick<TablePreview, "from" | "to">, down: boolean) {
  const { doc } = view.state;
  const edge = doc.lineAt(down ? preview.to : preview.from);
  let changes: { from: number; insert: string } | undefined;
  let anchor: number;
  if (!down) {
    anchor = Math.max(0, edge.from - 1);
    if (edge.from === 0) changes = { from: 0, insert: "\n" };
  } else if (edge.number === doc.lines) {
    changes = { from: edge.to, insert: "\n\n" };
    anchor = edge.to + 2;
  } else {
    const next = doc.line(edge.number + 1);
    // GFM treats plain text on a line immediately after a table as another row.
    const separator = !next.text.trim();
    const boundary = separator ? next.to : next.from;
    if (!separator || next.number === doc.lines || tableNodeAt(view.state, boundary + 1, 1)) {
      changes = { from: boundary, insert: "\n" };
    }
    anchor = boundary + 1;
  }
  view.dispatch({
    changes,
    annotations: changes ? isolateHistory.of("full") : undefined,
    selection: { anchor },
    scrollIntoView: true,
  });
  view.focus();
}

type TableEdgeInput = { anchor: number; before: string; after: string };
const setTableEdgeInput = StateEffect.define<TableEdgeInput | null>();
const tableEdgeInput = StateField.define<TableEdgeInput | null>({
  create: () => null,
  update(value, transaction) {
    for (const effect of transaction.effects) if (effect.is(setTableEdgeInput)) return effect.value;
    return transaction.docChanged || transaction.selection && !transaction.startState.selection.eq(transaction.selection) ? null : value;
  },
});

const padTableEdgeInput = EditorState.transactionFilter.of(transaction => {
  const placement = transaction.startState.field(tableEdgeInput);
  if (!placement || !transaction.docChanged) return transaction;
  if (transaction.isUserEvent("delete")) return [];
  if (!transaction.isUserEvent("input") || transaction.isUserEvent("input.tableBoundary")) return transaction;
  const { anchor, before, after } = placement;
  let size = 0, simpleInsertion = true;
  transaction.changes.iterChanges((from, to, _fromNew, _toNew, inserted) => {
    if (size || from !== anchor || to !== anchor) simpleInsertion = false;
    else size = inserted.length;
  });
  if (!simpleInsertion || !size) return transaction;
  // Keep the separator with the edit in one undo step and the caret before the trailing padding.
  return [transaction, { sequential: true,
    changes: [{ from: anchor, insert: before }, { from: anchor + size, insert: after }],
    selection: { anchor: anchor + before.length + size },
  }];
});

function selectAfterTable(view: CodeMirrorView, to: number, atEdge: boolean) {
  const { state } = view;
  const { doc } = state;
  const edge = doc.lineAt(to);
  const separator = edge.number < doc.lines ? doc.line(edge.number + 1) : undefined;
  const following = separator && separator.number < doc.lines ? doc.line(separator.number + 1) : undefined;
  let anchor = edge.to, before = "\n\n", after = "";
  if (separator?.text.trim()) {
    // Keep the caret at the table boundary when the next block has no separating blank line.
    after = "\n";
  } else if (separator) {
    const nextBlock = following && syntaxTree(state).topNode.childAfter(following.from);
    if (following && (!following.text.trim() || (!atEdge && nextBlock?.name === "Paragraph"))) {
      anchor = following.from;
      before = "";
    } else {
      anchor = separator.to;
      before = "\n";
      if (following) after = "\n";
    }
  }
  const placement = before || after ? { anchor, before, after } : null;
  view.dispatch({ selection: { anchor }, effects: setTableEdgeInput.of(placement), scrollIntoView: !placement });
  return placement;
}

function tableBeforeGap(view: CodeMirrorView, line: HTMLElement | null) {
  if (!line) return null;
  const codeStart = line.dataset.codeBlockStart;
  if (codeStart === undefined && !line.matches(".cm-markra-empty-line, .cm-markra-table-separator")) return null;
  const { doc } = view.state;
  const lineNumber = doc.lineAt(codeStart === undefined ? view.posAtDOM(line) : Number(codeStart)).number;
  if (lineNumber <= 1) return null;
  let previous = doc.line(lineNumber - 1);
  if (codeStart !== undefined && !previous.text.trim() && previous.number > 1) previous = doc.line(previous.number - 1);
  return tableNodeAt(view.state, previous.to, -1);
}

const tableEdgeSelection = ViewPlugin.fromClass(class {
  private readonly eventType: "click" | "mousedown";
  private mobileTableTap?: { tableFrom: string; down: boolean; cell: HTMLTableCellElement | null };
  private edgeCaret?: Element;
  private edgeInput?: HTMLTextAreaElement;
  private startCaret?: Element;

  constructor(private readonly view: CodeMirrorView) {
    this.eventType = view.dom.ownerDocument.documentElement.hasAttribute("data-mobile") ? "click" : "mousedown";
    view.contentDOM.addEventListener(this.eventType, this.selectProse, true);
    if (this.eventType === "click") view.contentDOM.addEventListener("mousedown", this.rememberTableTap, true);
  }

  private rememberTableTap = (event: MouseEvent) => {
    const target = event.target;
    this.mobileTableTap = undefined;
    if (!(target instanceof Element)) return;
    const wrapper = target.closest(".cm-line")?.querySelector<HTMLElement>(".qingyu-table-controls");
    const tableFrom = wrapper?.dataset.tableFrom;
    const rect = wrapper?.querySelector(".cm-markra-table")?.getBoundingClientRect();
    // On iOS, focusing the editor can move the table before the click arrives.
    if (tableFrom !== undefined && rect) {
      this.mobileTableTap = {
        tableFrom, down: event.clientY >= rect.top + rect.height / 2,
        cell: target.closest<HTMLTableCellElement>(".cm-markra-table :is(td:not([data-table-check]), th)"),
      };
    }
  };

  private selectProse = (event: MouseEvent) => {
    const { view } = this;
    const tap = this.mobileTableTap;
    this.mobileTableTap = undefined;
    this.clearEdgeCaret();
    const mobile = this.eventType === "click";
    if (event.button !== 0 || !event.detail || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey || view.state.readOnly || view.compositionStarted) return;
    const selection = view.dom.ownerDocument.getSelection();
    if (mobile && (event.detail !== 1 || selection?.isCollapsed === false)) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const tappedCell = tap?.cell;
    if (tappedCell?.isConnected && !tappedCell.contains(target)) {
      event.preventDefault();
      event.stopPropagation();
      if (!focusedTable(view)?.contains(tappedCell) || !tappedCell.contains(selection?.anchorNode ?? null)) {
        placeVisualTableCellCaret(tappedCell, tableCellCaretOffset(tappedCell));
      }
      return;
    }
    if (target.closest(".cm-markra-table, .markra-table-control")) return;
    const line = target.closest<HTMLElement>(".cm-line");
    const wrapper = line?.querySelector<HTMLElement>(".qingyu-table-controls");
    const grid = wrapper?.querySelector(".cm-markra-table");
    const rect = grid?.getBoundingClientRect();
    const onRight = grid && event.clientX >= grid.parentElement!.getBoundingClientRect().right;
    if (rect && !mobile && !onRight && event.clientY < rect.bottom) return;
    const node = wrapper ? tableNodeAt(view.state, Number(wrapper.dataset.tableFrom), 1) : tableBeforeGap(view, line);
    if (!node) return;
    // WebKit can put the caret in hidden table or fence source when clicking the surrounding space.
    event.preventDefault();
    const down = !mobile || !rect || (tap && tap.tableFrom === wrapper?.dataset.tableFrom
      ? tap.down
      : event.clientY >= rect.top + rect.height / 2);
    if (!down) { leaveTable(view, node, false); return; }
    this.selectBelow(node, Boolean(!mobile && onRight && rect && event.clientY >= rect.top && event.clientY < rect.bottom));
  };

  private selectBelow(node: { from: number; to: number }, right: boolean, atEdge = false, armed = false) {
    const { view } = this;
    const placement = selectAfterTable(view, node.to, right || atEdge);
    const table = renderedTableAt(view, node.from);
    const scroller = table?.parentElement;
    if (scroller && (placement || right)) {
      this.edgeCaret = scroller;
      scroller.setAttribute("data-table-edge-caret", "right");
    }
    if (placement && table) this.focusEdgeInput(table, node, placement, armed);
    else view.focus();
  }

  backspaceAfterTable() {
    const { view } = this;
    const { doc, selection, readOnly } = view.state;
    if (readOnly || view.compositionStarted || !selection.main.empty || selection.ranges.length !== 1) return false;
    const line = doc.lineAt(selection.main.head);
    if (!line.length || line.from !== selection.main.head || line.number < 3 || doc.line(line.number - 1).text.trim()) return false;
    const table = tableNodeAt(view.state, doc.line(line.number - 2).to, -1);
    if (!table || !renderedTableAt(view, table.from)) return false;
    // Removing this break could turn the following paragraph into another GFM row.
    // Like Pinto, the first press marks the table and the next one removes it.
    this.selectBelow(table, true, true, true);
    return true;
  }

  private focusEdgeInput(table: HTMLTableElement, node: { from: number; to: number }, placement: TableEdgeInput, armed = false) {
    const { view } = this;
    const wrapper = table.closest(".cm-markra-table-wrap");
    if (!wrapper) return;
    const scroller = table.closest<HTMLElement>(".markra-table-scroll");
    const arm = () => { armed = true; scroller?.setAttribute("data-table-armed", ""); };
    if (armed) arm();
    // The same keystroke can also arrive as beforeinput; skip it until this task ends.
    let handledKey = false;
    // Backspace at the table's edge: mark it first, then delete the whole table.
    const backspace = () => {
      if (!armed) { arm(); return; }
      const preview = tablePreviewAt(view.state, node.from, node.to);
      this.clearEdgeCaret();
      if (preview) deleteTable(view, preview);
      else view.focus();
    };
    const input = table.ownerDocument.createElement("textarea");
    input.className = "markra-table-edge-input";
    input.ariaLabel = translatedLabel('在表格后输入');
    input.tabIndex = -1;
    input.rows = 1;
    input.spellcheck = false;
    const doc = view.state.doc;
    let composing = false;
    const commit = (text: string, enter = false) => {
      if (this.edgeInput !== input || !input.isConnected || view.state.doc !== doc ||
        view.state.field(tableEdgeInput) !== placement || view.state.readOnly) return;
      this.clearEdgeCaret();
      const { anchor, before, after } = placement;
      const inserted = text.replace(/\r\n?/gu, "\n");
      view.dispatch({
        // An empty paragraph already separates the next block.
        changes: { from: anchor, insert: before + inserted + (enter ? "" : after) },
        selection: { anchor: anchor + before.length + inserted.length },
        annotations: isolateHistory.of(enter ? "full" : "before"),
        userEvent: "input.tableBoundary",
        scrollIntoView: true,
      });
      view.focus();
    };
    input.addEventListener("input", event => {
      const { inputType, isComposing } = event as InputEvent;
      if (composing || isComposing) return;
      // Software keyboards can insert a newline without a usable keydown.
      if (inputType === "insertLineBreak" || inputType === "insertParagraph") commit("", true);
      else if (input.value) commit(input.value);
    });
    // Android soft keyboards often send Backspace only as beforeinput, without a usable keydown.
    input.addEventListener("beforeinput", event => {
      if (composing || event.inputType !== "deleteContentBackward" || input.value) return;
      event.preventDefault();
      if (!handledKey) backspace();
    });
    input.addEventListener("compositionstart", () => { composing = true; });
    input.addEventListener("compositionend", event => {
      composing = false;
      if (event.data) commit(event.data);
      else input.value = "";
    });
    input.onpaste = event => {
      const text = event.clipboardData?.getData("text/plain");
      if (composing || !text) return;
      event.preventDefault();
      event.stopPropagation();
      commit(restoreCopiedTableHeader(text));
    };
    input.onkeydown = event => {
      if (composing || event.isComposing) return;
      const history = tableHistoryKey(event);
      if (history) {
        event.preventDefault();
        event.stopPropagation();
        if ((history === "redo" ? redo : undo)(view)) view.focus();
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const { key } = event;
      if (key === "Enter") { event.preventDefault(); commit("", true); return; }
      if (key === "Backspace" && !input.value) {
        event.preventDefault();
        handledKey = true;
        setTimeout(() => { handledKey = false; });
        backspace();
        return;
      }
      if (event.shiftKey) return;
      if (key === "ArrowLeft" || key === "ArrowUp" || key === "Escape") {
        event.preventDefault();
        this.clearEdgeCaret();
        const row = table.rows.length - 1;
        restoreTableCaret(view, node.from, row, key === "ArrowUp" ? 0 : table.rows[row].cells.length - 1);
      } else if (key === "ArrowRight" || key === "ArrowDown") {
        event.preventDefault();
        const line = doc.lineAt(node.to);
        let next = line.number + 1;
        while (next <= doc.lines && !doc.line(next).text.trim()) next++;
        if (next > doc.lines) return;
        this.clearEdgeCaret();
        view.dispatch({ selection: { anchor: doc.line(next).from }, scrollIntoView: true });
        view.focus();
      }
    };
    input.onblur = () => { if (this.edgeInput === input) this.clearEdgeCaret(); };
    // The table's last Markdown row is display:none. Keep native typing and IME
    // in a real control at the border until there is a real paragraph to edit.
    this.edgeInput = input;
    wrapper.append(input);
    input.focus({ preventScroll: true });
  }

  private clearEdgeCaret() {
    const input = this.edgeInput;
    this.edgeInput = undefined;
    input?.closest(".cm-markra-table-wrap")?.querySelector(".markra-table-scroll")?.removeAttribute("data-table-armed");
    input?.remove();
    this.edgeCaret?.removeAttribute("data-table-edge-caret");
    this.edgeCaret = undefined;
  }

  // 光标停在表格开头时，CodeMirror 会画一条和整行（含上下留白）一样高的竖线，
  // 还压在表格左边框上。换成和右边缘一样、与表格等高的光标。
  private syncStartCaret() {
    const { view } = this;
    const { selection } = view.state;
    const head = selection.main.head;
    const node = view.hasFocus && !view.compositionStarted && selection.ranges.length === 1 && selection.main.empty
      ? tableNodeAt(view.state, head, 1) : null;
    const scroller = node && node.headerFrom === head ? renderedTableAt(view, node.from)?.parentElement : null;
    if (scroller === this.startCaret) return;
    this.startCaret?.removeAttribute("data-table-edge-caret");
    this.startCaret = undefined;
    if (!scroller || scroller.hasAttribute("data-table-edge-caret")) return;
    scroller.setAttribute("data-table-edge-caret", "left");
    this.startCaret = scroller;
  }

  update(update: ViewUpdate) {
    if (update.docChanged || update.selectionSet || update.focusChanged) {
      queueMicrotask(() => { if (this.view.dom.isConnected) this.syncStartCaret(); });
    }
    if (this.edgeCaret && (update.docChanged || !this.edgeInput && (update.selectionSet ||
      update.focusChanged && !update.view.hasFocus))) this.clearEdgeCaret();
    const { state, view } = update;
    const { doc, selection, readOnly } = state;
    if (!(update.selectionSet || update.docChanged || update.focusChanged) || readOnly ||
      !view.hasFocus || view.compositionStarted || !selection.main.empty || selection.ranges.length !== 1 ||
      doc.lineAt(selection.main.head).text.trim()) return;
    const deletedLine = update.docChanged && update.transactions.some(transaction => transaction.isUserEvent("delete.backward"));
    // 撤销删表后回到删除前的右边缘光标，不进单元格：手机上进单元格会弹控件、触发输入法补行。
    const undone = update.docChanged && update.transactions.some(transaction => transaction.isUserEvent("undo"));
    // Backspace, undo and arrow keys can land on the Markdown separator. Once the DOM
    // settles, give it the same input target as a click in the compact gap.
    queueMicrotask(() => {
      if (this.edgeInput || !view.dom.isConnected || !view.hasFocus || view.compositionStarted ||
        view.state.doc !== doc || !view.state.selection.eq(selection)) return;
      const { node } = view.domAtPos(selection.main.head);
      const element = node instanceof Element ? node : node.parentElement;
      const line = element?.closest<HTMLElement>(".cm-markra-table-separator");
      if (!line) return;
      const beforeFence = line.nextElementSibling?.classList.contains("cm-markra-code-header-line");
      const table = tableBeforeGap(view, line);
      const grid = table && renderedTableAt(view, table.from);
      if (!table || !grid) return;
      if (undone) this.selectBelow(table, true);
      else if (deletedLine || beforeFence) this.selectBelow(table, deletedLine);
      else {
        const row = grid.rows.length - 1;
        restoreTableCaret(view, table.from, row, grid.rows[row].cells.length - 1);
      }
    });
  }

  destroy() {
    this.clearEdgeCaret();
    this.startCaret?.removeAttribute("data-table-edge-caret");
    this.view.contentDOM.removeEventListener(this.eventType, this.selectProse, true);
    if (this.eventType === "click") this.view.contentDOM.removeEventListener("mousedown", this.rememberTableTap, true);
  }
});

export function focusVisualTableCell(
  view: CodeMirrorView,
  tableFrom: number,
  rowIndex: number,
  columnIndex: number,
  header: boolean,
  caretOffset: number,
  lineBreakIndex?: number,
) {
  queueMicrotask(() => {
    if (!view.dom.isConnected) return;
    const cell = view.dom.querySelector<HTMLTableCellElement>(
      `.cm-markra-table-wrap[data-table-from="${tableFrom}"] ` +
        `[data-table-row="${rowIndex}"][data-table-column="${columnIndex}"]` +
        `[data-table-header="${String(header)}"]`,
    );
    if (!cell) return;

    const lineBreak = lineBreakIndex === undefined
      ? undefined
      : cell.querySelectorAll<HTMLBRElement>(
          'br[data-markra-source-break="true"]',
        )[lineBreakIndex];
    if (!lineBreak) {
      placeVisualTableCellCaret(cell, caretOffset);
      return;
    }

    cell.focus();
    const caretHost = createVisualTableCaretHost(cell.ownerDocument);
    lineBreak.after(caretHost.host);
    const selection = cell.ownerDocument.getSelection();
    if (!selection) return;
    const range = cell.ownerDocument.createRange();
    range.setStart(caretHost.text, TABLE_CARET_PLACEHOLDER.length);
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
    revealTableCellHorizontally(cell);
  });
}

function insertVisualTableCellLineBreak(
  view: CodeMirrorView,
  preview: TablePreview,
  cell: HTMLTableCellElement,
  rowIndex: number,
  columnIndex: number,
  header: boolean,
) {
  const selection = cell.ownerDocument.getSelection();
  let range = selection?.rangeCount ? selection.getRangeAt(0) : null;
  if (!range || !cell.contains(range.commonAncestorContainer)) {
    placeVisualTableCellCaret(cell, tableCellCaretOffset(cell));
    range = selection?.rangeCount ? selection.getRangeAt(0) : null;
  }
  if (!selection || !range) return;

  range.deleteContents();
  const lineBreak = cell.ownerDocument.createElement("br");
  lineBreak.dataset.markraSourceBreak = "true";
  range.insertNode(lineBreak);
  const caretHost = createVisualTableCaretHost(cell.ownerDocument);
  lineBreak.after(caretHost.host);
  range.setStart(caretHost.text, TABLE_CARET_PLACEHOLDER.length);
  range.collapse(true);
  selection.removeAllRanges();
  selection.addRange(range);

  const lineBreakIndex = Array.from(
    cell.querySelectorAll<HTMLBRElement>(
      'br[data-markra-source-break="true"]',
    ),
  ).indexOf(lineBreak);
  const changed = replaceVisualTableCell(
    view,
    preview,
    rowIndex,
    columnIndex,
    header,
    visualTableCellSource(cell),
  );
  if (changed) {
    focusVisualTableCell(
      view,
      preview.from,
      rowIndex,
      columnIndex,
      header,
      0,
      lineBreakIndex,
    );
  }
}

function handleVisualTableCellEnter(
  view: CodeMirrorView,
  preview: TablePreview,
  cell: HTMLTableCellElement,
  event: KeyboardEvent,
  rowIndex: number,
  columnIndex: number,
  header: boolean,
  images: ImagePreviewPluginOptions | undefined,
  links: LinksPluginOptions | undefined,
) {
  event.preventDefault();
  event.stopPropagation();
  if (event.shiftKey) {
    insertVisualTableCellLineBreak(
      view,
      preview,
      cell,
      rowIndex,
      columnIndex,
      header,
    );
    return;
  }

  const session = tableEditingSessions.get(view);
  if (!session?.inlineSourceVisible) {
    const math = cell.querySelector<HTMLElement>(
      "[data-markra-math-markdown]",
    );
    const editMathSource = visualTableMathSourceEditors.get(cell);
    const markdown = math?.dataset.markraMathMarkdown;
    if (math && editMathSource && markdown) {
      editMathSource(math, markdown);
      return;
    }
  }
  finishVisualTableInlineSource(view, cell, images, links);
  const nextRow = rowIndex + 1;
  if (nextRow === preview.rows.length) addRow(view, preview, nextRow, columnIndex);
  else {
    const next = cell.closest("table")?.querySelector<HTMLTableCellElement>(
      `td[data-table-row="${nextRow}"][data-table-column="${columnIndex}"]`,
    );
    if (next) placeVisualTableCellCaret(next, tableCellCaretOffset(next));
  }
}

function closestSourceIndex(source: string, value: string, expected: number) {
  let closest = -1;
  let closestDistance = Number.POSITIVE_INFINITY;
  let cursor = source.indexOf(value);
  while (cursor >= 0) {
    const distance = Math.abs(cursor - expected);
    if (distance < closestDistance) {
      closest = cursor;
      closestDistance = distance;
    }
    cursor = source.indexOf(value, cursor + Math.max(1, value.length));
  }
  return closest;
}

function updateVisualTableMathSourceRange(
  view: CodeMirrorView,
  cell: HTMLTableCellElement,
  source: string,
) {
  const session = tableEditingSessions.get(view);
  if (
    !session?.inlineSourceVisible ||
    session.mathFrom === undefined ||
    session.mathTo === undefined
  ) {
    return;
  }
  const element = cell.querySelector<HTMLElement>(
    "[data-markra-table-math-source]",
  );
  const value = element?.textContent ?? "";
  if (!value) {
    tableEditingSessions.delete(view);
    return;
  }
  const from = closestSourceIndex(source, value, session.mathFrom);
  if (from < 0) return;
  tableEditingSessions.set(view, {
    ...session,
    mathFrom: from,
    mathTo: from + value.length,
  });
}

function syncVisualTableCell(
  view: CodeMirrorView,
  preview: TablePreview,
  cell: HTMLTableCellElement,
  typing = true,
) {
  if (view.state.readOnly) return;

  const rowIndex = Number(cell.dataset.tableRow);
  const columnIndex = Number(cell.dataset.tableColumn);
  const header = cell.dataset.tableHeader === "true";
  if (!Number.isInteger(rowIndex) || !Number.isInteger(columnIndex)) return;

  const caretOffset = tableCellCaretOffset(cell);
  const source = visualTableCellSource(cell);
  updateVisualTableMathSourceRange(view, cell, source);
  const changed = replaceVisualTableCell(
    view,
    preview,
    rowIndex,
    columnIndex,
    header,
    source,
    typing,
  );
  const selectionNode = cell.ownerDocument.getSelection()?.anchorNode;
  if (changed || !selectionNode || !cell.contains(selectionNode)) {
    focusVisualTableCell(
      view,
      preview.from,
      rowIndex,
      columnIndex,
      header,
      caretOffset,
    );
  }
}

function revealVisualTableInlineSource(
  cell: HTMLTableCellElement,
  element: HTMLElement,
  markdown: string | undefined,
) {
  if (!markdown) return false;
  const source = cell.ownerDocument.createTextNode(markdown);
  const icon = element.nextElementSibling;
  if (icon?.classList.contains("markra-live-link-icon")) icon.remove();
  element.replaceWith(source);
  cell.focus();
  const selection = cell.ownerDocument.getSelection();
  if (selection) {
    const range = cell.ownerDocument.createRange();
    range.setStart(source, Math.min(1, markdown.length));
    range.collapse(true);
    selection.removeAllRanges();
    selection.addRange(range);
  }
  return true;
}

function renderVisualTableCell(
  view: CodeMirrorView,
  cell: HTMLTableCellElement,
  source: string,
  images: ImagePreviewPluginOptions | undefined,
  links: LinksPluginOptions | undefined,
  revealedMathRange?: MarkraSourceRange,
  highlights?: InlineMarkdownRenderOptions["highlights"],
) {
  const imageResolver = images?.resolveSource;
  const linkResolver = links?.resolveTarget;
  renderInlineMarkdown(cell, source, {
    highlights,
    wikiLinkExists: view.state.facet(wikiLinkAvailability),
    ...(revealedMathRange ? { revealedMathRange } : {}),
    ...(imageResolver
      ? {
          resolveImageSource: (details: InlineMarkdownImageDetails) =>
            imageResolver({ ...details, state: view.state, view }),
        }
      : {}),
    ...(linkResolver
      ? {
          resolveLinkTarget: (linkSource: string) =>
            linkResolver({ source: linkSource, state: view.state, view }),
        }
      : {}),
  });
}

function finishVisualTableInlineSource(
  view: CodeMirrorView,
  cell: HTMLTableCellElement,
  images: ImagePreviewPluginOptions | undefined,
  links: LinksPluginOptions | undefined,
) {
  const session = tableEditingSessions.get(view);
  tableEditingSessions.delete(view);
  if (session?.inlineSourceVisible && cell.isConnected) {
    renderVisualTableCell(
      view,
      cell,
      visualTableCellSource(cell),
      images,
      links,
    );
    cell.closest("table")?.dispatchEvent(new Event(tableDisplayChanged));
  }
}

function renderTableCheckbox(
  view: CodeMirrorView,
  cell: HTMLTableCellElement,
  getPreview: () => TablePreview,
  row: number,
  column: number,
  source: string,
  options: { yes: string; no: string },
) {
  const document = cell.ownerDocument;
  const button = document.createElement("button");
  const checked = source.toLowerCase() === options.yes.toLowerCase();
  const showsTick = !source || checked || source.toLowerCase() === options.no.toLowerCase();
  cell.dataset.tableCheck = "";
  button.type = "button";
  button.contentEditable = "false";
  button.className = "cm-markra-table-check";
  button.role = "checkbox";
  button.ariaChecked = String(checked);
  button.ariaLabel = getPreview().header[column]?.source || `第 ${column + 1} 列`;
  button.disabled = view.state.readOnly;
  if (showsTick) {
    const tick = document.createElement("span");
    tick.className = "cm-markra-table-tick";
    tick.ariaHidden = "true";
    button.append(tick);
  } else button.textContent = source;
  button.onmousedown = event => { if (event.button === 0 && !event.ctrlKey) event.preventDefault(); };
  button.onkeydown = event => { if (event.key === " " || event.key === "Enter") event.stopPropagation(); };
  button.onclick = event => {
    event.stopPropagation();
    if (view.state.readOnly || view.compositionStarted || !cell.isConnected) return;
    const grid = cell.closest("table");
    const active = grid && focusedTable(view) === grid ? activeVisualTableCell(view, grid) : null;
    const offset = active ? tableCellCaretOffset(active) : 0;
    const preview = getPreview();
    if (replaceVisualTableCell(view, preview, row, column, false, checked ? options.no : options.yes, false) && active) {
      focusVisualTableCell(view, preview.from, Number(active.dataset.tableRow), active.cellIndex, active.tagName === "TH", offset);
    }
  };
  cell.append(button);
}

function appendCell(
  view: CodeMirrorView,
  row: HTMLTableRowElement,
  cellPreview: TableCellPreview,
  alignment: CodeMirrorTableAlignment,
  header: boolean,
  getPreview: () => TablePreview,
  rowIndex: number,
  columnIndex: number,
  images: ImagePreviewPluginOptions | undefined,
  links: LinksPluginOptions | undefined,
) {
  // Cell DOM survives position-only updates, so events must read the shared preview.
  const cell = row.ownerDocument.createElement(header ? "th" : "td");
  cell.dataset.tableColumn = String(columnIndex);
  cell.dataset.tableHeader = String(header);
  cell.dataset.tableRow = String(rowIndex);
  if (alignment) cell.style.textAlign = alignment;
  const options = !header && checkboxOptions(getPreview().types?.[columnIndex]);
  if (options) {
    renderTableCheckbox(view, cell, getPreview, rowIndex, columnIndex, cellPreview.source, options);
    row.append(cell);
    return;
  }
  const highlights = codeMirrorSearchHighlights(
    view.state,
    cellPreview.from,
    cellPreview.to,
  );
  if (highlights.some((highlight) => highlight.active)) {
    // Source-only hits (such as a link URL) still have a visible cell to reveal.
    cell.dataset.markraSearchTarget = "";
  }
  const editMathSource: EditVisualTableMathSource = (element, markdown) => {
    const mathFrom = Number(element.dataset.markraMathFrom);
    const mathTo = Number(element.dataset.markraMathTo);
    const previousSession = tableEditingSessions.get(view);
    const originalSource = visualTableCellSource(cell);
    tableEditingSessions.set(view, {
      column: columnIndex,
      header,
      inlineSourceVisible: true,
      ...(Number.isInteger(mathFrom) && Number.isInteger(mathTo)
        ? { mathFrom, mathTo }
        : {}),
      row: rowIndex,
      tableFrom: getPreview().from,
    });
    let handled = false;
    if (Number.isInteger(mathFrom) && Number.isInteger(mathTo)) {
      renderVisualTableCell(
        view,
        cell,
        originalSource,
        images,
        links,
        { from: mathFrom, to: mathTo },
      );
      const source = cell.querySelector<HTMLElement>(
        "[data-markra-table-math-source]",
      );
      const text = source?.firstChild;
      if (source && text) {
        cell.focus();
        const selection = cell.ownerDocument.getSelection();
        if (selection) {
          const range = cell.ownerDocument.createRange();
          range.setStart(text, Math.min(1, text.textContent?.length ?? 0));
          range.collapse(true);
          selection.removeAllRanges();
          selection.addRange(range);
        }
        handled = true;
      }
    } else {
      handled = revealVisualTableInlineSource(cell, element, markdown);
    }
    if (handled) return;
    if (previousSession) tableEditingSessions.set(view, previousSession);
    else tableEditingSessions.delete(view);
  };
  visualTableMathSourceEditors.set(cell, editMathSource);
  const currentSession = tableEditingSessions.get(view);
  const keepInlineSourceVisible =
    currentSession?.tableFrom === getPreview().from &&
    currentSession.row === rowIndex &&
    currentSession.column === columnIndex &&
    currentSession.header === header &&
    currentSession.inlineSourceVisible;
  const revealedMathRange = keepInlineSourceVisible &&
      currentSession.mathFrom !== undefined &&
      currentSession.mathTo !== undefined
    ? { from: currentSession.mathFrom, to: currentSession.mathTo }
    : undefined;
  if (keepInlineSourceVisible && !revealedMathRange) {
    cell.textContent = cellPreview.source;
  } else {
    renderVisualTableCell(
      view,
      cell,
      cellPreview.source,
      images,
      links,
      revealedMathRange,
      highlights,
    );
  }
  cell.tabIndex = view.state.readOnly ? -1 : 0;
  cell.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;
    const target = event.target instanceof Element ? event.target : null;
    const link = target?.closest<HTMLElement>(
      "[data-markra-link-markdown]",
    );
    if (link && cell.contains(link)) {
      const source = link.dataset.markraLinkSource;
      const opens = view.state.readOnly || event.metaKey || event.ctrlKey || (link.classList.contains("cm-markra-wiki-link") && !event.altKey);
      const handled = opens
        ? Boolean(link.matches("a[href]") && source && links && openMarkraLinkSource(view, source, links))
        : revealVisualTableInlineSource(
            cell,
            link,
            link.dataset.markraLinkMarkdown,
          );
      if (handled && !opens) {
        tableEditingSessions.set(view, {
          column: columnIndex,
          header,
          inlineSourceVisible: true,
          row: rowIndex,
          tableFrom: getPreview().from,
        });
      }
      if (handled) {
        event.preventDefault();
        event.stopPropagation();
      }
      return;
    }
    const image = target?.closest<HTMLElement>(
      "[data-markra-image-markdown]",
    );
    if (image && cell.contains(image)) {
      const handled = revealVisualTableInlineSource(
        cell,
        image,
        image.dataset.markraImageMarkdown,
      );
      if (handled) {
        tableEditingSessions.set(view, {
          column: columnIndex,
          header,
          inlineSourceVisible: true,
          row: rowIndex,
          tableFrom: getPreview().from,
        });
        event.preventDefault();
        event.stopPropagation();
      }
      return;
    }
    if (event.ctrlKey) return;
    event.stopPropagation();
  });
  cell.addEventListener("click", event => {
    const target = event.target;
    const link = target instanceof Element && target.closest<HTMLAnchorElement>("a[data-markra-link-source]");
    if (link && cell.contains(link)) {
      // Keyboard and accessibility activation may deliver click without a mousedown.
      const source = link.dataset.markraLinkSource;
      if (event.detail === 0 && links && source && openMarkraLinkSource(view, source, links)) event.stopPropagation();
      return;
    }
    if (!view.state.readOnly && cell.ownerDocument.documentElement.hasAttribute("data-mobile")) {
      if (event.button !== 0 || event.ctrlKey || event.metaKey || view.compositionStarted) return;
      const selection = cell.ownerDocument.getSelection();
      if (!selection?.isCollapsed || cell.contains(selection.anchorNode)) return;
      // WebKit can place the tap's caret in the outer editing host instead of the cell.
      placeVisualTableCellCaret(cell, cell.textContent?.length ?? 0);
    }
  });
  cell.addEventListener("keydown", event => {
    if (event.key !== "Enter" || event.isComposing || view.compositionStarted || !links) return;
    const target = event.target;
    const link = target instanceof Element && target.closest<HTMLAnchorElement>("a[data-markra-link-source]");
    const source = link ? link.dataset.markraLinkSource : undefined;
    if (source && openMarkraLinkSource(view, source, links)) {
      event.preventDefault();
      event.stopPropagation();
    }
  });
  cell.addEventListener("focus", () => {
    const current = tableEditingSessions.get(view);
    if (
      current?.tableFrom === getPreview().from &&
      current.row === rowIndex &&
      current.column === columnIndex &&
      current.header === header
    ) {
      return;
    }
    tableEditingSessions.set(view, {
      column: columnIndex,
      header,
      inlineSourceVisible: false,
      row: rowIndex,
      tableFrom: getPreview().from,
    });
  });
  cell.addEventListener("blur", () => {
    cell.ownerDocument.defaultView?.setTimeout(() => {
      const table = cell.closest<HTMLTableElement>("table");
      const activeElement = cell.ownerDocument.activeElement;
      let activeCell = activeElement instanceof HTMLTableCellElement
        ? activeElement
        : null;
      if (!activeCell && table && activeElement === table) {
        const selectionNode = cell.ownerDocument.getSelection()?.anchorNode;
        const selectionElement = selectionNode instanceof Element
          ? selectionNode
          : selectionNode?.parentElement;
        const selectedCell = selectionElement?.closest<HTMLTableCellElement>(
          "th, td",
        );
        if (selectedCell && table.contains(selectedCell)) {
          activeCell = selectedCell;
        }
      }
      const session = tableEditingSessions.get(view);
      if (
        activeCell instanceof HTMLTableCellElement &&
        activeCell.dataset.tableRow === String(rowIndex) &&
        activeCell.dataset.tableColumn === String(columnIndex) &&
        activeCell.dataset.tableHeader === String(header)
      ) {
        return;
      }
      if (
        session?.tableFrom === getPreview().from &&
        session.row === rowIndex &&
        session.column === columnIndex &&
        session.header === header
      ) {
        finishVisualTableInlineSource(view, cell, images, links);
      }
    }, 0);
  });
  row.append(cell);
}

interface TableWidgetRuntime {
  controls: ReturnType<typeof createTableControls> | ReturnType<typeof createCellTableControls> | null;
  disposeLayout?: () => void;
  inlineSourceDocument: Document | null;
  inlineSourceMouseDownHandler: ((event: MouseEvent) => void) | null;
  preview: TablePreview;
}

function tablePreviewContentKey(preview: TablePreview) {
  return JSON.stringify({
    alignments: preview.alignments,
    types: preview.types,
    header: preview.header.map((cell) => cell.source),
    rows: preview.rows.map((row) => row.map((cell) => cell.source)),
  });
}

class TableWidget extends WidgetType {
  private readonly contentKey: string;
  private readonly labelsKey: string;
  private runtime: TableWidgetRuntime;

  constructor(
    preview: TablePreview,
    readonly labels: TablePreviewLabels,
    readonly defaultWidthMode: CodeMirrorTableWidthMode,
    readonly getDocumentKey: () => string | null | undefined,
    readonly images: ImagePreviewPluginOptions | undefined,
    readonly links: LinksPluginOptions | undefined,
    readonly searchKey: string,
  ) {
    super();
    this.contentKey = tablePreviewContentKey(preview);
    this.labelsKey = JSON.stringify(labels);
    this.runtime = {
      controls: null,
      inlineSourceDocument: null,
      inlineSourceMouseDownHandler: null,
      preview,
    };
  }

  private get preview() {
    return this.runtime.preview;
  }

  private hasSameContent(other: TableWidget) {
    return (
      this.contentKey === other.contentKey &&
      this.defaultWidthMode === other.defaultWidthMode &&
      this.getDocumentKey === other.getDocumentKey &&
      this.images === other.images &&
      this.links === other.links &&
      this.labelsKey === other.labelsKey &&
      this.searchKey === other.searchKey
    );
  }

  eq(other: TableWidget) {
    if (!this.hasSameContent(other)) return false;
    if (
      this.preview.from !== other.preview.from ||
      this.preview.to !== other.preview.to
    ) {
      return false;
    }

    const nextPreview = other.preview;
    other.runtime = this.runtime;
    this.runtime.preview = nextPreview;
    return true;
  }

  updateDOM(dom: HTMLElement, _view: CodeMirrorView, from: this) {
    if (!this.hasSameContent(from)) return false;

    const nextPreview = this.preview;
    // Reused DOM listeners close over the previous widget, so both widget
    // generations must share the latest document positions and popup state.
    this.runtime = from.runtime;
    this.runtime.preview = nextPreview;
    dom.dataset.tableFrom = String(nextPreview.from);
    return true;
  }

  ignoreEvent() {
    return true;
  }

  destroy() {
    this.runtime.controls?.destroy();
    this.runtime.controls = null;
    this.runtime.disposeLayout?.();
    if (
      this.runtime.inlineSourceDocument &&
      this.runtime.inlineSourceMouseDownHandler
    ) {
      this.runtime.inlineSourceDocument.removeEventListener(
        "mousedown",
        this.runtime.inlineSourceMouseDownHandler,
        true,
      );
    }
    this.runtime.inlineSourceDocument = null;
    this.runtime.inlineSourceMouseDownHandler = null;
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const wrapper = document.createElement("span");
    const tableScroll = document.createElement("span");
    const table = document.createElement("table");
    const head = document.createElement("thead");
    const headRow = document.createElement("tr");
    const body = document.createElement("tbody");
    const activeAlignment = normalizedTableAlignment(this.preview);
    const tableAlignment = activeAlignment ?? "left";
    const widthMode = readStoredWidthMode(
      document,
      this.getDocumentKey(),
      this.defaultWidthMode,
    );
    let composing = false;

    wrapper.className =
      "cm-markra-table-wrap tableWrapper markra-table-controls-wrapper";
    wrapper.dataset.tableFrom = String(this.preview.from);
    wrapper.dataset.tableAlignment = tableAlignment;
    wrapper.dataset.widthMode = widthMode;
    const activateMathSource = (eventTarget: EventTarget | null) => {
      if (view.state.readOnly) return false;
      const target = eventTarget instanceof Element ? eventTarget : null;
      const math = target?.closest<HTMLElement>(
        "[data-markra-math-markdown]",
      );
      const cell = math?.closest<HTMLTableCellElement>("th, td");
      const editMathSource = cell
        ? visualTableMathSourceEditors.get(cell)
        : undefined;
      const markdown = math?.dataset.markraMathMarkdown;
      if (
        !math ||
        !cell ||
        !table.contains(cell) ||
        !editMathSource ||
        !markdown
      ) {
        return false;
      }
      editMathSource(math, markdown);
      return true;
    };
    wrapper.addEventListener("mousedown", (event) => {
      if (event.button !== 0 || !activateMathSource(event.target)) return;
      // CodeMirror owns the widget boundary and can consume target/bubble
      // events, so formula activation must happen before editor selection work.
      event.preventDefault();
      event.stopPropagation();
    }, true);
    wrapper.addEventListener("click", (event) => {
      if (!activateMathSource(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
    }, true);
    const inlineSourceMouseDownHandler = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const target = event.target instanceof Node ? event.target : null;
      const targetElement = target instanceof Element ? target : target?.parentElement;
      if (tableLinePicks.has(table) && target && view.dom.contains(target)) {
        const control = targetElement?.closest(".markra-table-control");
        if (!control || !wrapper.contains(control)) setTableLinePick(table);
      }
      const session = tableEditingSessions.get(view);
      if (!session || session.tableFrom !== this.preview.from) return;
      const cell = table.querySelector<HTMLTableCellElement>(
        `[data-table-row="${session.row}"]` +
          `[data-table-column="${session.column}"]` +
          `[data-table-header="${String(session.header)}"]`,
      );
      if (!cell) return;
      const activeSource = cell.querySelector<HTMLElement>(
        "[data-markra-table-math-source]",
      );
      if (target && activeSource?.contains(target)) return;

      const nextMath = targetElement?.closest<HTMLElement>(
        "[data-markra-math-markdown]",
      );
      const nextMathFrom = nextMath?.dataset.markraMathFrom;
      const nextMathTo = nextMath?.dataset.markraMathTo;
      const nextMathInCell = Boolean(nextMath && cell.contains(nextMath));
      if (nextMathInCell) {
        event.preventDefault();
        event.stopPropagation();
      }
      finishVisualTableInlineSource(view, cell, this.images, this.links);
      if (!nextMathInCell || !nextMathFrom || !nextMathTo) return;

      const replacement = cell.querySelector<HTMLElement>(
        `[data-markra-math-from="${nextMathFrom}"]` +
          `[data-markra-math-to="${nextMathTo}"]`,
      );
      const editMathSource = visualTableMathSourceEditors.get(cell);
      const markdown = replacement?.dataset.markraMathMarkdown;
      if (replacement && editMathSource && markdown) {
        editMathSource(replacement, markdown);
      }
    };
    this.runtime.inlineSourceDocument = document;
    this.runtime.inlineSourceMouseDownHandler = inlineSourceMouseDownHandler;
    document.addEventListener("mousedown", inlineSourceMouseDownHandler, true);
    tableScroll.className = "markra-table-scroll";
    tableScroll.dataset.tableAlignment = tableAlignment;
    table.className = "cm-markra-table";
    table.toggleAttribute("data-table-empty-column", this.preview.header.some((cell, column) =>
      !cell.source.trim() && this.preview.rows.every(row => !row[column]?.source.trim())
    ));
    // Individual contenteditable cells trap native ranges inside one cell.
    // One shared editing host lets drag selections cross the complete table.
    table.setAttribute("contenteditable", String(!view.state.readOnly));
    table.dataset.tableAlignment = tableAlignment;
    table.dataset.widthMode = widthMode;
    table.classList.toggle("markra-table-width-auto", widthMode === "auto");
    wrapper.addEventListener("focusout", (event) => {
      const next = event.relatedTarget;
      if (next instanceof Element && !wrapper.contains(next) && !next.closest('[role="menu"]')) setTableLinePick(table);
    });
    const editHistory = (event: Event, forward: boolean) => {
      event.preventDefault();
      event.stopPropagation();
      const cell = activeVisualTableCell(view, table);
      const row = Number(cell?.dataset.tableRow) + 1;
      const column = Number(cell?.dataset.tableColumn);
      const from = this.preview.from;
      const pick = tableLinePicks.get(table);
      if ((forward ? redo : undo)(view)) restoreTableCaret(view, from, row, column, pick);
    };
    const clearPickedLine = (event: Event) => {
      const pick = tableLinePicks.get(table);
      if (!pick || view.state.readOnly) return false;
      event.preventDefault();
      event.stopPropagation();
      const cell = activeVisualTableCell(view, table);
      const values = tableValues(this.preview);
      const rows = [values.header, ...values.rows];
      const selected = pick.axis === "row" ? rows[pick.index] : rows.map(row => row[pick.index]);
      if (selected?.some(Boolean)) {
        if (pick.axis === "row") selected.fill("");
        else for (const row of rows) row[pick.index] = "";
        const from = this.preview.from;
        if (replaceTable(view, this.preview, values.header, values.rows, values.alignments)) {
          restoreTableCaret(view, from, Number(cell?.dataset.tableRow) + 1, Number(cell?.dataset.tableColumn), pick);
        }
      }
      return true;
    };
    table.addEventListener("keydown", (event) => {
      if (view.state.readOnly || composing || event.isComposing || event.keyCode === 229 || view.compositionStarted) return;
      const key = event.key.toLowerCase();
      const history = tableHistoryKey(event);
      if (history) {
        editHistory(event, history === "redo");
        return;
      }
      const plain = !(event.shiftKey || event.metaKey || event.ctrlKey || event.altKey);
      const deleting = key === "backspace" || key === "delete";
      if (plain && deleting && clearPickedLine(event)) return;
      if (deleting || key.startsWith("arrow") || key === "escape" || key === "enter" || key === "tab") setTableLinePick(table);
      const vertical = plain && (key === "arrowdown" || key === "arrowup");
      if (vertical || key === "escape") {
        const cell = activeVisualTableCell(view, table);
        if (!cell) return;
        event.preventDefault();
        event.stopPropagation();
        const down = key !== "arrowup";
        const row = Number(cell.dataset.tableRow) + 1 + (down ? 1 : -1);
        const neighbor = vertical ? table.rows[row]?.cells[cell.cellIndex] : undefined;
        finishVisualTableInlineSource(view, cell, this.images, this.links);
        if (neighbor) {
          placeVisualTableCellCaret(neighbor, tableCellCaretOffset(neighbor));
          this.runtime.controls?.update();
        } else leaveTable(view, this.preview, down);
        return;
      }
      if (event.key === "Enter") {
        const cell = activeVisualTableCell(view, table);
        if (!cell) return;
        const rowIndex = Number(cell.dataset.tableRow);
        const columnIndex = Number(cell.dataset.tableColumn);
        const header = cell.dataset.tableHeader === "true";
        if (!Number.isInteger(rowIndex) || !Number.isInteger(columnIndex)) {
          return;
        }
        // Safari targets keyboard events at the shared contenteditable table
        // instead of the focused cell, so route both targets through one path.
        handleVisualTableCellEnter(view, this.preview, cell, event, rowIndex, columnIndex, header, this.images, this.links);
        return;
      }
      if (
        event.key !== "Tab" ||
        event.altKey ||
        event.ctrlKey ||
        event.metaKey
      ) {
        return;
      }
      const cell = activeVisualTableCell(view, table);
      if (!cell) return;
      const session = tableEditingSessions.get(view);
      if (session?.inlineSourceVisible) {
        finishVisualTableInlineSource(view, cell, this.images, this.links);
      }
      moveVisualTableCellFocus(view, this.preview, table, event.shiftKey, cell);
      this.runtime.controls?.update();
      // WebKit preserves the old editing position during native Tab focus
      // navigation, so move the caret before its next insertion begins.
      event.preventDefault();
      event.stopPropagation();
    });
    // Browsers target editing events at the shared contenteditable table host,
    // so cell-level listeners miss real input even though the cell DOM changes.
    table.addEventListener("beforeinput", (event) => {
      if (composing || (event instanceof InputEvent && event.isComposing)) return;
      if (event instanceof InputEvent && (event.inputType === "historyUndo" || event.inputType === "historyRedo")) {
        editHistory(event, event.inputType === "historyRedo");
        return;
      }
      if (event instanceof InputEvent && (event.inputType === "deleteContentBackward" || event.inputType === "deleteContentForward") && clearPickedLine(event)) return;
      setTableLinePick(table);
      if (
        event instanceof InputEvent &&
        (event.inputType === "insertLineBreak" ||
          event.inputType === "insertParagraph")
      ) {
        // Enter is handled by the controlled keydown path. WebKit may still
        // deliver this native event after keydown was canceled, which would
        // otherwise rewrite the controlled <br> or add a second break.
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (!view.state.readOnly && event instanceof InputEvent && event.inputType.startsWith("delete")) {
        const cleared = clearTableSelection(table);
        if (cleared.length) {
          event.preventDefault(); event.stopPropagation();
          const values = tableValues(this.preview);
          for (const cell of cleared) {
            const row = Number(cell.dataset.tableRow);
            const column = Number(cell.dataset.tableColumn);
            if (cell.dataset.tableHeader === "true") values.header[column] = visualTableCellSource(cell);
            else if (values.rows[row]) values.rows[row]![column] = visualTableCellSource(cell);
          }
          replaceTable(view, this.preview, values.header, values.rows, values.alignments);
          const first = cleared[0]!;
          focusVisualTableCell(view, this.preview.from, Number(first.dataset.tableRow), Number(first.dataset.tableColumn), first.dataset.tableHeader === "true", 0);
          return;
        }
      }
      // WebKit can lift the range from a header cell after compositionstart.
      // Repair only escaped ranges so the IME never inserts outside the cell.
      repairVisualTableCellSelection(view, table);
    });
    table.addEventListener("compositionstart", (event) => {
      event.stopPropagation();
      setTableLinePick(table);
      repairVisualTableCellSelection(view, table);
      stabilizeVisualTableCompositionCaret(view, table);
      composing = true;
    });
    table.addEventListener("compositionend", (event) => {
      event.stopPropagation();
      composing = false;
      const cell = activeVisualTableCell(view, table);
      if (cell) syncVisualTableCell(view, this.preview, cell);
    });
    table.addEventListener("input", (event) => {
      event.stopPropagation();
      // Replacing the widget mid-composition cancels native CJK input, so commit only after compositionend.
      if (composing || (event instanceof InputEvent && event.isComposing)) return;
      const cell = activeVisualTableCell(view, table);
      if (cell) syncVisualTableCell(view, this.preview, cell);
    });

    for (const [index, cell] of this.preview.header.entries()) {
      appendCell(
        view,
        headRow,
        cell,
        this.preview.alignments[index] ?? null,
        true,
        () => this.preview,
        -1,
        index,
        this.images,
        this.links,
      );
    }
    head.append(headRow);

    for (const [rowIndex, values] of this.preview.rows.entries()) {
      const row = document.createElement("tr");
      for (const [columnIndex, cell] of values.entries()) {
        appendCell(
          view,
          row,
          cell,
          this.preview.alignments[columnIndex] ?? null,
          false,
          () => this.preview,
          rowIndex,
          columnIndex,
          this.images,
          this.links,
        );
      }
      body.append(row);
    }

    table.append(head, body);
    tableScroll.append(table);
    wrapper.append(tableScroll);
    if (widthMode === "auto") this.runtime.disposeLayout = layoutTableColumns(view, wrapper, tableScroll, table, Boolean(this.preview.types));
    const show = view.state.facet(tableContextMenu).at(0);
    const openMenu = (cell: HTMLTableCellElement, x: number, y: number, scope: TableMenuScope, bounds?: DOMRectReadOnly, capturedRange?: Range | null) => {
      if (!show || composing || view.compositionStarted) return;
      const row = Number(cell.dataset.tableRow);
      const column = Number(cell.dataset.tableColumn);
      setTableLinePick(table, scope === "row" ? { axis: scope, index: row + 1 }
        : scope === "column" ? { axis: scope, index: column } : undefined);
      const snapshot = view.state.doc;
      // A menu can outlive the widget that opened it when the document changes.
      const current = () => view.state.doc === snapshot && table.isConnected && !composing && !view.compositionStarted;
      const apply = (action: () => boolean) => () => {
        if (current()) action();
      };
      const selection = document.getSelection();
      const selectedRange = capturedRange === undefined ? (selection?.rangeCount ? selection.getRangeAt(0) : null) : capturedRange;
      const range = selectedRange && cell.contains(selectedRange.commonAncestorContainer) ? selectedRange.cloneRange() : null;
      const selected = range?.toString();
      const choice = scope === "cell" && row >= 0 ? checkboxOptions(this.preview.types?.[column]) : undefined;
      const cellValue = this.preview.rows[row]?.[column]?.source ?? "";
      const tableSelection = selectedRange && table.contains(selectedRange.commonAncestorContainer) && selectedRange.toString().trim()
        ? { from: this.preview.from, to: this.preview.to }
        : undefined;
      const restore = () => {
        if (!current()) return;
        if (!choice && range && selection) {
          cell.focus();
          selection.removeAllRanges();
          selection.addRange(range);
        } else placeVisualTableCellCaret(cell, tableCellCaretOffset(cell));
      };
      const insertText = (text: string) => {
        if (!current()) return;
        restore();
        const edit = selection?.rangeCount ? selection.getRangeAt(0) : null;
        if (!edit || !cell.contains(edit.commonAncestorContainer)) return;
        if (!selected) edit.selectNodeContents(cell);
        edit.deleteContents();
        if (text) {
          const value = document.createTextNode(text.replace(/[\r\n\u0085\u2028\u2029]+/gu, " "));
          edit.insertNode(value);
          edit.setStartAfter(value);
          edit.collapse(true);
        }
        syncVisualTableCell(view, this.preview, cell, false);
      };
      show({ x, y, bounds, cell, tableSelection }, {
        choices: choice ? {
          options: choice.options,
          value: cellValue,
          set: value => {
            if (current() && replaceVisualTableCell(view, this.preview, row, column, false, value, false)) {
              focusVisualTableCell(view, this.preview.from, row, column, false, 0);
            }
          },
        } : undefined,
        clipboard: {
          text: selected || visualTableCellSource(cell).replace(/(\\+)\|/gu, (_match, escapes: string) => `${escapes.slice(0, Math.floor(escapes.length / 2))}|`),
          replace: insertText,
        },
        insertRowBefore: apply(() => addRow(view, this.preview, Math.max(row, 0))),
        insertRowAfter: apply(() => addRow(view, this.preview, row + 1)),
        insertColumnBefore: apply(() => addColumn(view, this.preview, column)),
        insertColumnAfter: apply(() => addColumn(view, this.preview, column + 1)),
        deleteRow: this.preview.rows.length ? apply(() => deleteRow(view, this.preview, row, column)) : undefined,
        deleteColumn: this.preview.header.length > 1 ? apply(() => deleteColumn(view, this.preview, column, row)) : undefined,
        deleteTable: apply(() => deleteTable(view, this.preview)),
      }, scope, restore);
    };
    if (show && !view.state.readOnly) {
      tableActions.set(table, {
        openMenu,
        captureInsertion: cell => {
          const state = view.state.doc;
          const selection = document.getSelection();
          const current = selection?.rangeCount ? selection.getRangeAt(0) : null;
          const range = current && cell.contains(current.commonAncestorContainer) ? current.cloneRange() : null;
          return text => {
            if (!range || !selection || state !== view.state.doc || !cell.isConnected || !cell.contains(range.commonAncestorContainer) || composing) return false;
            const node = document.createTextNode(text(range.toString()));
            cell.focus();
            range.deleteContents();
            range.insertNode(node);
            range.setStartAfter(node);
            range.collapse(true);
            selection.removeAllRanges();
            selection.addRange(range);
            syncVisualTableCell(view, this.preview, cell);
            return true;
          };
        },
        format: (cell, action) => {
          if (composing || view.compositionStarted) return;
          if (action === "task") {
            const { preview } = this;
            const column = cell.cellIndex;
            const header = preview.header.map(item => item.source);
            const types = preview.types?.slice() ?? header.map(() => "text");
            types[column] = checkboxOptions(types[column]) ? "text" : "choice Yes|No";
            view.dispatch({
              changes: { from: preview.from, to: preview.headerFrom, insert: serializeTableTypes(header, types) },
              annotations: isolateHistory.of("full"),
            });
            focusVisualTableCell(view, preview.from, -1, column, true, 0);
            return;
          }
          const tag = cellFormatTags[action as CellFormat];
          const selection = document.getSelection();
          const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
          if (!range || !cell.contains(range.commonAncestorContainer) || cell.hasAttribute("data-table-check")) return;
          if (action === "link") {
            const draft = draftTableLink(cell, range);
            const { preview } = this;
            const row = Number(cell.dataset.tableRow), column = Number(cell.dataset.tableColumn);
            const header = cell.dataset.tableHeader === "true";
            if (!draft) return;
            tableEditingSessions.set(view, { tableFrom: preview.from, row, column, header, inlineSourceVisible: true });
            if (replaceVisualTableCell(view, preview, row, column, header, draft.source, false)) {
              focusVisualTableCell(view, preview.from, row, column, header, draft.caret);
            }
            return;
          }
          if (!tag || range.collapsed) return;
          formatCellRange(cell, range, tag);
          syncVisualTableCell(view, this.preview, cell);
        },
      });
      let contextRange: Range | null | undefined;
      table.addEventListener("mousedown", event => {
        contextRange = undefined;
        if (event.button !== 2 && !(event.button === 0 && event.ctrlKey)) return;
        // WebKit can select the clicked word between mouse down and contextmenu.
        const selection = document.getSelection();
        contextRange = selection?.rangeCount && !selection.isCollapsed ? selection.getRangeAt(0).cloneRange() : null;
      }, true);
      table.addEventListener("contextmenu", event => {
        const range = contextRange;
        contextRange = undefined;
        const target = event.target instanceof Element ? event.target : null;
        const cell = target?.closest<HTMLTableCellElement>("th,td");
        if (!cell || target?.closest("a[href]") || composing || view.compositionStarted) return;
        event.preventDefault();
        const active = activeTableCell(table);
        if (active !== cell) placeVisualTableCellCaret(cell, tableCellCaretOffset(cell));
        openMenu(cell, event.clientX, event.clientY, "cell", undefined, range);
      });
      const activeCell = () => activeVisualTableCell(view, table);
      this.runtime.controls = createCellTableControls(wrapper, table, activeCell, openMenu, () => {
        const cell = activeCell();
        if (!composing && cell) addRow(view, this.preview);
      }, () => {
        const cell = activeCell();
        if (!composing && cell) addColumn(view, this.preview);
      });
    } else if (!view.state.readOnly) this.runtime.controls = createTableControls(wrapper, table, {
      labels: this.labels,
      readOnly: () => view.state.readOnly,
      shape: () => ({ columns: this.preview.header.length, rows: this.preview.rows.length + 1 }),
      alignment: () => normalizedTableAlignment(this.preview),
      widthMode: () => wrapper.dataset.widthMode === "even" ? "even" : "auto",
      resize: (columns, rows) => resizeTable(view, this.preview, columns, rows),
      align: alignment => alignTable(view, this.preview, alignment),
      setWidthMode: mode => {
        writeStoredWidthMode(document, this.getDocumentKey(), mode);
        applyWidthModeToTableControls(view, mode);
      },
      addRow: () => addRow(view, this.preview),
      addColumn: () => addColumn(view, this.preview),
      deleteRow: row => deleteRow(view, this.preview, row),
      deleteColumn: column => deleteColumn(view, this.preview, column),
      deleteTable: () => deleteTable(view, this.preview),
      cellPosition: cell => ({ row: Number(cell.dataset.tableRow), column: Number(cell.dataset.tableColumn), header: cell.dataset.tableHeader === "true" }),
      focusEditor: () => view.focus(),
    });
    wrapper.addEventListener("copy", (event) => {
      if (!event.clipboardData) return;
      const selection = document.getSelection();
      if (!selection || selection.isCollapsed) return;
      const selectionInsideTable = selection.anchorNode && selection.focusNode
        && table.contains(selection.anchorNode) && table.contains(selection.focusNode);
      const range = selection.getRangeAt(0);
      const header = table.rows[0]?.cells[0];
      const tail = table.rows[table.rows.length - 1]?.cells;
      const last = tail?.[tail.length - 1];
      const selectedWholeTable = selectionInsideTable && header && last && header !== last
        && header.contains(range.startContainer) && last.contains(range.endContainer)
        && tableCellCaretOffsetAtBoundary(header, range, "start")
        && tableCellCaretOffsetAtBoundary(last, range, "end");
      const text = selectionInsideTable && !selectedWholeTable
        ? selection.toString()
        : view.state.sliceDoc(this.preview.from, this.preview.to);
      event.preventDefault();
      event.clipboardData.setData("text/plain", text);
    });
    return wrapper;
  }
}

function tableCellCaretOffsetAtBoundary(cell: HTMLTableCellElement, selection: Range, edge: "start" | "end") {
  const remainder = cell.ownerDocument.createRange();
  remainder.selectNodeContents(cell);
  if (edge === "start") remainder.setEnd(selection.startContainer, selection.startOffset);
  else remainder.setStart(selection.endContainer, selection.endOffset);
  return !remainder.toString().replaceAll(TABLE_CARET_PLACEHOLDER, "");
}

const tableTheme = EditorView.baseTheme({
  ".cm-markra-table-wrap": {
    display: "block",
    margin: "1.5em 0",
    overflow: "visible",
    padding: "1.75em 2.25em 2.25em 0",
    position: "relative",
  },
  ".cm-markra-table-hidden-line": {
    display: "none",
  },
  ".markra-table-scroll": {
    display: "block",
    overflowX: "auto",
  },
  ".markra-table-edge-input": {
    position: "absolute",
    insetInlineEnd: "1.5em",
    insetBlockEnd: "1.5em",
    width: "1px",
    height: "1em",
    opacity: "0",
    pointerEvents: "none",
    padding: "0",
    border: "0",
    font: "inherit",
  },
  ".markra-table-align-controls": {
    alignItems: "center",
    display: "flex",
    gap: "0.125em",
    left: "0.375em",
    position: "absolute",
    top: "0",
    zIndex: "10",
  },
  ".markra-table-size-controls": {
    display: "flex",
  },
  ".markra-table-control": {
    alignItems: "center",
    borderRadius: "999px",
    cursor: "pointer",
    display: "inline-flex",
    height: "1.5em",
    justifyContent: "center",
    opacity: "0",
    padding: "0",
    pointerEvents: "none",
    position: "absolute",
    transition: "opacity 150ms ease, scale 150ms ease",
    width: "1.5em",
  },
  ".cm-markra-table-wrap:hover .markra-table-control, .cm-markra-table-wrap:focus-within .markra-table-control": {
    opacity: "1",
    pointerEvents: "auto",
  },
  ".markra-table-size-button, .markra-table-align-button, .markra-table-width-button, .markra-table-delete-table, .markra-table-extra-control": {
    borderRadius: "0.375em",
    position: "static",
  },
  ".markra-table-add-column": {
    right: "0.375em",
    top: "50%",
    transform: "translateY(-50%)",
  },
  ".markra-table-add-row": {
    bottom: "0.375em",
    left: "50%",
    transform: "translateX(-50%)",
  },
  ".markra-table-delete-control": {
    transform: "translate(-50%, -50%)",
  },
  ".cm-markra-table": {
    borderCollapse: "collapse",
    width: "100%",
  },
  ".markra-table-size-popover": {
    background: "var(--bg-primary, Canvas)",
    border: "1px solid var(--border-default, currentColor)",
    borderRadius: "0.5em",
    boxShadow: "0 0.75em 2em rgb(0 0 0 / 18%)",
    padding: "0.55em",
    zIndex: "100",
  },
  ".markra-table-size-grid": {
    display: "grid",
    gap: "0.18em",
    gridTemplateColumns: `repeat(${tableSizePickerColumns}, 1.05em)`,
  },
  ".markra-table-size-cell": {
    aspectRatio: "1",
    background: "color-mix(in srgb, currentColor 4%, transparent)",
    border: "1px solid color-mix(in srgb, currentColor 20%, transparent)",
    borderRadius: "0.12em",
    padding: "0",
  },
  ".markra-table-size-cell:hover, .markra-table-size-cell:focus": {
    background: "color-mix(in srgb, var(--accent, currentColor) 22%, transparent)",
    borderColor: "var(--accent, currentColor)",
  },
  ".markra-table-size-footer": {
    alignItems: "center",
    display: "flex",
    gap: "0.35em",
    justifyContent: "center",
    marginTop: "0.5em",
  },
  ".markra-table-size-input": {
    width: "3.5em",
  },
  '.cm-markra-table[data-width-mode="auto"]': {
    tableLayout: "auto",
  },
  '.cm-markra-table[data-width-mode="even"]': {
    tableLayout: "fixed",
  },
  ".cm-markra-table th, .cm-markra-table td": {
    border: "1px solid color-mix(in srgb, currentColor 18%, transparent)",
    cursor: "text",
    outline: "none",
    padding: "0.42em 0.65em",
    position: "relative",
    verticalAlign: "top",
  },
  '.cm-markra-table[contenteditable="true"] th:focus, .cm-markra-table[contenteditable="true"] td:focus': {
    boxShadow: "inset 0 0 0 2px currentColor",
  },
  ".cm-markra-table th": {
    backgroundColor: "color-mix(in srgb, currentColor 5%, transparent)",
    fontWeight: "650",
  },
});

export function tablePreviewPlugin(
  options: TablePreviewPluginOptions = {},
) {
  const labels = { ...defaultTableLabels, ...options.labels };
  const widthMode = options.widthMode ?? "auto";
  const getDocumentKey = options.getDocumentKey ?? (() => undefined);

  return defineMarkraPlugin({
    id: tablePreviewRendererId,
    extension: [
      tableEdgeInput,
      padTableEdgeInput,
      tableEdgeSelection,
      Prec.high(keymap.of([
        { key: "Backspace", run: view => view.plugin(tableEdgeSelection)?.backspaceAfterTable() ?? false },
        { key: "ArrowDown", run: view => enterAdjacentTable(view, true) },
        { key: "ArrowUp", run: view => enterAdjacentTable(view, false) },
      ])),
      markraRenderer({
        id: tablePreviewRendererId,
        nodeNames: ["Table"],
        render(context) {
          if (!context.view.dom.ownerDocument.documentElement.hasAttribute("data-mobile") && context.revealed("node")) return true;

          const preview = tablePreviewAt(context.state, context.node.from, context.node.to);
          if (!preview) return true;

          const { doc } = context.state;
          const firstLine = doc.lineAt(context.node.from);
          const lastLine = doc.lineAt(context.node.to);
          if (syntaxTree(context.state).topNode.childAfter(context.node.to)?.name === "FencedCode") {
            context.add(Decoration.line({ class: "cm-markra-table-before-fenced-code" }).range(firstLine.from));
          }
          if (preview.from < firstLine.from) {
            context.add(Decoration.line({ class: "cm-markra-table-hidden-line" }).range(preview.from));
          }
          context.add(
            Decoration.replace({
              widget: new TableWidget(
                preview,
                labels,
                widthMode,
                getDocumentKey,
                options.images,
                options.links,
                JSON.stringify(codeMirrorSearchHighlights(
                  context.state, preview.from, preview.to,
                )),
              ),
            }).range(firstLine.from, firstLine.to),
          );
          for (
            let lineNumber = firstLine.number + 1;
            lineNumber <= lastLine.number;
            lineNumber += 1
          ) {
            const line = context.state.doc.line(lineNumber);
            context.add(
              Decoration.line({ class: "cm-markra-table-hidden-line" }).range(
                line.from,
              ),
            );
          }
          const separator = lastLine.number + 1 < doc.lines ? doc.line(lastLine.number + 1) : undefined;
          // Only the first blank is structural, regardless of how many editable rows follow.
          if (separator && !separator.text.trim()) {
            context.add(Decoration.line({ class: "cm-markra-table-separator" }).range(separator.from));
          }
          return false;
        },
      }),
      tableTheme,
    ],
  });
}
