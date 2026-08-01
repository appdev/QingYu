import { afterEach, describe, expect, it } from "vitest";
import {
  openMediaViewer,
  type MediaViewerHandle,
  type MediaViewerLabels,
} from "./media-viewer.ts";

const labels: MediaViewerLabels = {
  close: "Close viewer",
  dialog: "Media viewer",
  enterFullscreen: "Enter full screen",
  exitFullscreen: "Exit full screen",
  reset: "Reset view",
  viewport: "Media viewport",
  zoomIn: "Zoom in",
  zoomOut: "Zoom out",
};

const handles: MediaViewerHandle[] = [];

function createViewer() {
  const outside = document.createElement("button");
  const mount = document.createElement("div");
  const background = document.createElement("button");
  const originallyInert = document.createElement("div");
  const restoreFocus = document.createElement("button");
  const image = document.createElement("img");
  outside.textContent = "Outside";
  background.textContent = "Editor action";
  originallyInert.setAttribute("inert", "");
  restoreFocus.textContent = "Enlarge";
  image.alt = "Synthetic media";
  image.src = "https://example.test/media.png";
  mount.append(background, originallyInert, image);
  document.body.append(outside, mount, restoreFocus);

  const handle = openMediaViewer({
    labels,
    media: image,
    mount,
    restoreFocus,
  });
  handles.push(handle);
  const dialog = mount.querySelector<HTMLElement>(
    ".markra-media-viewer-dialog",
  );
  const content = dialog?.querySelector<HTMLElement>(
    ".markra-media-viewer-content",
  );
  const canvas = dialog?.querySelector<HTMLElement>(
    ".markra-media-viewer-canvas",
  );
  if (!dialog || !content || !canvas) {
    throw new Error("media viewer did not open");
  }
  return {
    background,
    canvas,
    content,
    dialog,
    handle,
    originallyInert,
    outside,
    restoreFocus,
  };
}

function pressDocumentKey(key: string, shiftKey = false) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    key,
    shiftKey,
  });
  document.dispatchEvent(event);
  return event;
}

afterEach(() => {
  for (const handle of handles.splice(0)) handle.close({ restoreFocus: false });
  document.body.replaceChildren();
});

describe("openMediaViewer", () => {
  it("contains focus and restores background interactivity on close", () => {
    const {
      background,
      content,
      dialog,
      handle,
      originallyInert,
      outside,
      restoreFocus,
    } = createViewer();
    const firstControl = dialog.querySelector<HTMLButtonElement>(
      ".markra-media-viewer-zoom-out-button",
    );

    expect(background.hasAttribute("inert")).toBe(true);
    expect(outside.hasAttribute("inert")).toBe(true);
    expect(restoreFocus.hasAttribute("inert")).toBe(true);
    expect(originallyInert.hasAttribute("inert")).toBe(true);

    content.focus();
    const wrappedForward = pressDocumentKey("Tab");
    expect(wrappedForward.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(firstControl);

    firstControl?.focus();
    const wrappedBackward = pressDocumentKey("Tab", true);
    expect(wrappedBackward.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(content);

    outside.focus();
    pressDocumentKey("Tab");
    expect(document.activeElement).toBe(firstControl);

    handle.close();
    expect(background.hasAttribute("inert")).toBe(false);
    expect(outside.hasAttribute("inert")).toBe(false);
    expect(restoreFocus.hasAttribute("inert")).toBe(false);
    expect(originallyInert.hasAttribute("inert")).toBe(true);
    expect(document.activeElement).toBe(restoreFocus);
  });

  it("supports keyboard panning and resetting from the focused viewport", () => {
    const { canvas, content, dialog } = createViewer();
    dialog.querySelector<HTMLButtonElement>(
      ".markra-media-viewer-zoom-in-button",
    )?.click();
    content.focus();

    content.dispatchEvent(new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "ArrowRight",
    }));
    content.dispatchEvent(new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "ArrowDown",
    }));
    expect(canvas.style.transform).toBe("translate(-40px, -40px) scale(1.25)");

    content.dispatchEvent(new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "Home",
    }));
    expect(canvas.style.transform).toBe("translate(0px, 0px) scale(1)");
  });

  it("ignores wheel gestures without a vertical delta", () => {
    const { canvas, content } = createViewer();
    const event = new WheelEvent("wheel", {
      bubbles: true,
      cancelable: true,
      deltaX: 24,
      deltaY: 0,
    });

    content.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(false);
    expect(canvas.style.transform).toBe("translate(0px, 0px) scale(1)");
  });
});
