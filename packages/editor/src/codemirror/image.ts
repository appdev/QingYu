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
  type MediaViewerHandle,
} from "./media-viewer.ts";
import {
  markraRenderer,
  type MarkraRendererContext,
  type MarkraSyntaxNode,
} from "./renderers.ts";
import { moveToEditableLine } from "./blank-lines.ts";
import {
  imageAttributeDetails,
  isImageAttributeListSource,
  replaceImageWidth,
  type ImageAttributeDetails,
} from "./image-attributes.ts";
import {
  imageDragActivated,
  imageDragWidth,
  persistedImageWidth,
} from "./image-resize.ts";
import { unescapeMarkdown, unquoteMarkdownTitle } from "./syntax.ts";

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

interface ImageSourceDetails {
  alt: string;
  source: string;
  title: string;
}

interface ImageDetails extends ImageSourceDetails {
  attributes: ImageAttributeDetails;
  authoredWidthPx: number | null;
  markdown: string;
}

interface ImageWidgetDomState {
  drag: ImageResizeDrag | null;
  frame: HTMLSpanElement;
  image: HTMLImageElement;
  imageRecord: ImageWidgetDomRecord;
  mediaViewer: MediaViewerHandle | null;
  onOutsideMouseDown: (event: MouseEvent) => void;
  onViewFocusOut: (event: FocusEvent) => void;
  onViewKeyDown: (event: KeyboardEvent) => void;
  resizeHandle: HTMLSpanElement | null;
  selected: boolean;
  sourceInput: HTMLInputElement;
  sourceRow: HTMLSpanElement;
  viewerButton: HTMLButtonElement;
  widget: ImageWidget;
}

interface ImageResizeDrag {
  activated: boolean;
  attributes: ImageAttributeDetails;
  document: string;
  from: number;
  maximumWidth: number;
  pointerId: number;
  pointerType: string;
  source: string;
  startWidth: number;
  startX: number;
  startY: number;
  to: number;
  width: number;
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

  const attributes = imageAttributeDetails(state, node);
  const title = node.getChild("LinkTitle");
  return {
    alt: unescapeMarkdown(
      state.sliceDoc(openingLabel.to, closingLabel.from),
    ),
    attributes,
    authoredWidthPx: attributes.authoredWidthPx,
    markdown: state.sliceDoc(node.from, attributes.ownedTo),
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
const imageWidgetDomStates = new WeakMap<
  CodeMirrorView,
  Set<ImageWidgetDomState>
>();

function unescapeImageMarkdownText(text: string) {
  return text.replace(/\\([\\\]"])/gu, "$1");
}

function parseImageMarkdownSource(
  source: string,
): (ImageSourceDetails & { markdown: string }) | null {
  const markdown = source;
  const match = /^!\[((?:\\.|[^\]\\])*)\]\((?:<([^>\n]+)>|([^\s)\n]+))(?:\s+"((?:\\.|[^"\n])*)")?\)/u.exec(
    markdown,
  );
  if (!match) return null;
  const attributes = markdown.slice(match[0].length);
  if (attributes && !isImageAttributeListSource(attributes)) {
    return null;
  }

  const [, alt = "", angleSource, plainSource, title = ""] = match;
  return {
    alt: unescapeImageMarkdownText(alt),
    markdown,
    source: angleSource ?? plainSource ?? "",
    title: unescapeImageMarkdownText(title),
  };
}

function createImageSourceIcon(ownerDocument: Document) {
  const svg = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "svg");
  const rect = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "rect");
  const circle = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "circle");
  const path = ownerDocument.createElementNS("http://www.w3.org/2000/svg", "path");

  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("class", "markra-image-node-source-icon");
  svg.setAttribute("fill", "none");
  svg.setAttribute("height", "18");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "18");
  rect.setAttribute("height", "18");
  rect.setAttribute("rx", "2");
  rect.setAttribute("ry", "2");
  rect.setAttribute("width", "18");
  rect.setAttribute("x", "3");
  rect.setAttribute("y", "3");
  circle.setAttribute("cx", "9");
  circle.setAttribute("cy", "9");
  circle.setAttribute("r", "2");
  path.setAttribute("d", "m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21");
  svg.append(rect, circle, path);
  return svg;
}

function updateImageElement(image: HTMLImageElement, widget: ImageWidget) {
  if (image.alt !== widget.details.alt) image.alt = widget.details.alt;
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

function updateImageFrame(frame: HTMLElement, widget: ImageWidget) {
  const { authoredWidthPx } = widget.details;
  if (authoredWidthPx === null) {
    frame.style.removeProperty("width");
    return;
  }
  frame.style.width = `${Math.max(17, authoredWidthPx)}px`;
}

function releaseImageResizeCapture(
  state: ImageWidgetDomState,
  pointerId: number,
) {
  if (
    !state.resizeHandle ||
    typeof state.resizeHandle.releasePointerCapture !== "function"
  ) {
    return;
  }
  try {
    state.resizeHandle.releasePointerCapture(pointerId);
  } catch {
    // Capture may already have been released by the browser.
  }
}

function cancelImageResize(
  state: ImageWidgetDomState,
  releaseCapture = true,
) {
  const drag = state.drag;
  if (!drag) return;
  state.drag = null;
  updateImageFrame(state.frame, state.widget);
  if (releaseCapture) releaseImageResizeCapture(state, drag.pointerId);
}

function clearImageSelection(root: HTMLElement, state: ImageWidgetDomState) {
  cancelImageResize(state);
  hideImageSource(root, state);
  const { view } = state.widget;
  const imageStart = Math.min(state.widget.from, view.state.doc.length);
  const imageEnd = Math.min(state.widget.to, view.state.doc.length);
  let selection = EditorSelection.cursor(0);
  if (imageStart > 0) {
    selection = EditorSelection.cursor(imageStart - 1);
  } else if (imageEnd < view.state.doc.length) {
    selection = EditorSelection.cursor(imageEnd + 1);
  }
  view.dispatch({ selection });
}

function imageWidgetDomKey(widget: ImageWidget) {
  return [
    widget.className,
    widget.details.markdown,
    widget.details.authoredWidthPx ?? "",
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

function hideImageSource(root: HTMLElement, state: ImageWidgetDomState) {
  const sourceVisible = state.sourceRow.isConnected;
  state.selected = false;
  state.sourceRow.remove();
  root.classList.remove("markra-image-node-selected");
  root.classList.remove("markra-image-node-source-invalid");
  if (sourceVisible) {
    root.ownerDocument.removeEventListener(
      "mousedown",
      state.onOutsideMouseDown,
      true,
    );
    if (root.isConnected) state.widget.view.requestMeasure();
  }
}

function showImageSource(
  root: HTMLElement,
  state: ImageWidgetDomState,
  preserveInput = false,
) {
  if (state.widget.readOnly) return false;
  const sourceVisible = state.sourceRow.isConnected;
  if (
    !preserveInput &&
    state.sourceInput.value !== state.widget.details.markdown
  ) {
    state.sourceInput.value = state.widget.details.markdown;
  }
  if (!sourceVisible) root.insertBefore(state.sourceRow, state.frame);
  state.selected = true;
  root.classList.add("markra-image-node-selected");
  if (!sourceVisible) {
    root.ownerDocument.addEventListener(
      "mousedown",
      state.onOutsideMouseDown,
      true,
    );
    if (root.isConnected) state.widget.view.requestMeasure();
  }
  return true;
}

function syncImageSource(root: HTMLElement, state: ImageWidgetDomState) {
  const { view } = state.widget;
  const markdown = state.sourceInput.value;
  if (!markdown.trim()) {
    const from = Math.min(state.widget.from, view.state.doc.length);
    const to = Math.min(state.widget.to, view.state.doc.length);
    view.dispatch({
      changes: { from, to },
      selection: EditorSelection.cursor(from),
      userEvent: "input.delete",
    });
    view.focus();
    return false;
  }

  const details = parseImageMarkdownSource(markdown);
  const source = details
    ? resolveImageSource(view, details, state.widget.resolver)
    : null;
  if (!details || !source) {
    root.classList.add("markra-image-node-source-invalid");
    return false;
  }

  root.classList.remove("markra-image-node-source-invalid");
  state.image.alt = details.alt;
  state.image.src = source;
  if (details.title) state.image.title = details.title;
  else state.image.removeAttribute("title");

  const from = Math.min(state.widget.from, view.state.doc.length);
  const to = Math.min(state.widget.to, view.state.doc.length);
  if (view.state.sliceDoc(from, to) !== markdown) {
    view.dispatch({
      changes: { from, insert: markdown, to },
      userEvent: "input",
    });
  }
  return true;
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
      this.details.authoredWidthPx === other.details.authoredWidthPx &&
      this.from === other.from &&
      this.readOnly === other.readOnly &&
      this.selected === other.selected &&
      this.source === other.source &&
      this.to === other.to
    );
  }

  ignoreEvent() {
    return true;
  }

  toDOM(view: CodeMirrorView) {
    const root = view.dom.ownerDocument.createElement("span");
    const frame = view.dom.ownerDocument.createElement("span");
    const sourceRow = view.dom.ownerDocument.createElement("span");
    const sourceInput = view.dom.ownerDocument.createElement("input");
    const viewerButton = view.dom.ownerDocument.createElement("button");
    const resizeHandle = this.readOnly
      ? null
      : view.dom.ownerDocument.createElement("span");
    const imageRecord = claimImageElement(root, this);
    const { image } = imageRecord;
    root.className = "markra-image-node";
    root.contentEditable = "false";
    root.draggable = false;
    frame.className = "markra-image-frame";
    sourceRow.className = "markra-image-node-source-row";
    sourceRow.contentEditable = "true";
    sourceInput.ariaLabel = "Image markdown source";
    sourceInput.className = "markra-image-node-source";
    sourceInput.contentEditable = "true";
    sourceInput.spellcheck = false;
    sourceInput.type = "text";
    sourceRow.append(createImageSourceIcon(view.dom.ownerDocument), sourceInput);
    viewerButton.type = "button";
    viewerButton.className = "markra-image-viewer-button";
    viewerButton.ariaLabel = "Enlarge image";
    viewerButton.title = "Enlarge image";
    viewerButton.append(createMediaViewerEnlargeIcon(
      view.dom.ownerDocument,
      "markra-image-viewer-icon",
    ));
    if (resizeHandle) {
      const visualHandle = view.dom.ownerDocument.createElement("span");
      resizeHandle.className = "markra-image-resize-hit-target";
      visualHandle.className = "markra-image-resize-handle";
      resizeHandle.append(visualHandle);
    }
    image.decoding = "async";
    image.draggable = false;
    image.loading = "lazy";
    updateImageElement(image, this);
    updateImageFrame(frame, this);
    frame.append(image, viewerButton);
    if (resizeHandle) frame.append(resizeHandle);
    root.append(frame);

    const state: ImageWidgetDomState = {
      drag: null,
      frame,
      image,
      imageRecord,
      mediaViewer: null,
      onOutsideMouseDown: (event: MouseEvent) => {
        if (event.target instanceof Node && root.contains(event.target)) return;
        const current = imageWidgetDomState.get(root);
        if (current) hideImageSource(root, current);
      },
      onViewFocusOut: (event: FocusEvent) => {
        if (
          event.relatedTarget instanceof Node &&
          view.dom.contains(event.relatedTarget)
        ) {
          return;
        }
        const current = imageWidgetDomState.get(root);
        if (current) cancelImageResize(current);
      },
      onViewKeyDown: (event: KeyboardEvent) => {
        const current = imageWidgetDomState.get(root);
        if (
          !current?.selected ||
          (event.target instanceof Node && current.sourceRow.contains(event.target)) ||
          event.altKey ||
          event.ctrlKey ||
          event.metaKey ||
          event.shiftKey
        ) {
          return;
        }

        if (event.key === "Escape") {
          event.preventDefault();
          clearImageSelection(root, current);
          return;
        }

        if (event.key === "Enter") {
          event.preventDefault();
          const imageEnd = Math.min(current.widget.to, view.state.doc.length);
          const hasFollowingLineBreak =
            view.state.sliceDoc(imageEnd, imageEnd + 1) === "\n";
          hideImageSource(root, current);
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
      resizeHandle,
      selected: false,
      sourceInput,
      sourceRow,
      viewerButton,
      widget: this,
    };
    imageWidgetDomState.set(root, state);
    let widgetStates = imageWidgetDomStates.get(view);
    if (!widgetStates) {
      widgetStates = new Set();
      imageWidgetDomStates.set(view, widgetStates);
    }
    widgetStates.add(state);

    const selectImageWidget = () => {
      if (state.widget.readOnly) return;
      view.focus();
      const anchor = Math.min(
        Math.max(state.widget.from + 1, 0),
        view.state.doc.length,
      );
      view.dispatch({ selection: EditorSelection.cursor(anchor) });
      showImageSource(root, state);
    };
    const selectImage = (event: MouseEvent) => {
      if (event.target instanceof Node && sourceRow.contains(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
      selectImageWidget();
    };
    const openImageViewer = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      state.mediaViewer?.close({ restoreFocus: false });
      state.mediaViewer = openMediaViewer({
        labels: {
          close: "Close enlarged image",
          dialog: "Enlarged image",
          enterFullscreen: "Enter full screen",
          exitFullscreen: "Exit full screen",
          reset: "Reset image view",
          viewport: "Image viewport",
          zoomIn: "Zoom in image",
          zoomOut: "Zoom out image",
        },
        media: image,
        mount: view.dom.closest(".markdown-paper") ?? view.dom.ownerDocument.body,
        restoreFocus: viewerButton,
      });
    };
    const keepSourceFocused = (event: MouseEvent) => {
      event.stopPropagation();
      showImageSource(root, state, true);
    };
    const handleSourceKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        clearImageSelection(root, state);
        view.focus();
        return;
      }
      if (
        event.key !== "Enter" ||
        event.altKey ||
        event.ctrlKey ||
        event.metaKey ||
        event.shiftKey
      ) {
        return;
      }
      event.preventDefault();
      if (!syncImageSource(root, state)) return;
      const imageEnd = Math.min(state.widget.to, view.state.doc.length);
      // The legacy editor treated Enter as "continue below the image". Move
      // across the existing line break so subsequent typing starts there.
      const anchor = Math.min(
        imageEnd + (view.state.sliceDoc(imageEnd, imageEnd + 1) === "\n" ? 1 : 0),
        view.state.doc.length,
      );
      hideImageSource(root, state);
      moveToEditableLine(view, anchor);
      view.focus();
    };

    const handleResizePointerDown = (event: PointerEvent) => {
      if (
        !resizeHandle ||
        state.widget.readOnly ||
        !event.isPrimary ||
        event.button !== 0
      ) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      cancelImageResize(state);
      selectImageWidget();

      const frameBounds = frame.getBoundingClientRect();
      const contentBounds = view.contentDOM.getBoundingClientRect();
      const maximumWidth = Math.max(
        17,
        Math.floor(contentBounds.right - frameBounds.left),
      );
      const startWidth = Math.max(
        17,
        Math.min(maximumWidth, Math.round(frameBounds.width)),
      );
      const { widget } = state;
      state.drag = {
        activated: false,
        attributes: widget.details.attributes,
        document: view.state.doc.toString(),
        from: widget.from,
        maximumWidth,
        pointerId: event.pointerId,
        pointerType: event.pointerType,
        source: widget.details.markdown,
        startWidth,
        startX: event.clientX,
        startY: event.clientY,
        to: widget.to,
        width: startWidth,
      };
      if (typeof resizeHandle.setPointerCapture === "function") {
        try {
          resizeHandle.setPointerCapture(event.pointerId);
        } catch {
          // The drag remains scoped to events delivered to the hit target.
        }
      }
    };
    const handleResizePointerMove = (event: PointerEvent) => {
      const drag = state.drag;
      if (!drag || drag.pointerId !== event.pointerId) return;
      event.preventDefault();
      event.stopPropagation();
      if (view.state.doc.toString() !== drag.document) {
        cancelImageResize(state);
        return;
      }
      const deltaX = event.clientX - drag.startX;
      const deltaY = event.clientY - drag.startY;
      if (!drag.activated) {
        if (!imageDragActivated(deltaX, deltaY)) return;
        drag.activated = true;
      }
      drag.width = imageDragWidth(
        drag.startWidth,
        deltaX,
        drag.maximumWidth,
      );
      frame.style.width = `${drag.width}px`;
    };
    const handleResizePointerUp = (event: PointerEvent) => {
      const drag = state.drag;
      if (!drag || drag.pointerId !== event.pointerId) return;
      event.preventDefault();
      event.stopPropagation();
      if (
        !drag.activated ||
        view.state.doc.toString() !== drag.document ||
        view.state.sliceDoc(drag.from, drag.to) !== drag.source
      ) {
        cancelImageResize(state);
        return;
      }

      const width = persistedImageWidth(drag.width, drag.maximumWidth);
      const replacement = replaceImageWidth(
        drag.source,
        drag.attributes,
        width,
      );
      state.drag = null;
      releaseImageResizeCapture(state, drag.pointerId);
      if (replacement === drag.source) {
        updateImageFrame(frame, state.widget);
        return;
      }
      view.dispatch({
        changes: { from: drag.from, insert: replacement, to: drag.to },
        selection: EditorSelection.cursor(drag.from + 1),
        userEvent: "input",
      });
    };
    const handleResizePointerCancel = (event: PointerEvent) => {
      const drag = state.drag;
      if (!drag || drag.pointerId !== event.pointerId) return;
      event.preventDefault();
      event.stopPropagation();
      cancelImageResize(state);
    };
    const handleLostPointerCapture = (event: PointerEvent) => {
      const drag = state.drag;
      if (!drag || drag.pointerId !== event.pointerId) return;
      event.stopPropagation();
      cancelImageResize(state, false);
    };
    const containResizeMouseEvent = (event: MouseEvent) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
    };

    root.addEventListener("mousedown", selectImage);
    root.addEventListener("click", selectImage);
    image.addEventListener("dblclick", openImageViewer);
    viewerButton.addEventListener("mousedown", (event) => {
      event.stopPropagation();
    });
    viewerButton.addEventListener("click", openImageViewer);
    resizeHandle?.addEventListener("pointerdown", handleResizePointerDown);
    resizeHandle?.addEventListener("pointermove", handleResizePointerMove);
    resizeHandle?.addEventListener("pointerup", handleResizePointerUp);
    resizeHandle?.addEventListener("pointercancel", handleResizePointerCancel);
    resizeHandle?.addEventListener(
      "lostpointercapture",
      handleLostPointerCapture,
    );
    resizeHandle?.addEventListener("mousedown", containResizeMouseEvent);
    resizeHandle?.addEventListener("click", containResizeMouseEvent);
    resizeHandle?.addEventListener("dblclick", containResizeMouseEvent);
    sourceRow.addEventListener("mousedown", keepSourceFocused);
    sourceRow.addEventListener("click", keepSourceFocused);
    sourceInput.addEventListener("input", () => syncImageSource(root, state));
    sourceInput.addEventListener("keydown", handleSourceKeyDown);
    view.dom.addEventListener("focusout", state.onViewFocusOut, true);
    view.dom.addEventListener("keydown", state.onViewKeyDown, true);
    if (this.selected) showImageSource(root, state);
    return root;
  }

  updateDOM(dom: HTMLElement) {
    const state = imageWidgetDomState.get(dom);
    if (!state) return false;
    cancelImageResize(state);
    if ((state.resizeHandle !== null) !== !this.readOnly) return false;
    if (state.widget.source !== this.source) {
      state.mediaViewer?.close({ restoreFocus: false });
      state.mediaViewer = null;
    }
    state.widget = this;
    state.imageRecord.from = this.from;
    state.imageRecord.key = imageWidgetDomKey(this);
    state.imageRecord.root = dom;
    updateImageElement(state.image, this);
    updateImageFrame(state.frame, this);
    const preserveInput = dom.ownerDocument.activeElement === state.sourceInput;
    // Cursor-driven source mode is temporary: once Enter moves the caret
    // beyond the image, only an actively focused source input may keep it open.
    if (this.selected || preserveInput) {
      showImageSource(dom, state, preserveInput);
    } else {
      hideImageSource(dom, state);
    }
    return true;
  }

  destroy(dom: HTMLElement) {
    const state = imageWidgetDomState.get(dom);
    if (!state) return;
    cancelImageResize(state);
    dom.ownerDocument.removeEventListener(
      "mousedown",
      state.onOutsideMouseDown,
      true,
    );
    state.widget.view.dom.removeEventListener(
      "focusout",
      state.onViewFocusOut,
      true,
    );
    state.widget.view.dom.removeEventListener(
      "keydown",
      state.onViewKeyDown,
      true,
    );
    state.mediaViewer?.close({ restoreFocus: false });
    const widgetStates = imageWidgetDomStates.get(state.widget.view);
    widgetStates?.delete(state);
    if (widgetStates?.size === 0) {
      imageWidgetDomStates.delete(state.widget.view);
    }
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
  details: ImageSourceDetails,
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
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) return;
        const widgetStates = imageWidgetDomStates.get(update.view);
        if (!widgetStates) return;
        for (const state of widgetStates) cancelImageResize(state);
      }),
      markraRenderer({
        id: "markra.image-preview",
        nodeNames: ["Image"],
        render(context) {
          const details = imageDetails(context.state, context.node);
          if (!details) return true;
          const startLine = context.state.doc.lineAt(context.node.from).number;
          const endLine = context.state.doc.lineAt(
            details.attributes.ownedTo,
          ).number;
          if (startLine !== endLine) return true;

          const source = resolveImageSource(
            context.view,
            details,
            options.resolveSource,
          );
          if (!source) return true;
          const line = context.state.doc.lineAt(context.node.from);
          if (
            context.node.from === line.from &&
            details.attributes.ownedTo === line.to
          ) {
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
                context.revealed("node-boundary"),
                source,
                details.attributes.ownedTo,
                context.view,
              ),
            }).range(context.node.from, details.attributes.ownedTo),
          );
          return false;
        },
      }),
      imageTheme,
    ],
  });
}
