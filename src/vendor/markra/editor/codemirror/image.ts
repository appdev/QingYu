import { ensureSyntaxTree, syntaxTree } from "@codemirror/language";
import { EditorSelection, EditorState } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  WidgetType,
  type EditorView as CodeMirrorView,
} from "@codemirror/view";
import { defineMarkraPlugin } from "./plugin.ts";
import {
  createMediaViewerEnlargeIcon,
  openMediaViewer,
  type MediaViewerGallery,
  type MediaViewerHandle,
} from "./media-viewer.ts";
import {
  markraRenderer,
  type MarkraRendererContext,
  type MarkraSyntaxNode,
} from "./renderers.ts";
import { markdownImageSizeSuffix, unescapeMarkdown, unquoteMarkdownTitle } from "./syntax.ts";

export interface MarkraImageSourceContext {
  readonly alt: string;
  readonly source: string;
  readonly state: EditorState;
  readonly title: string;
  readonly view: CodeMirrorView;
}

export interface ImagePreviewPluginOptions {
  className?: string;
  resolveSource?: (context: MarkraImageSourceContext) => string | null;
}

interface ImageDetails {
  alt: string;
  label?: { from: number; to: number };
  markdown: string;
  source: string;
  title: string;
}

interface ImageWidgetDomState {
  cancelPress: () => void;
  frame: HTMLSpanElement;
  image: HTMLImageElement;
  imageRecord: ImageWidgetDomRecord;
  mediaViewer: MediaViewerHandle | null;
  onViewKeyDown: (event: KeyboardEvent) => void;
  viewerButton: HTMLButtonElement;
  widget: ImageWidget;
}

interface ImageWidgetDomRecord {
  claimed: boolean;
  from: number;
  image: HTMLImageElement;
  key: string;
  root: HTMLElement;
}

const safeDataImage = /^data:image\/(?:avif|gif|jpeg|png|webp);base64,/iu;
const scheme = /^([a-z][a-z\d+.-]*):/iu;

const imageTheme = EditorView.baseTheme({
  ".cm-markra-image": {
    borderRadius: "0.35em",
    display: "inline-block",
    maxHeight: "32rem",
    maxWidth: "100%",
    objectFit: "contain",
    verticalAlign: "middle",
  },
});

function imageDetails(
  state: EditorState,
  node: MarkraSyntaxNode,
): ImageDetails | null {
  const marks = node.getChildren("LinkMark");
  const url = node.getChild("URL");
  const openingLabel = marks[0];
  const closingLabel = marks[1];
  if (!url || !openingLabel || !closingLabel) return null;

  const title = node.getChild("LinkTitle");
  return {
    alt: unescapeMarkdown(
      state.sliceDoc(openingLabel.to, closingLabel.from),
    ),
    label: { from: openingLabel.to, to: closingLabel.from },
    markdown: state.sliceDoc(node.from, node.to),
    source: unescapeMarkdown(state.sliceDoc(url.from, url.to).trim()),
    title: title
      ? unquoteMarkdownTitle(state.sliceDoc(title.from, title.to).trim())
      : "",
  };
}

export function resolveSafeImageSource(source: string) {
  const candidate = source.trim();
  if (!candidate) return null;
  if (safeDataImage.test(candidate)) return candidate;

  const matchedScheme = scheme.exec(candidate)?.[1]?.toLocaleLowerCase();
  if (!matchedScheme) return candidate;
  return matchedScheme === "http" ||
    matchedScheme === "https" ||
    matchedScheme === "blob"
    ? candidate
    : null;
}

const imageWidgetDomState = new WeakMap<HTMLElement, ImageWidgetDomState>();
const imageWidgetDomRecords = new WeakMap<
  CodeMirrorView,
  Set<ImageWidgetDomRecord>
>();

type ImageSize = "small" | "fit" | "cover";

function standaloneImage(widget: ImageWidget) {
  const { state } = widget.view;
  const line = state.doc.lineAt(widget.from);
  const { label } = widget.details;
  if (!label || line.text.trim() !== widget.details.markdown) return null;
  const suffix = markdownImageSizeSuffix.exec(state.sliceDoc(label.from, label.to));
  return {
    line,
    size: (suffix?.[1].toLowerCase() ?? "fit") as ImageSize,
    sizeFrom: label.to - (suffix?.[0].length ?? 0),
    sizeTo: label.to,
  };
}

export function imageMenuTarget(target: Element) {
  const root = target.closest(".markra-image-frame")?.parentElement;
  const state = root && imageWidgetDomState.get(root);
  const image = state && standaloneImage(state.widget);
  return image && {
    ...image,
    source: state.widget.details.source,
    openPreview: () => state.viewerButton.click(),
  };
}

/** 笔记里能显示的全部图片，按出现顺序，查看器用它前后切换。 */
function noteImages(view: CodeMirrorView, resolver: ImagePreviewPluginOptions["resolveSource"]) {
  const items: (MediaViewerGallery["items"][number] & { from: number })[] = [];
  const tree = ensureSyntaxTree(view.state, view.state.doc.length, 200) ?? syntaxTree(view.state);
  tree.iterate({
    enter(node) {
      if (node.name !== "Image") return;
      const details = imageDetails(view.state, node.node as MarkraSyntaxNode);
      const source = details && resolveImageSource(view, details, resolver);
      if (details && source) {
        items.push({ alt: details.alt.replace(markdownImageSizeSuffix, ""), from: node.from, source, title: details.title });
      }
      return false;
    },
  });
  return items;
}

function updateImageElement(image: HTMLImageElement, widget: ImageWidget) {
  const sized = standaloneImage(widget);
  const alt = sized && sized.sizeFrom !== sized.sizeTo
    ? unescapeMarkdown(widget.view.state.sliceDoc(widget.details.label!.from, sized.sizeFrom))
    : widget.details.alt;
  if (image.alt !== alt) image.alt = alt;
  if (sized) image.dataset.imageSize = sized.size;
  else delete image.dataset.imageSize;
  if (image.className !== widget.className) image.className = widget.className;
  // Editing above an image shifts its Markdown range and updates the widget.
  // Reassigning an unchanged src makes browsers reload the image and flash.
  if (image.getAttribute("src") !== widget.source) {
    image.src = widget.source;
  }
  if (widget.details.title) {
    if (image.title !== widget.details.title) image.title = widget.details.title;
  } else if (image.hasAttribute("title")) {
    image.removeAttribute("title");
  }
}

function imageWidgetDomKey(widget: ImageWidget) {
  return [
    widget.className,
    widget.details.markdown,
    widget.readOnly ? "readonly" : "editable",
    widget.source,
  ].join("\u0000");
}

function claimImageElement(root: HTMLElement, widget: ImageWidget) {
  let records = imageWidgetDomRecords.get(widget.view);
  if (!records) {
    records = new Set();
    imageWidgetDomRecords.set(widget.view, records);
  }

  const key = imageWidgetDomKey(widget);
  const composing =
    widget.view.composing ||
    widget.view.dom.dataset.markraComposing === "true";
  let record: ImageWidgetDomRecord | undefined;
  if (composing) {
    record = [...records]
      .filter((candidate) => !candidate.claimed && candidate.key === key)
      .sort(
        (left, right) =>
          Math.abs(left.from - widget.from) -
          Math.abs(right.from - widget.from),
      )[0];
  }

  if (!record) {
    record = {
      claimed: false,
      from: widget.from,
      image: root.ownerDocument.createElement("img"),
      key,
      root,
    };
    records.add(record);
  }

  const claimedRecord = record;
  claimedRecord.claimed = true;
  claimedRecord.from = widget.from;
  claimedRecord.root = root;
  queueMicrotask(() => {
    claimedRecord.claimed = false;
  });
  return claimedRecord;
}

// 与 Pinto 一致：单击只选中整张图片，不显示 Markdown 源码；选中时由样式画出选中色和描边。
function markImageSelected(root: HTMLElement, selected: boolean) {
  root.classList.toggle("markra-image-node-selected", selected);
}

class ImageWidget extends WidgetType {
  constructor(
    readonly className: string,
    readonly details: ImageDetails,
    readonly from: number,
    readonly readOnly: boolean,
    readonly resolver: ImagePreviewPluginOptions["resolveSource"],
    readonly selected: boolean,
    readonly source: string,
    readonly to: number,
    readonly view: CodeMirrorView,
  ) {
    super();
  }

  eq(other: ImageWidget) {
    return (
      this.className === other.className &&
      this.details.markdown === other.details.markdown &&
      this.from === other.from &&
      this.readOnly === other.readOnly &&
      this.selected === other.selected &&
      this.source === other.source &&
      this.to === other.to
    );
  }

  ignoreEvent(event: Event) {
    return event.type !== "contextmenu";
  }

  toDOM(view: CodeMirrorView) {
    const root = view.dom.ownerDocument.createElement("span");
    const frame = view.dom.ownerDocument.createElement("span");
    const viewerButton = view.dom.ownerDocument.createElement("button");
    const imageRecord = claimImageElement(root, this);
    const { image } = imageRecord;
    root.className = "markra-image-node";
    root.contentEditable = "false";
    root.draggable = false;
    frame.className = "markra-image-frame";
    viewerButton.type = "button";
    viewerButton.className = "markra-image-viewer-button";
    viewerButton.ariaLabel = "Enlarge image";
    viewerButton.title = "Enlarge image";
    viewerButton.append(createMediaViewerEnlargeIcon(
      view.dom.ownerDocument,
      "markra-image-viewer-icon",
    ));
    image.decoding = "async";
    image.draggable = false;
    image.loading = "lazy";
    updateImageElement(image, this);
    frame.append(image, viewerButton);
    root.append(frame);

    let press: { pointerId: number; x: number; y: number; timer: number } | undefined;
    let held = false;
    const cancelPress = () => {
      if (press) window.clearTimeout(press.timer);
      press = undefined;
    };
    const state: ImageWidgetDomState = {
      cancelPress,
      frame,
      image,
      imageRecord,
      mediaViewer: null,
      onViewKeyDown: (event: KeyboardEvent) => {
        const current = imageWidgetDomState.get(root);
        if (
          !current?.widget.selected ||
          event.altKey ||
          event.ctrlKey ||
          event.metaKey ||
          event.shiftKey
        ) {
          return;
        }

        // 和 Pinto、访达一样：选中的正好是这张图片时，按空格打开查看。
        if (event.key === " ") {
          const selection = view.state.selection.main;
          if (event.isComposing || selection.from !== current.widget.from || selection.to !== current.widget.to) return;
          openImageViewer(event);
          return;
        }

        if (event.key === "Enter") {
          event.preventDefault();
          const imageEnd = Math.min(current.widget.to, view.state.doc.length);
          const hasFollowingLineBreak =
            view.state.sliceDoc(imageEnd, imageEnd + 1) === "\n";
          view.dispatch({
            changes: hasFollowingLineBreak
              ? undefined
              : { from: imageEnd, insert: "\n" },
            selection: EditorSelection.cursor(imageEnd + 1),
            userEvent: "input",
          });
          view.focus();
          return;
        }

        if (event.key !== "Backspace" && event.key !== "Delete") return;
        event.preventDefault();
        const from = Math.min(current.widget.from, view.state.doc.length);
        const to = Math.min(current.widget.to, view.state.doc.length);
        view.dispatch({
          changes: { from, to },
          selection: EditorSelection.cursor(from),
          userEvent: "input.delete",
        });
        view.focus();
      },
      viewerButton,
      widget: this,
    };
    imageWidgetDomState.set(root, state);

    const touchDevice = view.dom.ownerDocument.documentElement.hasAttribute("data-mobile");
    const selectImage = (event: MouseEvent) => {
      if (event.button !== 0 || event.ctrlKey) return;
      event.preventDefault();
      event.stopPropagation();
      if (held || state.widget.readOnly) return;
      // 手机上点一下直接打开，和 Pinto 在 iPhone 上一样；桌面单击选中整张图片，双击打开。
      if (touchDevice) {
        if (event.type === "click") openImageViewer(event);
        return;
      }
      if (event.type !== "mousedown") return;
      view.focus();
      const from = Math.min(state.widget.from, view.state.doc.length);
      const to = Math.min(state.widget.to, view.state.doc.length);
      view.dispatch({ selection: EditorSelection.range(from, to), userEvent: "select.pointer" });
    };
    const openImageViewer = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      state.mediaViewer?.close({ restoreFocus: false });
      const items = noteImages(view, state.widget.resolver);
      const index = items.findIndex(item => item.from === state.widget.from);
      state.mediaViewer = openMediaViewer({
        gallery: index >= 0 ? { index, items, labels: { next: "下一张", previous: "上一张" } } : undefined,
        labels: {
          close: "关闭",
          dialog: "查看图片",
          enterFullscreen: "全屏",
          exitFullscreen: "退出全屏",
          reset: "还原大小",
          viewport: "图片",
          zoomIn: "放大",
          zoomOut: "缩小",
        },
        media: image,
        mount: view.dom.closest(".markdown-paper") ?? view.dom.ownerDocument.body,
        // 关闭后回到正文，图片仍保持选中，和 Pinto 关掉 Quick Look 后一样。
        restoreFocus: view.contentDOM,
      });
    };
    if (touchDevice) {
      root.addEventListener("pointerdown", event => {
        cancelPress();
        held = false;
        if (event.pointerType !== "touch" || !event.isPrimary || event.target !== image || !standaloneImage(state.widget)) return;
        const { pointerId, clientX: x, clientY: y } = event;
        const timer = window.setTimeout(() => {
          press = undefined;
          if (!image.isConnected || view.compositionStarted) return;
          held = true;
          image.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: x, clientY: y }));
        }, 450);
        press = { pointerId, x, y, timer };
      });
      root.addEventListener("pointermove", event => {
        if (press?.pointerId === event.pointerId && Math.hypot(event.clientX - press.x, event.clientY - press.y) > 10) cancelPress();
      });
      root.addEventListener("pointerup", cancelPress);
      root.addEventListener("pointercancel", cancelPress);
      root.addEventListener("contextmenu", event => {
        if (!event.isTrusted) return;
        cancelPress();
        if (held) { event.preventDefault(); event.stopPropagation(); }
      });
    }
    root.addEventListener("mousedown", selectImage);
    root.addEventListener("click", selectImage);
    image.addEventListener("dblclick", openImageViewer);
    viewerButton.addEventListener("mousedown", (event) => {
      event.stopPropagation();
    });
    viewerButton.addEventListener("click", openImageViewer);
    view.dom.addEventListener("keydown", state.onViewKeyDown, true);
    markImageSelected(root, this.selected);
    return root;
  }

  updateDOM(dom: HTMLElement) {
    const state = imageWidgetDomState.get(dom);
    if (!state) return false;
    if (state.widget.source !== this.source) {
      state.mediaViewer?.close({ restoreFocus: false });
      state.mediaViewer = null;
    }
    state.widget = this;
    state.imageRecord.from = this.from;
    state.imageRecord.key = imageWidgetDomKey(this);
    state.imageRecord.root = dom;
    updateImageElement(state.image, this);
    markImageSelected(dom, this.selected);
    return true;
  }

  destroy(dom: HTMLElement) {
    const state = imageWidgetDomState.get(dom);
    if (!state) return;
    state.cancelPress();
    state.widget.view.dom.removeEventListener(
      "keydown",
      state.onViewKeyDown,
      true,
    );
    state.mediaViewer?.close({ restoreFocus: false });
    if (state.imageRecord.root === dom) {
      const { view } = state.widget;
      const releaseRecord = () => {
        if (state.imageRecord.root !== dom) return;
        const records = imageWidgetDomRecords.get(view);
        records?.delete(state.imageRecord);
        if (records?.size === 0) imageWidgetDomRecords.delete(view);
      };
      if (
        view.composing ||
        view.dom.dataset.markraComposing === "true"
      ) {
        // CodeMirror destroys the old content tile before constructing its
        // IME replacement. Keep the decoded image available for that same
        // synchronous reconciliation, then release it if no replacement came.
        queueMicrotask(releaseRecord);
      } else {
        releaseRecord();
      }
    }
    imageWidgetDomState.delete(dom);
  }
}

function resolveImageSource(
  view: CodeMirrorView,
  details: ImageDetails,
  resolver: ImagePreviewPluginOptions["resolveSource"],
) {
  const sourceContext: MarkraImageSourceContext = {
    ...details,
    state: view.state,
    view,
  };
  if (!resolver) return resolveSafeImageSource(details.source);

  try {
    return resolver(sourceContext);
  } catch {
    return null;
  }
}

export function imagePreviewPlugin(options: ImagePreviewPluginOptions = {}) {
  const customClassName = options.className?.trim();
  const className = customClassName
    ? `cm-markra-image ${customClassName}`
    : "cm-markra-image";

  return defineMarkraPlugin({
    id: "markra.image-preview",
    extension: [
      markraRenderer({
        id: "markra.image-preview",
        nodeNames: ["Image"],
        render(context) {
          const startLine = context.state.doc.lineAt(context.node.from).number;
          const endLine = context.state.doc.lineAt(context.node.to).number;
          if (startLine !== endLine) return true;

          const details = imageDetails(context.state, context.node);
          if (!details) return true;
          // 只有选区把整张图片包进去时才算选中；光标停在图片前后不算。
          const selection = context.state.selection.main;
          const selected = !selection.empty && selection.from <= context.node.from && selection.to >= context.node.to;
          const source = resolveImageSource(
            context.view,
            details,
            options.resolveSource,
          );
          if (!source) return true;
          const line = context.state.doc.lineAt(context.node.from);
          if (line.text.trim() === details.markdown) {
            context.add(
              Decoration.line({ class: "cm-markra-image-line" }).range(
                line.from,
              ),
            );
          }
          context.add(
            Decoration.replace({
              widget: new ImageWidget(
                className,
                details,
                context.node.from,
                context.state.facet(EditorState.readOnly),
                options.resolveSource,
                selected,
                source,
                context.node.to,
                context.view,
              ),
            }).range(context.node.from, context.node.to),
          );
          return false;
        },
      }),
      imageTheme,
    ],
  });
}
