import { mountWebGlass, usesGlisse } from '../../../../web-glass-mount';
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
  readonly items: readonly { readonly alt: string; readonly source: string; readonly title: string; readonly original?: string }[];
  readonly labels: { readonly next: string; readonly previous: string };
}

interface OpenMediaViewerOptions {
  readonly gallery?: MediaViewerGallery;
  readonly labels: MediaViewerLabels;
  readonly media: Element;
  readonly mount: Element;
  readonly mountImage?: (image: HTMLImageElement, frame: HTMLElement, source: string, original: string) => (() => void) | undefined;
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
  mountImage,
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
  const phone = document.documentElement.hasAttribute("data-mobile");
  let shown = media.cloneNode(true) as Element;
  const separator = (group = "") => {
    const line = document.createElement("span");
    // 带 group 的分隔线只在那组按钮显示时才有用（手机上缩放按钮整组隐藏）。
    line.className = `markra-media-viewer-separator ${group}`.trim();
    line.ariaHidden = "true";
    return line;
  };
  const titleSeparator = separator();
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
  const selected = gallery?.items[gallery.index];
  let cleanImage = shown instanceof HTMLImageElement && selected?.original
    ? mountImage?.(shown, canvas, selected.source, selected.original) : undefined;

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
    touch: boolean;
  } | null = null;
  // 触摸中的所有手指；两根手指时是双指缩放。
  const touches = new Map<number, { x: number; y: number }>();
  let pinch: { distance: number; scale: number; localX: number; localY: number } | null = null;
  // 一次手势里用过双指，就不再把抬起的手指当成轻点或滑动。
  let pinched = false;
  let lastTap: { time: number; x: number; y: number } | null = null;
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
  const resetZoom = () => {
    scale = 1;
    translateX = 0;
    translateY = 0;
    syncZoom();
  };
  // 画布按中心缩放，平移叠加在外面：屏幕上的点 p 对应画布里的 (p - 中心 - 平移) / 缩放。
  const canvasCenter = () => {
    const bounds = canvas.getBoundingClientRect();
    return { x: bounds.left + bounds.width / 2 - translateX, y: bounds.top + bounds.height / 2 - translateY };
  };
  /** 以屏幕上的 (x, y) 为中心缩放到 nextScale，这一点在图上的位置保持不动。 */
  const zoomAt = (nextScale: number, x: number, y: number) => {
    const center = canvasCenter();
    const localX = (x - center.x - translateX) / scale, localY = (y - center.y - translateY) / scale;
    scale = Math.max(0.25, Math.min(6, nextScale));
    translateX = x - center.x - localX * scale;
    translateY = y - center.y - localY * scale;
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
    cleanImage?.();
    cleanImage = undefined;
    shown.replaceWith(image);
    shown = image;
    if (item.original) cleanImage = mountImage?.(image, canvas, item.source, item.original);
    if (!cleanImage) image.src = item.source;
    title.textContent = (item.alt || item.title).trim();
    title.hidden = !title.textContent;
    titleSeparator.hidden = title.hidden;
    resetZoom();
    syncGallery();
  };
  const canStep = (amount: -1 | 1) => Boolean(gallery) && position >= 0 && position + amount >= 0 && position + amount < gallery!.items.length;
  /** 跟手滑动后的切换：当前这张滑出屏幕，下一张从另一侧滑进来。减少动态效果时直接换。 */
  const slide = (amount: -1 | 1) => {
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) { step(amount); return; }
    const width = content.clientWidth || window.innerWidth;
    translateX = -amount * width;
    syncZoom();
    window.setTimeout(() => {
      if (closed) return;
      step(amount);
      // 新图片先放到另一侧（不带过渡），再滑回中间。
      content.dataset.dragging = "true";
      translateX = amount * width;
      syncZoom();
      void canvas.offsetWidth;
      delete content.dataset.dragging;
      translateX = 0;
      syncZoom();
    }, 180);
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
  let unmountGlass: (() => void) | undefined;
  const handle: MediaViewerHandle = {
    close(options = {}) {
      if (closed) return;
      closed = true;
      cleanImage?.();
      document.removeEventListener("keydown", onKeyDown, true);
      unmountGlass?.();
      dialog.remove();
      for (const element of blocked) element.removeAttribute("inert");
      document.dispatchEvent(new CustomEvent("media-viewer-toggle", { detail: false }));
      if (activeMediaViewers.get(document) === handle) {
        activeMediaViewers.delete(document);
      }
      if (options.restoreFocus !== false) restoreFocus?.focus();
    },
  };

  fullscreen.ariaPressed = "false";
  zoomOut.addEventListener("click", () => setZoom(scale - 0.25));
  zoomIn.addEventListener("click", () => setZoom(scale + 0.25));
  reset.addEventListener("click", resetZoom);
  fullscreen.addEventListener("click", () => {
    setFullscreen(dialog.dataset.fullscreen !== "true");
  });
  closeButton.addEventListener("click", () => handle.close());
  content.addEventListener("wheel", (event) => {
    event.preventDefault();
    setZoom(scale * (event.deltaY < 0 ? 1.12 : 1 / 1.12));
  }, { passive: false });
  const startDrag = (event: PointerEvent, x = event.clientX, y = event.clientY) => {
    drag = {
      onMedia: event.target instanceof Node && shown.contains(event.target),
      originX: translateX,
      originY: translateY,
      pointerId: event.pointerId,
      startX: x,
      startY: y,
      touch: event.pointerType === "touch",
    };
  };
  const startPinch = () => {
    const [a, b] = [...touches.values()];
    const center = canvasCenter(), midX = (a.x + b.x) / 2, midY = (a.y + b.y) / 2;
    pinch = {
      distance: Math.max(1, Math.hypot(a.x - b.x, a.y - b.y)),
      scale,
      localX: (midX - center.x - translateX) / scale,
      localY: (midY - center.y - translateY) / scale,
    };
    pinched = true;
    drag = null;
    content.dataset.dragging = "true";
  };
  content.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    // Pointer capture keeps Windows WebView drag updates flowing after the
    // cursor leaves the viewport while a large diagram or image is panned.
    content.setPointerCapture?.(event.pointerId);
    content.dataset.dragging = "true";
    if (event.pointerType === "touch") {
      touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (touches.size === 1) pinched = false;
      if (touches.size === 2) { startPinch(); return; }
      if (touches.size > 2) return;
    }
    startDrag(event);
  });
  content.addEventListener("pointermove", (event) => {
    if (touches.has(event.pointerId)) touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pinch && touches.size >= 2) {
      event.preventDefault();
      // 两指中点下的那一点跟着中点走，同时按两指距离缩放。
      const [a, b] = [...touches.values()];
      const center = canvasCenter(), midX = (a.x + b.x) / 2, midY = (a.y + b.y) / 2;
      scale = Math.max(0.5, Math.min(6, pinch.scale * Math.hypot(a.x - b.x, a.y - b.y) / pinch.distance));
      translateX = midX - center.x - pinch.localX * scale;
      translateY = midY - center.y - pinch.localY * scale;
      syncZoom();
      return;
    }
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault();
    // 手机上原尺寸时横向跟手，用来切换上一张、下一张；到头了只给一点阻尼，不跟满。
    if (drag.touch && scale <= 1.01) {
      const dx = event.clientX - drag.startX;
      translateX = canStep(dx < 0 ? 1 : -1) ? dx : dx * 0.3;
      translateY = 0;
      syncZoom();
      return;
    }
    translateX = drag.originX + event.clientX - drag.startX;
    translateY = drag.originY + event.clientY - drag.startY;
    syncZoom();
  });
  const stopDragging = (event: PointerEvent) => {
    if (content.hasPointerCapture?.(event.pointerId)) {
      content.releasePointerCapture(event.pointerId);
    }
    if (touches.delete(event.pointerId) && pinch) {
      pinch = null;
      // 缩得比原尺寸还小时松手弹回原样；还剩一根手指就接着拖动。
      if (scale < 1) resetZoom();
      const [rest] = [...touches.entries()];
      if (rest) {
        startDrag(event, rest[1].x, rest[1].y);
        if (drag) drag.pointerId = rest[0];
      } else delete content.dataset.dragging;
      return;
    }
    if (!drag || drag.pointerId !== event.pointerId) return;
    delete content.dataset.dragging;
    const dx = event.clientX - drag.startX, dy = event.clientY - drag.startY;
    const tapped = Math.hypot(dx, dy) < 4 && !pinched;
    const { onMedia, touch } = drag;
    drag = null;
    if (event.type !== "pointerup" || pinched) return;
    if (touch) {
      // 原尺寸时横向滑动切换图片，和「照片」一样；不够远或已到头就弹回原位。
      if (scale <= 1.01 && Math.abs(dx) > 60 && Math.abs(dx) > Math.abs(dy) * 1.5 && canStep(dx < 0 ? 1 : -1)) { slide(dx < 0 ? 1 : -1); return; }
      if (scale <= 1.01 && translateX !== 0) { resetZoom(); return; }
      // 在图片上快速点两下：放大到两倍（以点的位置为中心），再点两下还原。
      if (tapped && onMedia) {
        const now = event.timeStamp;
        if (lastTap && now - lastTap.time < 320 && Math.hypot(event.clientX - lastTap.x, event.clientY - lastTap.y) < 30) {
          lastTap = null;
          if (scale > 1.01) resetZoom(); else zoomAt(2, event.clientX, event.clientY);
          return;
        }
        lastTap = { time: now, x: event.clientX, y: event.clientY };
        return;
      }
    }
    // 没有拖动、也没点在图片上：和预览、照片一样，点空白处关闭。
    if (tapped && !onMedia) handle.close();
  };
  // 双击图片在「适合窗口」和两倍之间切换。
  content.addEventListener("dblclick", (event) => {
    // 拖动用了指针捕获，双击事件的目标会变成内容区，所以按坐标判断是否点在图片上。
    const bounds = shown.getBoundingClientRect();
    if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) return;
    event.preventDefault();
    if (scale > 1.01) resetZoom();
    else setZoom(2);
  });
  content.addEventListener("pointerup", stopDragging);
  content.addEventListener("pointercancel", stopDragging);
  dialog.addEventListener("mousedown", (event) => {
    if (event.target === dialog) handle.close();
  });
  document.addEventListener("keydown", onKeyDown, true);

  if (previous && next) toolbar.append(previous, counter, next, separator());
  // 手机上名称放在顶部正中，和「照片」一样；桌面放在工具条里。
  if (!phone) toolbar.append(title, titleSeparator);
  toolbar.append(
    zoomOut, zoomValue, zoomIn,
    separator("markra-media-viewer-zoom-separator"), reset, fullscreen,
    separator("markra-media-viewer-zoom-separator"), closeButton,
  );
  titleSeparator.hidden = title.hidden;
  syncGallery();
  content.append(canvas);
  panel.append(content, toolbar);
  if (phone) {
    title.classList.add("markra-media-viewer-title-top");
    panel.append(title);
  }
  dialog.append(panel);
  mount.append(dialog);
  // 查看期间页面其余部分不可交互：编辑器拿不到焦点，按键和输入法都不会改到下面选中的图片。
  const blocked = [...mount.children].filter(element => element !== dialog && !element.hasAttribute("inert"));
  for (const element of blocked) element.setAttribute("inert", "");
  if (usesGlisse(toolbar)) {
    toolbar.classList.add("web-glass-frost");
    unmountGlass = mountWebGlass(toolbar, { tint: 76, blur: 9 });
  } else toolbar.classList.add("lb", "qingyu-glass");
  activeMediaViewers.set(document, handle);
  // 手机上原生窗口在系统栏后面露出背景色，打开期间让它跟着变暗（preferences.ts）。
  document.dispatchEvent(new CustomEvent("media-viewer-toggle", { detail: true }));
  syncZoom();
  // 用鼠标打开时不让关闭按钮出现焦点环；Esc 仍可关闭，Tab 可以进入工具条。
  content.focus({ preventScroll: true });
  return handle;
}
