import { syntaxTree } from "@codemirror/language";
import {
  type ChangeSet,
  EditorSelection,
  EditorState,
  StateEffect,
  StateField,
  Transaction,
  type SelectionRange,
} from "@codemirror/state";
import { EditorView, ViewPlugin } from "@codemirror/view";
import type { EditorTextSelection } from "../text-selection";
import { getMarkdownOutline, readMarkdownFrontmatter } from "../markdown";
import {
  normalizedExternalAutolinkUrl,
  type SearchRange,
} from "../shared";
import { findCodeMirrorMathRanges } from "./math-preview";
import {
  focusVisualTableCell,
  tablePreviewEnabled,
} from "./table";

export interface ReplaceCodeMirrorMarkdownOptions {
  addToHistory?: boolean;
  historyBaselineMarkdown?: string;
}

export interface CodeMirrorSearchOptions {
  caseSensitive?: boolean;
}

export interface CodeMirrorMarkdownImageReference {
  alt: string;
  src: string;
}

const codeMirrorMarkdownImageInsertionTargetBrand: unique symbol = Symbol(
  "CodeMirrorMarkdownImageInsertionTarget",
);

export interface CodeMirrorMarkdownImageInsertionTarget {
  readonly [codeMirrorMarkdownImageInsertionTargetBrand]: true;
}

interface InternalCodeMirrorMarkdownImageInsertionTarget
  extends CodeMirrorMarkdownImageInsertionTarget {
  readonly id: number;
  readonly view: EditorView;
}

interface StoredCodeMirrorMarkdownImageInsertionTarget {
  readonly from: number;
  readonly id: number;
  readonly protectedFrontmatter: boolean;
  readonly to: number;
}

export interface CodeMirrorMarkdownLinkReference {
  href: string;
  label: string;
}

const headingNodePattern = /^(?:ATX|Setext)Heading([1-6])$/u;

export interface CodeMirrorHeadingAnchor {
  from: number;
  level: number;
  title: string;
  to: number;
}

export interface CodeMirrorDocumentAnchor {
  description: string;
  from: number;
  id: string;
  kind: "section" | "table";
  text: string;
  title: string;
  to: number;
}

export function comparableCodeMirrorMarkdown(markdown: string) {
  return markdown
    .replace(/\r\n?/gu, "\n")
    .replace(/[ \t]+$/gmu, "")
    .trim();
}

export function isCodeMirrorMarkdownEquivalent(
  view: EditorView,
  markdown: string,
) {
  return (
    comparableCodeMirrorMarkdown(view.state.doc.toString()) ===
    comparableCodeMirrorMarkdown(markdown)
  );
}

function boundedSelection(
  selection: SelectionRange,
  documentLength: number,
) {
  return EditorSelection.range(
    Math.min(selection.anchor, documentLength),
    Math.min(selection.head, documentLength),
  );
}

function replaceDocument(
  view: EditorView,
  markdown: string,
  addToHistory: boolean,
) {
  view.dispatch({
    annotations: Transaction.addToHistory.of(addToHistory),
    changes: {
      from: 0,
      insert: markdown,
      to: view.state.doc.length,
    },
    scrollIntoView: true,
    selection: boundedSelection(view.state.selection.main, markdown.length),
  });
}

export function replaceCodeMirrorMarkdown(
  view: EditorView,
  markdown: string,
  options: ReplaceCodeMirrorMarkdownOptions = {},
) {
  // Read-only protects user mutations, but the host must still be able to
  // reload or switch the document shown by an already-mounted editor.
  if (isCodeMirrorMarkdownEquivalent(view, markdown)) {
    const baseline = options.historyBaselineMarkdown;
    if (
      options.addToHistory &&
      baseline !== undefined &&
      comparableCodeMirrorMarkdown(baseline) !==
        comparableCodeMirrorMarkdown(markdown)
    ) {
      // Rebuild a missing shared-history step without recording the temporary
      // baseline itself, so one undo returns to the app's previous snapshot.
      replaceDocument(view, baseline, false);
      replaceDocument(view, markdown, true);
    }
    return true;
  }

  replaceDocument(view, markdown, options.addToHistory ?? false);
  return true;
}

export function readCodeMirrorTextSelection(
  view: EditorView,
): EditorTextSelection | null {
  const { doc, selection } = view.state;
  const { from, to } = selection.main;
  return from === to ? null : { from, text: doc.sliceString(from, to), to };
}

export function codeMirrorSelectionIsInsideFencedCode(state: EditorState) {
  const position = state.selection.main.head;
  let node: ReturnType<typeof syntaxTree>["topNode"] | null =
    syntaxTree(state).resolveInner(position, position === 0 ? 1 : -1);
  while (node) {
    if (node.name === "FencedCode" || node.name === "CodeBlock") return true;
    node = node.parent;
  }
  return false;
}

function headingTitle(source: string, level: number, setext: boolean) {
  const titleMarkdown = setext
    ? (source.split(/\r?\n/u)[0] ?? "").trim()
    : source
        .replace(/^[ \t]{0,3}#{1,6}(?:[ \t]+|$)/u, "")
        .replace(/[ \t]+#+[ \t]*$/u, "")
        .trim();
  const outline = getMarkdownOutline(
    `${"#".repeat(level)} ${titleMarkdown}`,
  );
  return outline[0]?.title ?? titleMarkdown;
}

export function readCodeMirrorHeadingAnchors(
  state: EditorState,
): CodeMirrorHeadingAnchor[] {
  const headings: CodeMirrorHeadingAnchor[] = [];
  const frontmatter = readMarkdownFrontmatter(state.doc.toString());
  const frontmatterRange = frontmatter.status === "valid" ? frontmatter.range : null;
  syntaxTree(state).iterate({
    enter(node) {
      const match = headingNodePattern.exec(node.name);
      if (!match) return;
      if (
        frontmatterRange &&
        node.from >= frontmatterRange.from &&
        node.to <= frontmatterRange.to
      ) {
        return;
      }

      const level = Number(match[1]);
      const source = state.sliceDoc(node.from, node.to);
      headings.push({
        from: node.from,
        level,
        title: headingTitle(source, level, node.name.startsWith("Setext")),
        to: node.to,
      });
    },
  });

  return headings;
}

const atxHeadingLinePattern = /^[\t ]{0,3}#{1,6}(?:[\t ]+|$)/u;
const setextHeadingLinePattern = /^[\t ]{0,3}(?:=+|-+)[\t ]*$/u;

function lineNeighborhoodMayContainHeading(
  state: EditorState,
  from: number,
  to: number,
) {
  const boundedFrom = Math.max(0, Math.min(from, state.doc.length));
  const boundedTo = Math.max(0, Math.min(to, state.doc.length));
  // Setext headings are owned jointly by a title line and its underline, so
  // edits must inspect one neighboring line on either side.
  const firstLine = Math.max(1, state.doc.lineAt(boundedFrom).number - 1);
  const lastLine = Math.min(
    state.doc.lines,
    state.doc.lineAt(boundedTo).number + 1,
  );

  for (let lineNumber = firstLine; lineNumber <= lastLine; lineNumber += 1) {
    const source = state.doc.line(lineNumber).text;
    if (
      atxHeadingLinePattern.test(source) ||
      setextHeadingLinePattern.test(source)
    ) {
      return true;
    }
  }
  return false;
}

export function updateCodeMirrorHeadingAnchors(
  headings: readonly CodeMirrorHeadingAnchor[],
  startState: EditorState,
  state: EditorState,
  changes: ChangeSet,
): CodeMirrorHeadingAnchor[] {
  let refresh = false;
  changes.iterChangedRanges((fromA, toA, fromB, toB) => {
    if (
      lineNeighborhoodMayContainHeading(startState, fromA, toA) ||
      lineNeighborhoodMayContainHeading(state, fromB, toB)
    ) {
      refresh = true;
    }
  });
  if (refresh) return readCodeMirrorHeadingAnchors(state);

  return headings.map((heading) => {
    const from = changes.mapPos(heading.from, 1);
    const to = changes.mapPos(heading.to, -1);
    return from === heading.from && to === heading.to
      ? heading
      : { ...heading, from, to };
  });
}

export function readCodeMirrorSectionAnchors(
  state: EditorState,
): CodeMirrorDocumentAnchor[] {
  const headings = readCodeMirrorHeadingAnchors(state);

  return headings.map((heading, index) => {
    let sectionEnd = state.doc.length;
    for (let nextIndex = index + 1; nextIndex < headings.length; nextIndex += 1) {
      const nextHeading = headings[nextIndex];
      if (nextHeading && nextHeading.level <= heading.level) {
        sectionEnd = nextHeading.from;
        break;
      }
    }

    return {
      description: `Section ${heading.title}`,
      from: heading.from,
      id: `section:${index}`,
      kind: "section",
      text: state.sliceDoc(heading.from, sectionEnd),
      title: heading.title,
      to: sectionEnd,
    };
  });
}

function splitTableRow(line: string) {
  const cells: string[] = [];
  let current = "";
  let escaped = false;

  for (const character of line.trim().replace(/^\|/u, "").replace(/\|$/u, "")) {
    if (character === "|" && !escaped) {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += character;
    escaped = character === "\\" ? !escaped : false;
  }
  cells.push(current.trim());
  return cells;
}

function tableHeaderTitle(tableMarkdown: string) {
  return splitTableRow(tableMarkdown.split(/\r?\n/u)[0] ?? "")
    .filter(Boolean)
    .slice(0, 3)
    .join(" / ");
}

export function readCodeMirrorTableAnchors(
  state: EditorState,
): CodeMirrorDocumentAnchor[] {
  const headings = readCodeMirrorHeadingAnchors(state);
  const anchors: CodeMirrorDocumentAnchor[] = [];

  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== "Table") return;

      const tableMarkdown = state.sliceDoc(node.from, node.to);
      const headerTitle = tableHeaderTitle(tableMarkdown);
      const currentHeading = [...headings]
        .reverse()
        .find((heading) => heading.from < node.from);
      const title = currentHeading
        ? `${currentHeading.title} table`
        : `Table: ${headerTitle}`;

      anchors.push({
        description: headerTitle
          ? `Markdown table ${title}: ${headerTitle}`
          : `Markdown table ${title}`,
        from: node.from,
        id: `table:${anchors.length}`,
        kind: "table",
        text: tableMarkdown,
        title,
        to: node.to,
      });
      return false;
    },
  });

  return anchors;
}

function searchTextMatches(
  candidate: string,
  query: string,
  caseSensitive: boolean,
) {
  return caseSensitive
    ? candidate === query
    : candidate.toLocaleLowerCase() === query.toLocaleLowerCase();
}

export function findCodeMirrorSearchMatches(
  state: EditorState,
  query: string,
  options: CodeMirrorSearchOptions = {},
): SearchRange[] {
  if (!query) return [];

  const document = state.doc.toString();
  const hiddenDisplayMath = findCodeMirrorMathRanges(state).filter(
    (range) => range.kind === "display",
  );
  const matches: SearchRange[] = [];
  let position = 0;

  while (position + query.length <= document.length) {
    const candidate = document.slice(position, position + query.length);
    const hidden = hiddenDisplayMath.some(
      (range) => position < range.to && position + query.length > range.from,
    );
    if (
      !hidden &&
      searchTextMatches(candidate, query, options.caseSensitive ?? false)
    ) {
      matches.push({ from: position, to: position + query.length });
      position += query.length;
      continue;
    }
    position += 1;
  }

  return matches;
}

function validSearchRange(
  match: SearchRange | null | undefined,
  documentLength: number,
): match is SearchRange {
  return Boolean(
    match &&
      Number.isInteger(match.from) &&
      Number.isInteger(match.to) &&
      match.from >= 0 &&
      match.from < match.to &&
      match.to <= documentLength,
  );
}

export function replaceCodeMirrorSearchMatch(
  view: EditorView,
  match: SearchRange | null | undefined,
  replacement: string,
) {
  if (
    view.state.facet(EditorState.readOnly) ||
    !validSearchRange(match, view.state.doc.length)
  ) {
    return false;
  }

  view.dispatch({
    changes: { from: match.from, insert: replacement, to: match.to },
    scrollIntoView: true,
  });
  return true;
}

export function replaceAllCodeMirrorSearchMatches(
  view: EditorView,
  matches: readonly SearchRange[],
  replacement: string,
) {
  if (view.state.facet(EditorState.readOnly)) return false;

  const valid = matches
    .filter((match) => validSearchRange(match, view.state.doc.length))
    .sort((left, right) => left.from - right.from);
  if (valid.length === 0) return false;

  const nonOverlapping: SearchRange[] = [];
  for (const match of valid) {
    const previous = nonOverlapping[nonOverlapping.length - 1];
    if (previous && match.from < previous.to) continue;
    nonOverlapping.push(match);
  }

  view.dispatch({
    changes: nonOverlapping.map((match) => ({
      from: match.from,
      insert: replacement,
      to: match.to,
    })),
    scrollIntoView: true,
  });
  return true;
}

export function insertCodeMirrorMarkdownSnippet(
  view: EditorView,
  open: string,
  close: string,
  placeholder: string,
) {
  if (view.state.facet(EditorState.readOnly)) return false;

  const { from, to } = view.state.selection.main;
  const selectedText = view.state.sliceDoc(from, to).replace(/\n/gu, " ");
  const content = selectedText || placeholder;
  const insertedText = `${open}${content}${close}`;
  const cursor = selectedText
    ? from + insertedText.length
    : from + open.length + content.length;

  view.dispatch({
    changes: { from, insert: insertedText, to },
    scrollIntoView: true,
    selection: EditorSelection.cursor(cursor),
  });
  view.focus();
  return true;
}

function escapeMarkdownLabel(label: string) {
  return label.replace(/\\/gu, "\\\\").replace(/\]/gu, "\\]");
}

export function serializeCodeMirrorMarkdownImage(reference: CodeMirrorMarkdownImageReference) {
  return `![${escapeMarkdownLabel(reference.alt || "image")}](${reference.src})`;
}

export function serializeCodeMirrorMarkdownLink(reference: CodeMirrorMarkdownLinkReference) {
  return `[${escapeMarkdownLabel(reference.label || reference.href)}](${reference.href})`;
}

const addCodeMirrorMarkdownImageInsertionTarget = StateEffect.define<
  StoredCodeMirrorMarkdownImageInsertionTarget
>();
const discardCodeMirrorMarkdownImageInsertionTargetEffect =
  StateEffect.define<number>();
const liveCodeMirrorMarkdownImageInsertionTargetViews =
  new WeakSet<EditorView>();

function mappedCodeMirrorMarkdownImageInsertionTarget(
  target: StoredCodeMirrorMarkdownImageInsertionTarget,
  changes: ChangeSet,
) {
  let invalid = false;
  changes.iterChangedRanges((fromA, toA) => {
    if (target.from === target.to) {
      if (fromA < target.from && toA > target.to) invalid = true;
      return;
    }

    if (
      (fromA === toA && fromA >= target.from && fromA <= target.to) ||
      (fromA < target.to && toA > target.from)
    ) {
      invalid = true;
    }
  });
  if (invalid) return null;

  const from = changes.mapPos(target.from, 1);
  return {
    ...target,
    from,
    to: target.from === target.to
      ? from
      : changes.mapPos(target.to, -1),
  };
}

const codeMirrorMarkdownImageInsertionTargets = StateField.define<
  readonly StoredCodeMirrorMarkdownImageInsertionTarget[]
>({
  create: () => [],
  update(targets, transaction) {
    let nextTargets = transaction.docChanged
      ? targets.flatMap((target) => {
          const mapped = mappedCodeMirrorMarkdownImageInsertionTarget(
            target,
            transaction.changes,
          );
          return mapped ? [mapped] : [];
        })
      : targets;

    for (const effect of transaction.effects) {
      if (effect.is(addCodeMirrorMarkdownImageInsertionTarget)) {
        nextTargets = [...nextTargets, effect.value];
      } else if (effect.is(discardCodeMirrorMarkdownImageInsertionTargetEffect)) {
        nextTargets = nextTargets.filter(({ id }) => id !== effect.value);
      }
    }
    return nextTargets;
  },
});

const codeMirrorMarkdownImageInsertionTargetViewPlugin = ViewPlugin.fromClass(
  class {
    private readonly view: EditorView;

    constructor(view: EditorView) {
      this.view = view;
      liveCodeMirrorMarkdownImageInsertionTargetViews.add(view);
    }

    destroy() {
      liveCodeMirrorMarkdownImageInsertionTargetViews.delete(this.view);
    }
  },
);

let nextCodeMirrorMarkdownImageInsertionTargetId = 1;

function ensureCodeMirrorMarkdownImageInsertionTargetState(view: EditorView) {
  if (view.state.field(codeMirrorMarkdownImageInsertionTargets, false)) return;
  view.dispatch({
    effects: StateEffect.appendConfig.of([
      codeMirrorMarkdownImageInsertionTargets,
      codeMirrorMarkdownImageInsertionTargetViewPlugin,
    ]),
  });
}

function internalCodeMirrorMarkdownImageInsertionTarget(
  target: CodeMirrorMarkdownImageInsertionTarget,
) {
  return target as InternalCodeMirrorMarkdownImageInsertionTarget;
}

function storedCodeMirrorMarkdownImageInsertionTarget(
  view: EditorView,
  target: CodeMirrorMarkdownImageInsertionTarget,
) {
  const internalTarget = internalCodeMirrorMarkdownImageInsertionTarget(target);
  if (
    internalTarget.view !== view ||
    !liveCodeMirrorMarkdownImageInsertionTargetViews.has(view)
  ) {
    return null;
  }
  return view
    .state
    .field(codeMirrorMarkdownImageInsertionTargets, false)
    ?.find(({ id }) => id === internalTarget.id) ?? null;
}

function markdownImageTarget(
  view: EditorView,
  selection: SelectionRange = view.state.selection.main,
) {
  const source = view.state.doc.toString();
  const frontmatter = readMarkdownFrontmatter(source);
  if (frontmatter.status !== "valid") {
    return {
      from: selection.from,
      protectedFrontmatter: false,
      selectedFrom: selection.from,
      selectedTo: selection.to,
      source,
      to: selection.to,
    };
  }

  let contentFrom = frontmatter.range.to;
  while (
    contentFrom < source.length &&
    (source[contentFrom] === "\r" || source[contentFrom] === "\n")
  ) {
    contentFrom += 1;
  }
  if (selection.from >= contentFrom) {
    return {
      from: selection.from,
      protectedFrontmatter: false,
      selectedFrom: selection.from,
      selectedTo: selection.to,
      source,
      to: selection.to,
    };
  }

  const from = contentFrom;
  const to = Math.max(selection.to, contentFrom);
  return {
    from,
    protectedFrontmatter: true,
    selectedFrom: from,
    selectedTo: to,
    source,
    to,
  };
}

export function captureCodeMirrorMarkdownImageInsertionTarget(
  view: EditorView,
): CodeMirrorMarkdownImageInsertionTarget | null {
  if (view.state.facet(EditorState.readOnly)) return null;

  ensureCodeMirrorMarkdownImageInsertionTargetState(view);
  if (!liveCodeMirrorMarkdownImageInsertionTargetViews.has(view)) return null;

  const target = markdownImageTarget(view);
  const id = nextCodeMirrorMarkdownImageInsertionTargetId;
  nextCodeMirrorMarkdownImageInsertionTargetId += 1;
  const publicTarget: InternalCodeMirrorMarkdownImageInsertionTarget = {
    [codeMirrorMarkdownImageInsertionTargetBrand]: true,
    id,
    view,
  };
  view.dispatch({
    effects: addCodeMirrorMarkdownImageInsertionTarget.of({
      from: target.from,
      id,
      protectedFrontmatter: target.protectedFrontmatter,
      to: target.to,
    }),
  });
  return publicTarget;
}

export function isCodeMirrorMarkdownImageInsertionTargetActive(
  view: EditorView,
  target: CodeMirrorMarkdownImageInsertionTarget,
) {
  return (
    !view.state.facet(EditorState.readOnly) &&
    storedCodeMirrorMarkdownImageInsertionTarget(view, target) !== null
  );
}

export function discardCodeMirrorMarkdownImageInsertionTarget(
  view: EditorView,
  target: CodeMirrorMarkdownImageInsertionTarget,
) {
  const storedTarget = storedCodeMirrorMarkdownImageInsertionTarget(view, target);
  if (!storedTarget) return false;
  view.dispatch({
    effects: discardCodeMirrorMarkdownImageInsertionTargetEffect.of(
      storedTarget.id,
    ),
  });
  return true;
}

function markdownImageInsertion(
  target: ReturnType<typeof markdownImageTarget>,
  markdown: string,
) {
  if (!target.protectedFrontmatter) {
    return {
      from: target.from,
      insert: markdown,
      markdownFrom: target.from,
      to: target.to,
    };
  }

  const before = target.source.slice(0, target.from);
  const after = target.source.slice(target.to);
  const newline = "\n";
  const prefix = before.endsWith(newline.repeat(2))
    ? ""
    : before.endsWith(newline)
      ? newline
      : newline.repeat(2);
  const suffix = after.length === 0 || after.startsWith(newline.repeat(2))
    ? ""
    : after.startsWith(newline)
      ? newline
      : newline.repeat(2);

  return {
    from: target.from,
    insert: `${prefix}${markdown}${suffix}`,
    markdownFrom: target.from + prefix.length,
    to: target.to,
  };
}

function enclosingLink(state: EditorState, from: number, to: number) {
  let node: ReturnType<typeof syntaxTree>["topNode"] | null =
    syntaxTree(state).resolveInner(from, 1);
  while (node) {
    if (node.name === "Link" && node.from <= from && node.to >= to) {
      const marks = node.getChildren("LinkMark");
      const opening = marks[0];
      const closing = marks[1];
      if (!opening || !closing) return null;
      return {
        from: node.from,
        label: state.sliceDoc(opening.to, closing.from),
        to: node.to,
      };
    }
    node = node.parent;
  }
  return null;
}

export function insertCodeMirrorMarkdownLink(view: EditorView) {
  if (view.state.facet(EditorState.readOnly)) return false;

  const { from, to } = view.state.selection.main;
  const activeLink = enclosingLink(view.state, from, to);
  if (activeLink) {
    view.dispatch({
      changes: {
        from: activeLink.from,
        insert: activeLink.label,
        to: activeLink.to,
      },
      scrollIntoView: true,
      selection: EditorSelection.range(
        activeLink.from,
        activeLink.from + activeLink.label.length,
      ),
    });
    view.focus();
    return true;
  }

  const selectedText = view.state.sliceDoc(from, to);
  const href = normalizedExternalAutolinkUrl(selectedText);
  const label = href ? selectedText.trim() : selectedText || "text";
  const target = href ?? "https://";
  const insertedText = serializeCodeMirrorMarkdownLink({ href: target, label });
  const selection = selectedText
    ? EditorSelection.range(from + 1, from + 1 + label.length)
    : EditorSelection.cursor(from + 1 + label.length);

  view.dispatch({
    changes: { from, insert: insertedText, to },
    scrollIntoView: true,
    selection,
  });
  view.focus();
  return true;
}

export function insertCodeMirrorMarkdownImage(view: EditorView) {
  if (view.state.facet(EditorState.readOnly)) return false;

  const target = markdownImageTarget(view);
  const selectedText = view.state.sliceDoc(target.selectedFrom, target.selectedTo).replace(/\n/gu, " ");
  const src = "assets/image.png";
  const insertedText = serializeCodeMirrorMarkdownImage({
    alt: selectedText || "alt",
    src,
  });
  const insertion = markdownImageInsertion(target, insertedText);
  const sourceFrom = insertion.markdownFrom + insertedText.lastIndexOf("(") + 1;

  view.dispatch({
    changes: {
      from: insertion.from,
      insert: insertion.insert,
      to: insertion.to,
    },
    scrollIntoView: true,
    selection: EditorSelection.range(sourceFrom, sourceFrom + src.length),
  });
  view.focus();
  return true;
}

export function insertCodeMirrorMarkdownImages(
  view: EditorView,
  images: readonly CodeMirrorMarkdownImageReference[],
) {
  if (
    images.length === 0 ||
    view.state.facet(EditorState.readOnly)
  ) {
    return false;
  }

  const insertedText = images.map(serializeCodeMirrorMarkdownImage).join("");
  const insertion = markdownImageInsertion(markdownImageTarget(view), insertedText);
  view.dispatch({
    changes: {
      from: insertion.from,
      insert: insertion.insert,
      to: insertion.to,
    },
    scrollIntoView: true,
    selection: EditorSelection.cursor(
      insertion.markdownFrom + insertedText.length,
    ),
  });
  view.focus();
  return true;
}

export function insertCodeMirrorMarkdownImagesAtTarget(
  view: EditorView,
  target: CodeMirrorMarkdownImageInsertionTarget,
  images: readonly CodeMirrorMarkdownImageReference[],
) {
  if (images.length === 0) return false;

  const storedTarget = storedCodeMirrorMarkdownImageInsertionTarget(view, target);
  if (
    !storedTarget ||
    view.state.facet(EditorState.readOnly)
  ) {
    return false;
  }

  const insertedText = images.map(serializeCodeMirrorMarkdownImage).join("");
  const insertion = markdownImageInsertion(
    {
      from: storedTarget.from,
      protectedFrontmatter: storedTarget.protectedFrontmatter,
      selectedFrom: storedTarget.from,
      selectedTo: storedTarget.to,
      source: view.state.doc.toString(),
      to: storedTarget.to,
    },
    insertedText,
  );
  view.dispatch({
    changes: {
      from: insertion.from,
      insert: insertion.insert,
      to: insertion.to,
    },
    effects: discardCodeMirrorMarkdownImageInsertionTargetEffect.of(
      storedTarget.id,
    ),
    scrollIntoView: true,
    selection: EditorSelection.cursor(
      insertion.markdownFrom + insertedText.length,
    ),
  });
  view.focus();
  return true;
}

export function insertCodeMirrorMarkdownLinks(
  view: EditorView,
  links: readonly CodeMirrorMarkdownLinkReference[],
) {
  if (
    links.length === 0 ||
    view.state.facet(EditorState.readOnly)
  ) {
    return false;
  }

  const { from, to } = view.state.selection.main;
  const insertedText = links.map(serializeCodeMirrorMarkdownLink).join(" ");
  view.dispatch({
    changes: { from, insert: insertedText, to },
    scrollIntoView: true,
    selection: EditorSelection.cursor(from + insertedText.length),
  });
  view.focus();
  return true;
}

const defaultMarkdownTable = [
  "|  |  |",
  "| --- | --- |",
  "|  |  |",
].join("\n");

export function insertCodeMirrorMarkdownTable(view: EditorView) {
  if (view.state.facet(EditorState.readOnly)) return false;

  const { from, to } = view.state.selection.main;
  const visualPreview = tablePreviewEnabled(view.state);
  view.dispatch({
    changes: { from, insert: defaultMarkdownTable, to },
    scrollIntoView: true,
    // A source cursor inside the table reveals the complete Markdown node.
    // Keep it at the boundary while the visual cell owns the editing focus.
    selection: EditorSelection.cursor(
      from + (visualPreview ? defaultMarkdownTable.length : 2),
    ),
  });
  view.focus();
  if (visualPreview) {
    focusVisualTableCell(view, from, -1, 0, true, 0);
  }
  return true;
}
