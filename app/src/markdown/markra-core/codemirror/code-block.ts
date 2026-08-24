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
  markraCodeLanguageOptions,
  normalizeMarkraCodeLanguage,
  type MarkraCodeLanguageOption,
} from "../code-support";
import {
  ensureMermaidContrast,
  isMermaidLanguage,
  mermaidThemeFromElement,
  renderMermaidToSvg,
} from "../mermaid";
import { syntaxTreeChanged } from "./changes";
import { defineMarkraPlugin } from "./plugin";
import {
  createMediaViewerEnlargeIcon,
  openMediaViewer,
  type MediaViewerHandle,
} from "./media-viewer";
import {
  markraRenderer,
  type MarkraRendererContext,
  type MarkraSyntaxNode,
} from "./renderers";
import type {MarkdownControlHandle, MarkdownHostAdapter} from "../adapter";

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
  icons?: Partial<Record<"check" | "copy" | "more", string>>;
  languages?: readonly MarkraCodeLanguageOption[] | (() => readonly MarkraCodeLanguageOption[]);
  ligatures?: boolean;
  lineWrap?: boolean;
  openCodeLanguageMenu?: MarkdownHostAdapter["openCodeLanguageMenu"];
  plainTextLabel?: string;
  positionLanguagePopover?: (anchor: HTMLElement, popover: HTMLElement) => void;
  showLineNumbers?: boolean;
  updateLanguages?: (context: CodeBlockLanguageUpdateContext) => readonly string[];
  renderMermaid?: (
    context: CodeBlockMermaidContext,
  ) => Promise<string>;
}

export interface CodeBlockMermaidContext {
  readonly source: string;
  readonly theme: string;
  readonly view: CodeMirrorView;
}

export interface CodeBlockPreviewLabels {
  readonly clearLanguage: string;
  readonly codeCopied: string;
  readonly copyCode: string;
  readonly language: string;
  readonly mermaidDiagram: string;
  readonly mermaidError: string;
  readonly searchLanguage: string;
}

export interface CodeBlockLanguageUpdateContext {
  readonly languages: string[];
  readonly listElement: HTMLElement;
  readonly type: "init" | "match";
  readonly value: string;
}

interface CodeBlockParts {
  code: string;
  codeNode: MarkraSyntaxNode | null;
  hasClosingFence: boolean;
  language: string;
  languageFrom: number;
  languageTo: number;
  openingMarkTo: number;
}

const defaultLabels: CodeBlockPreviewLabels = {
  clearLanguage: "Clear",
  codeCopied: "Code copied",
  copyCode: "Copy code block",
  language: "Code block language",
  mermaidDiagram: "Mermaid diagram",
  mermaidError: "Unable to render Mermaid diagram",
  searchLanguage: "Search",
};

const svgNamespace = "http://www.w3.org/2000/svg";

function createCodeControlIcon(
  document: Document,
  className: string,
  symbol: string,
) {
  const icon = document.createElementNS(svgNamespace, "svg");
  const use = document.createElementNS(svgNamespace, "use");
  icon.classList.add(className);
  icon.setAttribute("aria-hidden", "true");
  use.setAttribute("href", symbol);
  use.setAttributeNS("http://www.w3.org/1999/xlink", "xlink:href", symbol);
  icon.append(use);
  return icon;
}

const codeBlockTheme = EditorView.baseTheme({
  ".cm-markra-code-content-line[data-code-line-wrap='true']": {
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
  },
  ".cm-markra-code-content-line[data-code-line-wrap='false']": {
    whiteSpace: "pre",
    wordBreak: "initial",
  },
  ".cm-markra-code-content-line[data-code-ligatures='true']": {
    fontVariantLigatures: "normal",
  },
  ".cm-markra-code-content-line[data-code-ligatures='false']": {
    fontVariantLigatures: "none",
  },
});

const codeBlockHeaderCleanups = new WeakMap<HTMLElement, () => void>();

class CodeBlockHeaderWidget extends WidgetType {
  constructor(
    readonly code: string,
    readonly displayLanguage: string,
    readonly icons: Readonly<Record<"check" | "copy" | "more", string>>,
    readonly labels: CodeBlockPreviewLabels,
    readonly language: string,
    readonly languageFrom: number,
    readonly languageTo: number,
    readonly languages: NonNullable<CodeBlockPreviewPluginOptions["languages"]>,
    readonly openingMarkTo: number,
    readonly openCodeLanguageMenu?: CodeBlockPreviewPluginOptions["openCodeLanguageMenu"],
    readonly positionLanguagePopover?: CodeBlockPreviewPluginOptions["positionLanguagePopover"],
    readonly updateLanguages?: CodeBlockPreviewPluginOptions["updateLanguages"],
  ) {
    super();
  }

  eq(other: CodeBlockHeaderWidget) {
    return (
      this.code === other.code &&
      this.displayLanguage === other.displayLanguage &&
      JSON.stringify(this.icons) === JSON.stringify(other.icons) &&
      this.language === other.language &&
      this.languageFrom === other.languageFrom &&
      this.languageTo === other.languageTo &&
      this.openingMarkTo === other.openingMarkTo &&
      JSON.stringify(this.labels) === JSON.stringify(other.labels) &&
      (this.languages === other.languages || JSON.stringify(this.languages) === JSON.stringify(other.languages))
    );
  }

  toDOM(view: CodeMirrorView) {
    const document = view.dom.ownerDocument;
    const wrapper = document.createElement("span");
    const label = document.createElement("span");
    const languageControl = label;
    const spacer = document.createElement("span");
    const copy = document.createElement("button");
    const more = document.createElement("button");
    let languagePopover: HTMLElement | null = null;
    let languageMenuHandle: MarkdownControlHandle | null = null;
    let removeOutsideListener: (() => void) | null = null;

    wrapper.className = "protyle-action cm-markra-code-actions";
    label.className = "protyle-action--first protyle-action__language markra-code-language-control cm-markra-code-header markra-code-language-label";
    label.textContent = this.displayLanguage;
    spacer.className = "fn__flex-1";
    label.ariaLabel = this.labels.language;
    label.role = "button";
    label.setAttribute("aria-disabled", String(view.state.readOnly));
    label.tabIndex = view.state.readOnly ? -1 : 0;
    copy.className = "protyle-icon protyle-action__copy markra-code-copy-button";
    copy.type = "button";
    copy.ariaLabel = this.labels.copyCode;
    copy.title = this.labels.copyCode;
    copy.dataset.copied = "false";
    copy.append(
      createCodeControlIcon(
        document,
        "markra-code-copy-icon",
        this.icons.copy,
      ),
      createCodeControlIcon(
        document,
        "markra-code-copy-check-icon",
        this.icons.check,
      ),
    );
    more.className = "protyle-icon protyle-action__menu markra-code-more-button";
    more.type = "button";
    more.ariaLabel = this.labels.language;
    more.title = this.labels.language;
    more.disabled = view.state.readOnly;
    more.append(createCodeControlIcon(
      document,
      "markra-code-more-icon",
      this.icons.more,
    ));

    const updateLanguage = (nextValue: string) => {
      if (view.state.readOnly) return;
      const nextLanguage = normalizeMarkraCodeLanguage(nextValue);
      if (nextLanguage === this.language) return;
      view.dispatch({
        changes: this.languageFrom < this.languageTo
          ? {
              from: this.languageFrom,
              insert: nextLanguage,
              to: this.languageTo,
            }
          : { from: this.openingMarkTo, insert: nextLanguage },
      });
    };

    const closeLanguagePopover = () => {
      languageMenuHandle?.destroy();
      languageMenuHandle = null;
      removeOutsideListener?.();
      removeOutsideListener = null;
      languagePopover?.remove();
      languagePopover = null;
    };

    const openLanguagePopover = () => {
      if (view.state.readOnly || languagePopover || languageMenuHandle) return;
      const baseLanguages = (typeof this.languages === "function" ? this.languages() : this.languages)
        .map((option) => option.value)
        .filter(Boolean)
        .sort((left, right) => left.localeCompare(right));
      let hostMenuDestroyed = false;
      let hostHandle: MarkdownControlHandle | null = null;
      hostHandle = this.openCodeLanguageMenu?.({
        anchor: label,
        currentLanguage: this.language,
        languages: baseLanguages,
        onDestroy: () => {
          hostMenuDestroyed = true;
          if (languageMenuHandle === hostHandle) languageMenuHandle = null;
        },
        onSelect: updateLanguage,
        ownerDocument: document,
      }) ?? null;
      if (hostHandle) {
        if (!hostMenuDestroyed) {
          languageMenuHandle = hostHandle;
          hostHandle.focus();
        }
        return;
      }
      const popover = document.createElement("div");
      const content = document.createElement("div");
      const search = document.createElement("input");
      const list = document.createElement("div");
      popover.className = "protyle-util markra-code-language-popover";
      content.className = "fn__flex-column";
      content.dataset.id = "codeLanguage";
      content.style.maxHeight = "50vh";
      search.className = "b3-text-field";
      search.placeholder = this.labels.searchLanguage;
      search.style.margin = "0 8px 4px 8px";
      list.className = "b3-list fn__flex-1 b3-list--background";
      list.style.position = "relative";
      content.append(search, list);
      popover.append(content);
      (view.dom.closest(".markdown-editor") ?? document.body).append(popover);
      languagePopover = popover;

      const applyHostLanguages = (
        languages: string[],
        type: CodeBlockLanguageUpdateContext["type"],
        value: string,
      ) => [...(this.updateLanguages?.({ languages, listElement: list, type, value }) ?? languages)];
      const languages = applyHostLanguages(baseLanguages, "init", "");

      const renderList = (items: readonly string[], value: string) => {
        list.replaceChildren();
        const clear = document.createElement("div");
        clear.className = "b3-list-item";
        clear.dataset.id = "clearLanguage";
        clear.textContent = this.labels.clearLanguage;
        list.append(clear);
        for (const item of items) {
          const option = document.createElement("div");
          option.className = "b3-list-item";
          option.dataset.id = item;
          option.textContent = item;
          list.append(option);
        }
        if (value && !items.includes(value)) {
          const custom = document.createElement("div");
          const strong = document.createElement("b");
          custom.className = "b3-list-item";
          custom.dataset.id = "customLanguage";
          strong.textContent = value.replace(/`| /gu, "_");
          custom.append(strong);
          list.append(custom);
        }
        (list.children[1] ?? list.firstElementChild)?.classList.add("b3-list-item--focus");
      };

      renderList(languages, "");
      search.addEventListener("input", (event) => {
        const value = search.value.trim();
        const lowerValue = value.toLowerCase();
        let matches = value
          ? languages.filter((item) => item.toLowerCase().includes(lowerValue))
            .sort((left, right) => {
              const leftStarts = left.toLowerCase().startsWith(lowerValue);
              const rightStarts = right.toLowerCase().startsWith(lowerValue);
              if (leftStarts && rightStarts) return left.length - right.length;
              if (leftStarts) return -1;
              if (rightStarts) return 1;
              return 0;
            })
          : languages;
        matches = applyHostLanguages(matches, "match", value);
        renderList(matches, value);
        event.stopPropagation();
      });
      list.addEventListener("click", (event) => {
        const item = (event.target as HTMLElement).closest<HTMLElement>(".b3-list-item");
        if (!item) return;
        const nextLanguage = item.dataset.id === "clearLanguage"
          ? ""
          : item.dataset.id === "customLanguage"
            ? item.textContent ?? ""
            : item.dataset.id ?? "";
        closeLanguagePopover();
        updateLanguage(nextLanguage);
        event.preventDefault();
        event.stopPropagation();
      });
      search.addEventListener("keydown", (event) => {
        const focused = list.querySelector<HTMLElement>(".b3-list-item--focus");
        if (event.key === "Escape") {
          closeLanguagePopover();
          label.focus();
          event.preventDefault();
        } else if (event.key === "Enter" && focused) {
          focused.click();
          event.preventDefault();
        } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          const items = [...list.querySelectorAll<HTMLElement>(".b3-list-item")];
          const currentIndex = Math.max(0, items.indexOf(focused as HTMLElement));
          const nextIndex = event.key === "ArrowDown"
            ? Math.min(items.length - 1, currentIndex + 1)
            : Math.max(0, currentIndex - 1);
          focused?.classList.remove("b3-list-item--focus");
          items[nextIndex]?.classList.add("b3-list-item--focus");
          items[nextIndex]?.scrollIntoView({ block: "nearest" });
          event.preventDefault();
        }
        event.stopPropagation();
      });
      this.positionLanguagePopover?.(label, popover);
      const outsideListener = (event: MouseEvent) => {
        const target = event.target as Node;
        if (!popover.contains(target) && !languageControl.contains(target) && !more.contains(target)) {
          closeLanguagePopover();
        }
      };
      document.addEventListener("mousedown", outsideListener, true);
      removeOutsideListener = () => document.removeEventListener("mousedown", outsideListener, true);
      search.select();
    };

    label.addEventListener("mousedown", (event) => event.stopPropagation());
    label.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openLanguagePopover();
    });
    label.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        openLanguagePopover();
      }
    });

    copy.addEventListener("mousedown", (event) => event.stopPropagation());
    copy.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      const clipboard = document.defaultView?.navigator.clipboard;
      if (!clipboard?.writeText) return;
      clipboard.writeText(this.code).then(() => {
        copy.ariaLabel = this.labels.codeCopied;
        copy.title = this.labels.codeCopied;
        copy.dataset.copied = "true";
      }).catch(() => undefined);
    });

    more.addEventListener("mousedown", (event) => event.stopPropagation());
    more.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openLanguagePopover();
    });

    // Keep both controls in one header surface. The closing widget stays a
    // compact exit target instead of creating detached chrome below the block.
    wrapper.append(languageControl, spacer, copy, more);
    codeBlockHeaderCleanups.set(wrapper, closeLanguagePopover);
    return wrapper;
  }

  destroy(dom: HTMLElement) {
    codeBlockHeaderCleanups.get(dom)?.();
    codeBlockHeaderCleanups.delete(dom);
  }
}

function moveSelectionAfterCodeBlock(
  view: CodeMirrorView,
  requestedAfterFence: number,
) {
  const afterFence = Math.min(requestedAfterFence, view.state.doc.length);
  const hasFollowingLineBreak =
    view.state.sliceDoc(afterFence, afterFence + 1) === "\n";
  const canMaterializeLine = !hasFollowingLineBreak && !view.state.readOnly;

  // The closing fence is visually folded. Materialize/select the line after
  // it so clicks in the visual gap can never append code inside the fence.
  view.dispatch({
    changes: canMaterializeLine
      ? { from: afterFence, insert: "\n" }
      : undefined,
    scrollIntoView: true,
    selection: EditorSelection.cursor(
      afterFence + (hasFollowingLineBreak || canMaterializeLine ? 1 : 0),
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
  ) {
    super();
  }

  eq(other: MermaidPreviewWidget) {
    return (
      this.source === other.source &&
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
    preview.dataset.appearanceState = "loading";
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
      preview.dataset.appearanceState = "loading";
      delete preview.dataset.error;
      preview.setAttribute("aria-busy", "true");
      this.renderMermaid({ source: this.source, theme, view })
        .then((svg) => {
          if (token !== runtime.renderToken) return;
          this.closeViewer(runtime);
          preview.innerHTML = svg;
          ensureMermaidContrast(preview);
          removeEmptyMermaidLabels(preview);
          this.appendZoomButton(runtime, view, preview, wrapper);
          preview.dataset.appearanceState = "ready";
          preview.ariaLabel = this.labels.mermaidDiagram;
          preview.setAttribute("aria-busy", "false");
        })
        .catch(() => {
          if (token !== runtime.renderToken) return;
          preview.textContent = this.labels.mermaidError;
          preview.dataset.error = "true";
          preview.dataset.appearanceState = "error";
          preview.ariaLabel = this.labels.mermaidError;
          preview.setAttribute("aria-busy", "false");
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
  const openingMark = node.getChildren("CodeMark")[0];
  const info = infoNode
    ? state.sliceDoc(infoNode.from, infoNode.to).trim()
    : "";
  const rawLanguage = info.split(/\s+/u)[0] ?? "";
  return {
    code: codeNode ? state.sliceDoc(codeNode.from, codeNode.to) : "",
    codeNode,
    hasClosingFence: node.getChildren("CodeMark").length > 1,
    language: normalizeMarkraCodeLanguage(rawLanguage),
    languageFrom: infoNode?.from ?? openingMark?.to ?? node.from,
    languageTo: infoNode ? infoNode.from + rawLanguage.length : openingMark?.to ?? node.from,
    openingMarkTo: openingMark?.to ?? node.from,
  };
}

const setMermaidPreviewFocusedEffect = StateEffect.define<boolean>();

interface MermaidPreviewState {
  readonly blocks: readonly MermaidPreviewBlock[];
  readonly decorations: DecorationSet;
  readonly focused: boolean;
}

interface MermaidPreviewBlock {
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
        !parts.hasClosingFence ||
        !parts.codeNode ||
        !parts.code.trim() ||
        !isMermaidLanguage(parts.language)
      ) {
        return;
      }
      blocks.push({
        from: node.from,
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

function fencedCodeStartingAt(state: EditorState, from: number) {
  const node = fencedCodeAtPosition(state, Math.min(state.doc.length, from + 1));
  return node?.from === from ? node : null;
}

interface HoveredCodeBlockState {
  readonly decorations: DecorationSet;
  readonly from: number | null;
}

const setHoveredCodeBlockEffect = StateEffect.define<number | null>({
  map(value, changes) {
    return value === null ? null : changes.mapPos(value, 1);
  },
});

const hoveredCodeBlockState = (state: EditorState, from: number | null): HoveredCodeBlockState => {
  if (from === null) return {decorations: Decoration.none, from: null};
  const node = fencedCodeStartingAt(state, from);
  if (!node) return {decorations: Decoration.none, from: null};
  return {
    decorations: Decoration.set([
      Decoration.line({attributes: {"data-code-block-hovered": "true"}}).range(state.doc.lineAt(node.from).from),
    ]),
    from: node.from,
  };
};

const hoveredCodeBlockField = StateField.define<HoveredCodeBlockState>({
  create(state) {
    return hoveredCodeBlockState(state, null);
  },
  update(previous, transaction) {
    let from = transaction.docChanged && previous.from !== null
      ? transaction.changes.mapPos(previous.from, 1)
      : previous.from;
    for (const effect of transaction.effects) {
      if (effect.is(setHoveredCodeBlockEffect)) from = effect.value;
    }
    if (!transaction.docChanged && !syntaxTreeChanged(transaction.startState, transaction.state) && from === previous.from) {
      return previous;
    }
    return hoveredCodeBlockState(transaction.state, from);
  },
  provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
});

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
  if (!selection.empty || view.state.readOnly) return false;
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

  // The first Enter creates this trailing empty code line. The next Enter
  // exits. Unfinished blocks are closed first so the new cursor position is
  // structurally outside the fence instead of extending code forever.
  moveSelectionAfterCodeBlock(view, node.to);
  return true;
}

function exitMermaidSource(view: CodeMirrorView) {
  const node = fencedCodeAt(view);
  if (!node) return false;
  const parts = codeBlockParts(view.state, node);
  if (!isMermaidLanguage(parts.language)) return false;

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
  mouseleave(_event, view) {
    syncHoveredCodeBlock(view, null);
    return false;
  },
  mousemove(event, view) {
    syncHoveredCodeBlock(view, codeBlockFromPointer(event, view));
    return false;
  },
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

function codeBlockFromPointer(event: MouseEvent, view: CodeMirrorView) {
  const rawFrom = (event.target instanceof Element ? event.target : null)
    ?.closest<HTMLElement>("[data-code-block-from]")?.dataset.codeBlockFrom;
  const targetFrom = rawFrom === undefined ? null : Number(rawFrom);
  if (targetFrom !== null && Number.isInteger(targetFrom) && fencedCodeStartingAt(view.state, targetFrom)) return targetFrom;
  try {
    const position = view.posAtCoords({x: event.clientX, y: event.clientY});
    return position === null ? null : fencedCodeAtPosition(view.state, position)?.from ?? null;
  } catch {
    return null;
  }
}

function syncHoveredCodeBlock(view: CodeMirrorView, from: number | null) {
  if (view.state.field(hoveredCodeBlockField).from !== from) {
    view.dispatch({effects: setHoveredCodeBlockEffect.of(from)});
  }
}

export function codeBlockPreviewPlugin(
  options: CodeBlockPreviewPluginOptions = {},
) {
  const plainTextLabel = options.plainTextLabel?.trim() || "Plain text";
  const labels = { ...defaultLabels, ...options.labels };
  const icons = {
    check: "#iconCheck",
    copy: "#iconCopy",
    more: "#iconMore",
    ...options.icons,
  };
  const languages = options.languages ?? markraCodeLanguageOptions;
  const ligatures = options.ligatures ?? true;
  const lineWrap = options.lineWrap ?? true;
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
      // Vertical margins and padding on editable lines are not part of
      // CodeMirror's height map in every WebView. State-field block widgets
      // are measured explicitly, so repeated blocks cannot accumulate a
      // pointer-to-caret offset.
      createMermaidPreviewField(labels, renderMermaid),
      hoveredCodeBlockField,
      markraRenderer({
        id: "markra.code-block-preview",
        nodeNames: ["FencedCode"],
        scope: "visible-range",
        render(context) {
          const { node, state, visibleRange } = context;
          const parts = codeBlockParts(state, node);
          const firstLine = state.doc.lineAt(node.from);
          const lastLine = state.doc.lineAt(node.to);
          // 未闭合围栏会被解析到文档末尾，闭合前保持源码可避免吞入下方正文。
          if (!parts.hasClosingFence) return false;
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
          const sourceRevealed =
            isMermaidLanguage(parts.language) &&
            (revealed || selectionAnchoredInside);
          if (
            !sourceRevealed &&
            parts.codeNode &&
            parts.code.trim() &&
            isMermaidLanguage(parts.language)
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
            const roleClass =
              line.number === firstLine.number
                ? sourceRevealed
                  ? "cm-markra-code-source-line"
                  : "cm-markra-code-opening-line"
                : parts.hasClosingFence && line.number === lastLine.number
                  ? sourceRevealed
                    ? "cm-markra-code-source-line"
                    : "cm-markra-code-closing-line"
                  : "cm-markra-code-content-line";
            const codeContentLine = roleClass === "cm-markra-code-content-line";
            const positionClasses = codeContentLine
              ? `${line.number === firstLine.number + 1 ? " markra-code-block cm-markra-code-content-first" : ""}${
                line.number === lastLine.number - 1 ? " cm-markra-code-content-last" : ""
              }`
              : "";
            if (codeContentLine) codeLineNumber += 1;
            const codeBlockIdentity = {"data-code-block-from": String(node.from)};
            const lineNumberVisibility = {
              "data-code-ligatures": String(ligatures),
              "data-code-line-numbers": String(showLineNumbers),
              "data-code-line-wrap": String(lineWrap),
            };
            context.add(
              Decoration.line({
                attributes: codeContentLine
                  ? {
                      ...codeBlockIdentity,
                      ...lineNumberVisibility,
                      ...(showLineNumbers
                        ? { "data-code-line-number": String(codeLineNumber) }
                        : {}),
                    }
                  : roleClass === "cm-markra-code-closing-line"
                    ? {
                        ...codeBlockIdentity,
                        ...lineNumberVisibility,
                        "data-code-block-active": String(revealed),
                        "data-code-block-end": String(node.to),
                      }
                    : codeBlockIdentity,
                class: `cm-markra-code-line ${roleClass}${positionClasses}`,
              }).range(line.from),
            );
          }

          if (!sourceRevealed && lineIntersects(firstLine, visibleRange)) {
            context.add(
              Decoration.replace({
                widget: new CodeBlockHeaderWidget(
                  parts.code,
                  parts.language || plainTextLabel,
                  icons,
                  labels,
                  parts.language,
                  parts.languageFrom,
                  parts.languageTo,
                  languages,
                  parts.openingMarkTo,
                  options.openCodeLanguageMenu,
                  options.positionLanguagePopover,
                  options.updateLanguages,
                ),
              }).range(firstLine.from, firstLine.to),
            );
          }
          if (
            !sourceRevealed &&
            parts.hasClosingFence &&
            lineIntersects(lastLine, visibleRange)
          ) {
            context.add(
              Decoration.replace({
                widget: new CodeBlockExitWidget(node.to),
              }).range(lastLine.from, node.to),
            );
          }

          if (parts.codeNode) {
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
      markraRenderer({
        id: "markra.indented-code-block-preview",
        nodeNames: ["CodeBlock"],
        scope: "visible-range",
        render(context) {
          const firstLine = context.state.doc.lineAt(context.node.from);
          const lastLine = context.state.doc.lineAt(context.node.to);
          const firstVisibleLine = context.state.doc.lineAt(
            Math.max(context.node.from, context.visibleRange.from),
          ).number;
          const lastVisibleLine = context.state.doc.lineAt(
            Math.max(context.node.from, Math.min(context.node.to - 1, context.visibleRange.to - 1)),
          ).number;

          for (
            let lineNumber = firstVisibleLine;
            lineNumber <= lastVisibleLine;
            lineNumber += 1
          ) {
            const line = context.state.doc.line(lineNumber);
            const first = line.number === firstLine.number;
            const last = line.number === lastLine.number;
            context.add(
              Decoration.line({
                class: `cm-markra-code-line cm-markra-code-content-line cm-markra-indented-code-line markra-code-block${
                  first ? " cm-markra-code-content-first" : ""
                }${last ? " cm-markra-code-content-last" : ""}`,
              }).range(line.from),
            );
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
