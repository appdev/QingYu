import { glass } from "glassy-liquid";

const svgNamespace = "http://www.w3.org/2000/svg";

type MediaViewerIconName =
  | "chevron-left"
  | "chevron-right"
  | "close"
  | "enlarge"
  | "exit-fullscreen"
  | "fullscreen"
  | "reset"
  | "zoom-in"
  | "zoom-out";

type MediaViewerIconChild = {
  readonly attributes: Readonly<Record<string, string>>;
  readonly tag: "path" | "rect";
};

const mediaViewerIconChildren = {
  "chevron-left": [{ tag: "path", attributes: { d: "m15 18-6-6 6-6" } }],
  "chevron-right": [{ tag: "path", attributes: { d: "m9 18 6-6-6-6" } }],
  close: [
    { tag: "path", attributes: { d: "M18 6 6 18" } },
    { tag: "path", attributes: { d: "m6 6 12 12" } },
  ],
  enlarge: [
    { tag: "path", attributes: { d: "M15 3h6v6" } },
    { tag: "path", attributes: { d: "m21 3-7 7" } },
    { tag: "path", attributes: { d: "M9 21H3v-6" } },
    { tag: "path", attributes: { d: "m3 21 7-7" } },
  ],
  "exit-fullscreen": [
    { tag: "path", attributes: { d: "M8 3v3a2 2 0 0 1-2 2H3" } },
    { tag: "path", attributes: { d: "M21 8h-3a2 2 0 0 1-2-2V3" } },
    { tag: "path", attributes: { d: "M3 16h3a2 2 0 0 1 2 2v3" } },
    { tag: "path", attributes: { d: "M16 21v-3a2 2 0 0 1 2-2h3" } },
  ],
  fullscreen: [
    { tag: "path", attributes: { d: "M8 3H5a2 2 0 0 0-2 2v3" } },
    { tag: "path", attributes: { d: "M21 8V5a2 2 0 0 0-2-2h-3" } },
    { tag: "path", attributes: { d: "M3 16v3a2 2 0 0 0 2 2h3" } },
    { tag: "path", attributes: { d: "M16 21h3a2 2 0 0 0 2-2v-3" } },
  ],
  reset: [
    { tag: "path", attributes: { d: "M3 12a9 9 0 1 0 3-6.7" } },
    { tag: "path", attributes: { d: "M3 3v6h6" } },
  ],
  "zoom-in": [
    { tag: "path", attributes: { d: "M12 5v14" } },
    { tag: "path", attributes: { d: "M5 12h14" } },
  ],
  "zoom-out": [
    { tag: "path", attributes: { d: "M5 12h14" } },
  ],
} as const satisfies Readonly<Record<MediaViewerIconName, readonly MediaViewerIconChild[]>>;

export interface MediaViewerLabels {
  readonly close: string;
  readonly dialog: string;
  readonly enterFullscreen: string;
  readonly exitFullscreen: string;
  readonly reset: string;
  readonly viewport: string;
  readonly zoomIn: string;
  readonly zoomOut: string;
}

export interface MediaViewerHandle {
  close(options?: { restoreFocus?: boolean }): unknown;
}

/** 同一篇笔记里的其他图片，按出现顺序；查看器里可以前后切换。 */
export interface MediaViewerGallery {
  readonly index: number;
  readonly items: readonly { readonly alt: string; readonly source: string; readonly title: string }[];
  readonly labels: { readonly next: string; readonly previous: string };
}

interface OpenMediaViewerOptions {
  readonly gallery?: MediaViewerGallery;
  readonly labels: MediaViewerLabels;
  readonly media: Element;
  readonly mount: Element;
  readonly restoreFocus?: HTMLElement | null;
}

const activeMediaViewers = new WeakMap<Document, MediaViewerHandle>();

function createMediaViewerIcon(
  document: Document,
  className: string,
  name: MediaViewerIconName,
) {
  const icon = document.createElementNS(svgNamespace, "svg");
  icon.classList.add(className);
  icon.setAttribute("aria-hidden", "true");
  icon.setAttribute("fill", "none");
  icon.setAttribute("height", "15");
  icon.setAttribute("stroke", "currentColor");
  icon.setAttribute("stroke-linecap", "round");
  icon.setAttribute("stroke-linejoin", "round");
  icon.setAttribute("stroke-width", "2");
  icon.setAttribute("viewBox", "0 0 24 24");
  icon.setAttribute("width", "15");
  for (const childDefinition of mediaViewerIconChildren[name]) {
    const child = document.createElementNS(svgNamespace, childDefinition.tag);
    for (const [attribute, value] of Object.entries(childDefinition.attributes)) {
      child.setAttribute(attribute, value);
    }
    icon.append(child);
  }
  return icon;
}

export function createMediaViewerEnlargeIcon(
  document: Document,
  className: string,
) {
  return createMediaViewerIcon(document, className, "enlarge");
}

export function openMediaViewer({
  gallery,
  labels,
  media,
  mount,
  restoreFocus,
}: OpenMediaViewerOptions): MediaViewerHandle {
  const document = media.ownerDocument;
  activeMediaViewers.get(document)?.close({ restoreFocus: false });

  const dialog = document.createElement("div");
  const panel = document.createElement("div");
  const toolbar = document.createElement("div");
  const zoomValue = document.createElement("span");
  const content = document.createElement("div");
  const canvas = document.createElement("div");
  const title = document.createElement("span");
  let shown = media.cloneNode(true) as Element;
  const separator = () => {
    const line = document.createElement("span");
    line.className = "markra-media-viewer-separator";
    line.ariaHidden = "true";
    return line;
  };
  const makeButton = (
    className: string,
    label: string,
    iconClassName: string,
    iconName: MediaViewerIconName,
  ) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = className;
    button.ariaLabel = label;
    button.title = label;
    button.append(createMediaViewerIcon(document, iconClassName, iconName));
    return button;
  };
  const zoomOut = makeButton(
    "markra-media-viewer-control-button markra-media-viewer-zoom-out-button",
    labels.zoomOut,
    "markra-media-viewer-zoom-out-icon",
    "zoom-out",
  );
  const zoomIn = makeButton(
    "markra-media-viewer-control-button markra-media-viewer-zoom-in-button",
    labels.zoomIn,
    "markra-media-viewer-zoom-in-icon",
    "zoom-in",
  );
  const reset = makeButton(
    "markra-media-viewer-control-button markra-media-viewer-reset-button",
    labels.reset,
    "markra-media-viewer-reset-icon",
    "reset",
  );
  const fullscreen = makeButton(
    "markra-media-viewer-control-button markra-media-viewer-fullscreen-button",
    labels.enterFullscreen,
    "markra-media-viewer-fullscreen-icon",
    "fullscreen",
  );
  const closeButton = makeButton(
    "markra-media-viewer-close-button",
    labels.close,
    "markra-media-viewer-close-icon",
    "close",
  );

  dialog.className = "markra-media-viewer-dialog";
  dialog.setAttribute("role", "dialog");
  dialog.setAttribute("aria-modal", "true");
  dialog.ariaLabel = labels.dialog;
  panel.className = "markra-media-viewer-panel";
  toolbar.className = "markra-media-viewer-toolbar";
  zoomValue.className = "markra-media-viewer-zoom-value";
  zoomValue.setAttribute("aria-live", "polite");
  content.className = "markra-media-viewer-content";
  content.tabIndex = 0;
  content.ariaLabel = labels.viewport;
  canvas.className = "markra-media-viewer-canvas";
  // 工具条最左边显示图片名称（替代文字或标题），没有就不显示。
  title.className = "markra-media-viewer-title";
  title.textContent = (media.getAttribute("alt") || media.getAttribute("title") || "").trim();
  title.hidden = !title.textContent;
  shown.classList.add("markra-media-viewer-media");
  if (shown.tagName.toLowerCase() === "img") {
    shown.classList.add("markra-media-viewer-image");
  }
  canvas.append(shown);

  let closed = false;
  let scale = 1;
  let translateX = 0;
  let translateY = 0;
  let drag: {
    onMedia: boolean;
    originX: number;
    originY: number;
    pointerId: number;
    startX: number;
    startY: number;
  } | null = null;
  const syncZoom = () => {
    const scaleValue = String(scale);
    canvas.style.transform = `translate(${translateX}px, ${translateY}px) scale(${scaleValue})`;
    content.dataset.zoom = scaleValue;
    zoomValue.textContent = `${Math.round(scale * 100)}%`;
  };
  const setZoom = (nextScale: number) => {
    scale = Math.max(0.25, Math.min(6, nextScale));
    syncZoom();
  };
  const setFullscreen = (isFullscreen: boolean) => {
    if (isFullscreen) dialog.dataset.fullscreen = "true";
    else delete dialog.dataset.fullscreen;
    const label = isFullscreen
      ? labels.exitFullscreen
      : labels.enterFullscreen;
    fullscreen.ariaLabel = label;
    fullscreen.title = label;
    fullscreen.ariaPressed = String(isFullscreen);
    fullscreen.replaceChildren(
      createMediaViewerIcon(
        document,
        "markra-media-viewer-fullscreen-icon",
        isFullscreen ? "exit-fullscreen" : "fullscreen",
      ),
    );
  };
  // 多张图片时的上一张、下一张和序号。
  let position = gallery && gallery.items.length > 1 ? Math.max(0, Math.min(gallery.index, gallery.items.length - 1)) : -1;
  const counter = document.createElement("span");
  counter.className = "markra-media-viewer-counter";
  const previous = gallery && position >= 0
    ? makeButton("markra-media-viewer-control-button markra-media-viewer-previous-button", gallery.labels.previous, "markra-media-viewer-previous-icon", "chevron-left")
    : null;
  const next = gallery && position >= 0
    ? makeButton("markra-media-viewer-control-button markra-media-viewer-next-button", gallery.labels.next, "markra-media-viewer-next-icon", "chevron-right")
    : null;
  const syncGallery = () => {
    if (!gallery || position < 0 || !previous || !next) return;
    counter.textContent = `${position + 1} / ${gallery.items.length}`;
    previous.disabled = position === 0;
    next.disabled = position === gallery.items.length - 1;
  };
  const step = (amount: -1 | 1) => {
    if (!gallery || position < 0) return;
    const target = position + amount;
    if (target < 0 || target >= gallery.items.length) return;
    position = target;
    const item = gallery.items[position];
    const image = document.createElement("img");
    image.className = "markra-media-viewer-media markra-media-viewer-image";
    image.alt = item.alt;
    if (item.title) image.title = item.title;
    image.decoding = "async";
    image.draggable = false;
    image.src = item.source;
    shown.replaceWith(image);
    shown = image;
    title.textContent = (item.alt || item.title).trim();
    title.hidden = !title.textContent;
    title.nextElementSibling?.toggleAttribute("hidden", title.hidden);
    scale = 1;
    translateX = 0;
    translateY = 0;
    syncZoom();
    syncGallery();
  };
  previous?.addEventListener("click", () => step(-1));
  next?.addEventListener("click", () => step(1));

  const onKeyDown = (event: KeyboardEvent) => {
    if ((event.key === "ArrowLeft" || event.key === "ArrowRight") && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
      if (position < 0) return;
      event.preventDefault();
      step(event.key === "ArrowLeft" ? -1 : 1);
      return;
    }
    if ((event.key === "+" || event.key === "=") && (event.metaKey || event.ctrlKey)) { event.preventDefault(); setZoom(scale + 0.25); return; }
    if (event.key === "-" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); setZoom(scale - 0.25); return; }
    if (event.key === "0" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); reset.click(); return; }
    // 空格开、空格关，与 Quick Look 一致；焦点在工具条按钮上时空格仍按下该按钮。
    if (event.key === " " && !(event.target instanceof HTMLButtonElement)) {
      event.preventDefault();
      event.stopPropagation();
      handle.close();
      return;
    }
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (dialog.dataset.fullscreen === "true") {
      setFullscreen(false);
      return;
    }
    handle.close();
  };
  const handle: MediaViewerHandle = {
    close(options = {}) {
      if (closed) return;
      closed = true;
      document.removeEventListener("keydown", onKeyDown, true);
      dialog.remove();
      for (const element of blocked) element.removeAttribute("inert");
      if (activeMediaViewers.get(document) === handle) {
        activeMediaViewers.delete(document);
      }
      if (options.restoreFocus !== false) restoreFocus?.focus();
    },
  };

  fullscreen.ariaPressed = "false";
  zoomOut.addEventListener("click", () => setZoom(scale - 0.25));
  zoomIn.addEventListener("click", () => setZoom(scale + 0.25));
  reset.addEventListener("click", () => {
    scale = 1;
    translateX = 0;
    translateY = 0;
    syncZoom();
  });
  fullscreen.addEventListener("click", () => {
    setFullscreen(dialog.dataset.fullscreen !== "true");
  });
  closeButton.addEventListener("click", () => handle.close());
  content.addEventListener("wheel", (event) => {
    event.preventDefault();
    setZoom(scale * (event.deltaY < 0 ? 1.12 : 1 / 1.12));
  }, { passive: false });
  content.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    // Pointer capture keeps Windows WebView drag updates flowing after the
    // cursor leaves the viewport while a large diagram or image is panned.
    content.setPointerCapture?.(event.pointerId);
    content.dataset.dragging = "true";
    drag = {
      onMedia: event.target instanceof Node && shown.contains(event.target),
      originX: translateX,
      originY: translateY,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
    };
  });
  content.addEventListener("pointermove", (event) => {
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    translateX = drag.originX + event.clientX - drag.startX;
    translateY = drag.originY + event.clientY - drag.startY;
    syncZoom();
  });
  const stopDragging = (event: PointerEvent) => {
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (content.hasPointerCapture?.(event.pointerId)) {
      content.releasePointerCapture(event.pointerId);
    }
    delete content.dataset.dragging;
    // 没有拖动、也没点在图片上：和预览、照片一样，点空白处关闭。
    const tapped = Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) < 4;
    const outside = !drag.onMedia;
    drag = null;
    if (event.type === "pointerup" && tapped && outside) handle.close();
  };
  // 双击图片在「适合窗口」和两倍之间切换。
  content.addEventListener("dblclick", (event) => {
    // 拖动用了指针捕获，双击事件的目标会变成内容区，所以按坐标判断是否点在图片上。
    const bounds = shown.getBoundingClientRect();
    if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) return;
    event.preventDefault();
    if (scale > 1.01) {
      scale = 1;
      translateX = 0;
      translateY = 0;
      syncZoom();
    } else setZoom(2);
  });
  content.addEventListener("pointerup", stopDragging);
  content.addEventListener("pointercancel", stopDragging);
  dialog.addEventListener("mousedown", (event) => {
    if (event.target === dialog) handle.close();
  });
  document.addEventListener("keydown", onKeyDown, true);

  if (previous && next) toolbar.append(previous, counter, next, separator());
  toolbar.append(title, separator(), zoomOut, zoomValue, zoomIn, separator(), reset, fullscreen, separator(), closeButton);
  title.nextElementSibling?.toggleAttribute("hidden", title.hidden);
  syncGallery();
  content.append(canvas);
  panel.append(content, toolbar);
  dialog.append(panel);
  mount.append(dialog);
  // 查看期间页面其余部分不可交互：编辑器拿不到焦点，按键和输入法都不会改到下面选中的图片。
  const blocked = [...mount.children].filter(element => element !== dialog && !element.hasAttribute("inert"));
  for (const element of blocked) element.setAttribute("inert", "");
  // 浮在图片下方的深色玻璃工具条，与编辑器工具栏同一个玻璃组件。
  glass(toolbar, {
    radius: "capsule", appearance: "dark", blur: 16, specular: 0, shadow: false,
    fill: "var(--viewer-glass-fill)",
  });
  activeMediaViewers.set(document, handle);
  syncZoom();
  // 用鼠标打开时不让关闭按钮出现焦点环；Esc 仍可关闭，Tab 可以进入工具条。
  content.focus({ preventScroll: true });
  return handle;
}
