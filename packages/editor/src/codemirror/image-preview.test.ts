import { history, redo, undo } from "@codemirror/commands";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  imagePreviewPlugin,
  liveMarkdown,
  resolveSafeImageSource,
} from "./index.ts";

import "./dom.test-support.ts";

const views: EditorView[] = [];
const readOnlyCompartment = new Compartment();

function observeDocumentTransactions() {
  let count = 0;
  return {
    extension: EditorView.updateListener.of((update) => {
      count += update.transactions.filter((transaction) => (
        transaction.docChanged
      )).length;
    }),
    get count() {
      return count;
    },
    reset() {
      count = 0;
    },
  };
}

function createView(
  doc: string,
  plugin: ReturnType<typeof imagePreviewPlugin> | null = imagePreviewPlugin(),
  readOnly = false,
  additionalExtensions: Extension = [],
) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        history(),
        readOnlyCompartment.of(EditorState.readOnly.of(readOnly)),
        liveMarkdown({ plugins: plugin ? [plugin] : [] }),
        additionalExtensions,
      ],
      selection: { anchor: doc.length },
    }),
  });
  views.push(view);
  view.focus();
  view.dispatch({ selection: view.state.selection });
  return view;
}

function setViewReadOnly(view: EditorView, readOnly: boolean) {
  view.dispatch({
    effects: readOnlyCompartment.reconfigure(
      EditorState.readOnly.of(readOnly),
    ),
  });
}

function firstLine(view: EditorView) {
  return view.dom.querySelector(".cm-line")?.textContent ?? "";
}

function clickImage(view: EditorView) {
  view.dom.querySelector<HTMLImageElement>(".cm-markra-image")?.dispatchEvent(
    new MouseEvent("click", { bubbles: true, cancelable: true }),
  );
}

function sourceInput(view: EditorView) {
  return view.dom.querySelector<HTMLInputElement>(
    ".markra-image-node-source",
  );
}

function pressSelectedKey(view: EditorView, key: string) {
  const event = new KeyboardEvent("keydown", {
    bubbles: true,
    cancelable: true,
    key,
  });
  view.contentDOM.dispatchEvent(event);
  return event;
}

type TestPointerEventType =
  | "lostpointercapture"
  | "pointercancel"
  | "pointerdown"
  | "pointermove"
  | "pointerup";

interface TestPointerEventOptions {
  button?: number;
  clientX: number;
  clientY?: number;
  isPrimary?: boolean;
  pointerId?: number;
  pointerType?: string;
}

function dispatchPointerEvent(
  target: EventTarget,
  type: TestPointerEventType,
  options: TestPointerEventOptions,
) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    button: { value: options.button ?? 0 },
    clientX: { value: options.clientX },
    clientY: { value: options.clientY ?? 0 },
    isPrimary: { value: options.isPrimary ?? true },
    pointerId: { value: options.pointerId ?? 7 },
    pointerType: { value: options.pointerType ?? "mouse" },
  });
  target.dispatchEvent(event);
  return event;
}

function prepareImageResize(
  view: EditorView,
  startWidth: number,
  maximumWidth = 600,
) {
  const frame = view.dom.querySelector<HTMLElement>(".markra-image-frame");
  const handle = view.dom.querySelector<HTMLElement>(
    ".markra-image-resize-hit-target",
  );
  if (!frame || !handle) {
    throw new Error("Expected an editable image resize handle");
  }

  vi.spyOn(frame, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, startWidth, 200),
  );
  vi.spyOn(view.contentDOM, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, maximumWidth, 800),
  );
  const setPointerCapture = vi.fn();
  const releasePointerCapture = vi.fn();
  Object.defineProperties(handle, {
    releasePointerCapture: { value: releasePointerCapture },
    setPointerCapture: { value: setPointerCapture },
  });
  return { frame, handle, releasePointerCapture, setPointerCapture };
}

function dragImage(
  view: EditorView,
  startWidth: number,
  targetWidth: number,
  maximumWidth = 600,
  pointerType = "mouse",
) {
  const resize = prepareImageResize(view, startWidth, maximumWidth);
  dispatchPointerEvent(resize.handle, "pointerdown", {
    clientX: startWidth,
    pointerType,
  });
  dispatchPointerEvent(resize.handle, "pointermove", {
    clientX: targetWidth,
    pointerType,
  });
  dispatchPointerEvent(resize.handle, "pointerup", {
    clientX: targetWidth,
    pointerType,
  });
  return resize;
}

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
});

describe("imagePreviewPlugin", () => {
  it("waits for Enter before previewing a newly typed image", () => {
    const view = createView("");
    const markdown = "![a](https://example.test/image.png)";

    view.dispatch({
      changes: { from: 0, insert: markdown },
      selection: { anchor: markdown.length },
      userEvent: "input",
    });

    expect(
      view.dom.querySelector<HTMLInputElement>(".markra-image-node-source")
        ?.value,
    ).toBe(markdown);

    view.dispatch({
      changes: { from: markdown.length, insert: "\n" },
      selection: { anchor: markdown.length + 1 },
      userEvent: "input",
    });

    expect(view.dom.querySelector(".markra-image-node-source")).toBeNull();
    expect(view.dom.querySelector(".cm-markra-image")).not.toBeNull();
  });

  it("renders an existing image when the initial caret is at its end", () => {
    const doc = "![Synthetic alt](https://example.test/image.png)";
    const view = createView(doc);

    expect(view.dom.querySelector(".markra-image-node-source")).toBeNull();
    expect(view.dom.querySelector(".cm-markra-image")).not.toBeNull();
  });

  it("renders a safe Markdown image without changing its source", () => {
    const doc =
      'Before ![Synthetic alt](https://example.test/image.png "Preview") after\n\nEdit';
    const view = createView(doc);
    const image = view.dom.querySelector<HTMLImageElement>(
      ".cm-markra-image",
    );

    expect(image).not.toBeNull();
    expect(image?.getAttribute("src")).toBe(
      "https://example.test/image.png",
    );
    expect(image?.alt).toBe("Synthetic alt");
    expect(image?.title).toBe("Preview");
    expect(image?.loading).toBe("lazy");
    expect(image?.decoding).toBe("async");
    expect(firstLine(view)).toBe("Before  after");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("renders an authored image width", () => {
    const doc = "![Synthetic](./assets/mock.png){width=420px}\n\nEdit";
    const view = createView(doc);

    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("420px");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("renders an authored image width in read-only mode", () => {
    const doc = "![Synthetic](./assets/mock.png){width=240px}";
    const view = createView(doc, imagePreviewPlugin(), true);

    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("240px");
    expect(sourceInput(view)).toBeNull();
  });

  it("clamps an authored image width below the rendered minimum", () => {
    const doc = "![Synthetic](./assets/mock.png){width=12px}";
    const view = createView(doc);

    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("17px");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("retains an oversized authored image width in source", () => {
    const doc = "![Synthetic](./assets/mock.png){width=4000px}";
    const view = createView(doc);

    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("4000px");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it.each([
    ["![a](x.png)", 100, 420, "![a](x.png){width=420px}"],
    [
      "![a](x.png){width=320px}",
      320,
      420,
      "![a](x.png){width=420px}",
    ],
    [
      "![a](x.png){#hero width=320px data-x=yes}",
      320,
      420,
      "![a](x.png){#hero width=420px data-x=yes}",
    ],
  ])(
    "persists one image resize for %s",
    (doc, startWidth, targetWidth, expected) => {
      const transactions = observeDocumentTransactions();
      const view = createView(
        doc,
        imagePreviewPlugin(),
        false,
        transactions.extension,
      );

      dragImage(view, startWidth, targetWidth);

      expect(view.state.doc.toString()).toBe(expected);
      expect(transactions.count).toBe(1);
      expect(
        view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
      ).toBe("420px");
    },
  );

  it("removes an authored width when resized within eight pixels of the content maximum", () => {
    const view = createView("![a](x.png){width=320px}");

    dragImage(view, 320, 492, 500);

    expect(view.state.doc.toString()).toBe("![a](x.png)");
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("");
  });

  it("undoes and redoes one resize transaction with its authoritative frame width", () => {
    const original = "![a](x.png){width=320px}";
    const resized = "![a](x.png){width=420px}";
    const view = createView(original);

    dragImage(view, 320, 420);
    expect(view.state.doc.toString()).toBe(resized);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(original);
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("320px");
    expect(redo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(resized);
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("420px");
  });

  it("keeps the resized image selected with a cursor inside its new owned range", () => {
    const original = "![a](x.png){width=100px}";
    const resized = "![a](x.png){width=140px}";
    const view = createView(original);

    dragImage(view, 100, 140);

    expect(view.state.doc.toString()).toBe(resized);
    expect(view.dom.querySelector(".markra-image-node-selected")).not.toBeNull();
    expect(sourceInput(view)?.value).toBe(resized);
    expect(view.dom.querySelector(".markra-image-resize-hit-target")).not.toBeNull();
    expect(view.state.selection.main.head).toBeGreaterThan(0);
    expect(view.state.selection.main.head).toBeLessThan(resized.length);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(original);
  });

  it.each(["mouse", "touch", "pen"])(
    "resizes from a captured %s pointer",
    (pointerType) => {
      const view = createView("![a](x.png){width=100px}");

      const resize = dragImage(view, 100, 140, 600, pointerType);

      expect(view.state.doc.toString()).toBe("![a](x.png){width=140px}");
      expect(
        resize.handle.querySelector(".markra-image-resize-handle"),
      ).not.toBeNull();
      expect(resize.setPointerCapture).toHaveBeenCalledWith(7);
      expect(resize.releasePointerCapture).toHaveBeenCalledWith(7);
    },
  );

  it("does not persist movement below the five-pixel drag threshold", () => {
    const doc = "![a](x.png){width=100px}";
    const view = createView(doc);

    const resize = dragImage(view, 100, 104);

    expect(view.state.doc.toString()).toBe(doc);
    expect(resize.frame.style.width).toBe("100px");
  });

  it("ignores move and up events from a different pointer", () => {
    const doc = "![a](x.png){width=100px}";
    const view = createView(doc);
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });

    dispatchPointerEvent(resize.handle, "pointermove", {
      clientX: 180,
      pointerId: 11,
    });
    dispatchPointerEvent(resize.handle, "pointerup", {
      clientX: 180,
      pointerId: 11,
    });

    expect(view.state.doc.toString()).toBe(doc);
    expect(resize.frame.style.width).toBe("100px");
    dispatchPointerEvent(resize.handle, "pointercancel", { clientX: 100 });
  });

  it.each([
    ["non-primary pointer", { isPrimary: false }],
    ["secondary mouse button", { button: 2 }],
  ])("ignores a %s resize start", (_label, options) => {
    const doc = "![a](x.png){width=100px}";
    const view = createView(doc);
    const resize = prepareImageResize(view, 100);

    const down = dispatchPointerEvent(resize.handle, "pointerdown", {
      clientX: 100,
      ...options,
    });
    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });
    dispatchPointerEvent(resize.handle, "pointerup", { clientX: 180 });

    expect(down.defaultPrevented).toBe(false);
    expect(resize.setPointerCapture).not.toHaveBeenCalled();
    expect(resize.releasePointerCapture).not.toHaveBeenCalled();
    expect(resize.frame.style.width).toBe("100px");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("clamps transient and persisted resize width to the image minimum", () => {
    const view = createView("![a](x.png){width=100px}");
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });

    dispatchPointerEvent(resize.handle, "pointermove", { clientX: -500 });
    expect(resize.frame.style.width).toBe("17px");
    expect(view.state.doc.toString()).toBe("![a](x.png){width=100px}");
    dispatchPointerEvent(resize.handle, "pointerup", { clientX: -500 });

    expect(view.state.doc.toString()).toBe("![a](x.png){width=17px}");
  });

  it("clamps transient resize width to the available content width", () => {
    const doc = "![a](x.png){width=100px}";
    const view = createView(doc);
    const resize = prepareImageResize(view, 100, 420);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });

    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 900 });

    expect(resize.frame.style.width).toBe("420px");
    expect(view.state.doc.toString()).toBe(doc);
    dispatchPointerEvent(resize.handle, "pointercancel", { clientX: 900 });
  });

  it("prevents and contains only handle pointer gestures", () => {
    const view = createView("![a](x.png){width=100px}");
    const resize = prepareImageResize(view, 100);
    const bubbledPointerDown = vi.fn();
    resize.handle.parentElement?.addEventListener(
      "pointerdown",
      bubbledPointerDown,
    );

    const handleEvent = dispatchPointerEvent(resize.handle, "pointerdown", {
      clientX: 100,
    });
    expect(handleEvent.defaultPrevented).toBe(true);
    expect(bubbledPointerDown).not.toHaveBeenCalled();

    const imageEvent = dispatchPointerEvent(
      view.dom.querySelector<HTMLImageElement>(".cm-markra-image")!,
      "pointerdown",
      { clientX: 100, pointerId: 11 },
    );

    expect(imageEvent.defaultPrevented).toBe(false);
    expect(bubbledPointerDown).toHaveBeenCalledTimes(1);
  });

  it("never opens the media viewer from resize handle events", () => {
    const view = createView("![a](x.png){width=100px}");
    const resize = dragImage(view, 100, 140);

    resize.handle.dispatchEvent(new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    }));
    resize.handle.dispatchEvent(new MouseEvent("dblclick", {
      bubbles: true,
      cancelable: true,
    }));

    expect(document.querySelector(".markra-media-viewer-dialog")).toBeNull();
  });

  it("keeps source focus and outside-selection clearing around resize handle gestures", () => {
    const view = createView("![a](x.png){width=100px}");
    const resize = prepareImageResize(view, 100);

    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
    const source = sourceInput(view);
    source?.focus();
    source?.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
    }));

    expect(document.activeElement).toBe(source);
    expect(source?.isConnected).toBe(true);
    document.body.dispatchEvent(new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
    }));
    expect(source?.isConnected).toBe(false);
    dispatchPointerEvent(resize.handle, "pointercancel", { clientX: 100 });
  });

  it("does not render a resize handle in read-only mode", () => {
    const view = createView(
      "![a](x.png){width=100px}",
      imagePreviewPlugin(),
      true,
    );

    expect(view.dom.querySelector(".markra-image-resize-hit-target")).toBeNull();
  });

  it("removes the resize handle when an editable view becomes read-only", () => {
    const view = createView("![a](x.png){width=100px}");
    const editableRoot = view.dom.querySelector(".markra-image-node");
    expect(editableRoot?.querySelector(".markra-image-resize-hit-target"))
      .not.toBeNull();

    setViewReadOnly(view, true);

    const readOnlyRoot = view.dom.querySelector(".markra-image-node");
    expect(readOnlyRoot).not.toBe(editableRoot);
    expect(readOnlyRoot?.querySelector(".markra-image-resize-hit-target"))
      .toBeNull();
  });

  it("adds the resize handle when a read-only view becomes editable", () => {
    const view = createView(
      "![a](x.png){width=100px}",
      imagePreviewPlugin(),
      true,
    );
    const readOnlyRoot = view.dom.querySelector(".markra-image-node");
    expect(readOnlyRoot?.querySelector(".markra-image-resize-hit-target"))
      .toBeNull();

    setViewReadOnly(view, false);

    const editableRoot = view.dom.querySelector(".markra-image-node");
    expect(editableRoot).not.toBe(readOnlyRoot);
    expect(editableRoot?.querySelector(".markra-image-resize-hit-target"))
      .not.toBeNull();
  });

  it.each(["pointercancel", "lostpointercapture"] as const)(
    "restores authored width without persisting on %s",
    (eventType) => {
      const doc = "![a](x.png){width=100px}";
      const transactions = observeDocumentTransactions();
      const view = createView(
        doc,
        imagePreviewPlugin(),
        false,
        transactions.extension,
      );
      const resize = prepareImageResize(view, 100);
      dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
      dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });
      expect(resize.frame.style.width).toBe("180px");

      dispatchPointerEvent(resize.handle, eventType, { clientX: 180 });

      expect(view.state.doc.toString()).toBe(doc);
      expect(resize.frame.style.width).toBe("100px");
      expect(transactions.count).toBe(0);
    },
  );

  it("cancels an active resize when editor focus leaves", () => {
    const doc = "![a](x.png){width=100px}";
    const transactions = observeDocumentTransactions();
    const view = createView(
      doc,
      imagePreviewPlugin(),
      false,
      transactions.extension,
    );
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });
    expect(resize.frame.style.width).toBe("180px");

    view.contentDOM.dispatchEvent(new FocusEvent("focusout", {
      bubbles: true,
      relatedTarget: document.body,
    }));

    expect(resize.frame.style.width).toBe("100px");
    expect(resize.releasePointerCapture).toHaveBeenCalledWith(7);
    expect(view.state.doc.toString()).toBe(doc);
    transactions.reset();
    dispatchPointerEvent(resize.handle, "pointerup", { clientX: 180 });
    expect(view.state.doc.toString()).toBe(doc);
    expect(transactions.count).toBe(0);
  });

  it("restores authored width without dispatch when a dragged widget is destroyed", () => {
    const doc = "![a](x.png){width=100px}";
    const transactions = observeDocumentTransactions();
    const view = createView(
      doc,
      imagePreviewPlugin(),
      false,
      transactions.extension,
    );
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });
    expect(resize.frame.style.width).toBe("180px");

    view.destroy();
    views.splice(views.indexOf(view), 1);

    expect(view.state.doc.toString()).toBe(doc);
    expect(resize.frame.style.width).toBe("100px");
    expect(transactions.count).toBe(0);
  });

  it("cancels a pending resize when the document changes during the drag", () => {
    const doc = "![a](x.png){width=100px}\nEdit";
    const transactions = observeDocumentTransactions();
    const view = createView(
      doc,
      imagePreviewPlugin(),
      false,
      transactions.extension,
    );
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });

    view.dispatch({
      changes: { from: doc.length, insert: "!" },
      userEvent: "input",
    });
    expect(resize.frame.style.width).toBe("100px");
    expect(resize.releasePointerCapture).toHaveBeenCalledWith(7);
    transactions.reset();
    dispatchPointerEvent(resize.handle, "pointerup", { clientX: 180 });

    expect(view.state.doc.toString()).toBe(`${doc}!`);
    expect(resize.frame.style.width).toBe("100px");
    expect(transactions.count).toBe(0);
  });

  it("cancels a transient resize before applying an updated image source width", () => {
    const doc = "![a](x.png){width=100px}";
    const view = createView(doc);
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });

    const widthFrom = doc.indexOf("100px");
    view.dispatch({
      changes: { from: widthFrom, insert: "140px", to: widthFrom + 5 },
      userEvent: "input",
    });
    dispatchPointerEvent(resize.handle, "pointerup", { clientX: 180 });

    expect(view.state.doc.toString()).toBe("![a](x.png){width=140px}");
    expect(resize.frame.style.width).toBe("140px");
  });

  it("owns adjacent image attributes in the preview decoration", () => {
    const doc = "![Synthetic](./assets/mock.png){width=420px}\n\nEdit";
    const view = createView(doc);
    const image = view.dom.querySelector<HTMLImageElement>(".cm-markra-image");

    expect(firstLine(view)).toBe("");
    expect(image?.closest(".cm-markra-image-line")).not.toBeNull();
  });

  it("preserves unknown and invalid owned image attributes in editable source", () => {
    const markdown =
      "![Synthetic](./assets/mock.png){#hero width=1.5px data-x=yes}";
    const view = createView(`${markdown}\n\nEdit`);

    clickImage(view);

    expect(sourceInput(view)?.value).toBe(markdown);
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("");
  });

  it.each([
    ["non-safe", "9007199254740992"],
    ["non-finite", "9".repeat(400)],
  ])("preserves and deterministically renders a %s authored width", (_label, width) => {
    const doc = `![Synthetic](./assets/mock.png){#hero width=${width}px}`;
    const firstView = createView(doc);

    expect(
      firstView.dom.querySelector<HTMLElement>(".markra-image-frame")
        ?.style.width,
    ).toBe("");
    clickImage(firstView);
    expect(sourceInput(firstView)?.value).toBe(doc);

    firstView.destroy();
    views.splice(views.indexOf(firstView), 1);
    const recreatedView = createView(doc);
    expect(
      recreatedView.dom.querySelector<HTMLElement>(".markra-image-frame")
        ?.style.width,
    ).toBe("");
    expect(recreatedView.state.doc.toString()).toBe(doc);
  });

  it("deletes the complete owned image attribute range", () => {
    const doc = "![Synthetic](./assets/mock.png){width=420px}\n\nEdit";
    const view = createView(doc);

    clickImage(view);
    const event = pressSelectedKey(view, "Delete");

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe("\n\nEdit");
  });

  it("moves below the complete owned image attribute range on Enter", () => {
    const markdown = "![Synthetic](./assets/mock.png){width=420px}";
    const doc = `${markdown}\nFollowing`;
    const view = createView(doc);

    clickImage(view);
    const event = pressSelectedKey(view, "Enter");

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe(doc);
    expect(view.state.selection.main.head).toBe(markdown.length + 1);
  });

  it("leaves whitespace-separated image attributes as ordinary text", () => {
    const doc = "![Synthetic](./assets/mock.png) {width=420px}\n\nEdit";
    const view = createView(doc);

    expect(firstLine(view)).toBe(" {width=420px}");
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("");
    clickImage(view);
    expect(sourceInput(view)?.value).toBe(
      "![Synthetic](./assets/mock.png)",
    );
  });

  it("keeps authored image width stable across editor recreation", () => {
    const doc = "![Synthetic](./assets/mock.png){width=320px}\n\nEdit";
    const firstView = createView(doc);
    const firstWidth = firstView.dom.querySelector<HTMLElement>(
      ".markra-image-frame",
    )?.style.width;

    firstView.destroy();
    views.splice(views.indexOf(firstView), 1);
    const recreatedView = createView(doc);

    expect(firstWidth).toBe("320px");
    expect(
      recreatedView.dom.querySelector<HTMLElement>(".markra-image-frame")
        ?.style.width,
    ).toBe(firstWidth);
    expect(recreatedView.state.doc.toString()).toBe(doc);
  });

  it("accepts an attribute-aware image source edit", () => {
    const doc = "![Synthetic](./assets/mock.png){width=320px}\n\nEdit";
    const view = createView(doc);
    clickImage(view);
    const source = sourceInput(view);

    expect(source).not.toBeNull();
    if (!source) return;
    source.value =
      "![Changed](https://example.test/changed.png){#hero width=360px}";
    source.dispatchEvent(new Event("input", { bubbles: true }));

    expect(view.state.doc.toString()).toBe(
      "![Changed](https://example.test/changed.png){#hero width=360px}\n\nEdit",
    );
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("360px");
  });

  it("accepts image source attributes with quoted spaces using the parser grammar", () => {
    const doc = "![Synthetic](./assets/mock.png){width=320px}";
    const submitted =
      `![Changed](https://example.test/changed.png){title="wide hero" width=360px}`;
    const view = createView(doc);
    clickImage(view);
    const source = sourceInput(view);

    expect(source).not.toBeNull();
    if (!source) return;
    source.value = submitted;
    source.dispatchEvent(new Event("input", { bubbles: true }));

    expect(view.state.doc.toString()).toBe(submitted);
    expect(
      view.dom.querySelector<HTMLElement>(".markra-image-frame")?.style.width,
    ).toBe("360px");
  });

  it.each([
    [
      "leading whitespace",
      " ![Changed](https://example.test/changed.png){width=360px}",
    ],
    [
      "trailing whitespace",
      "![Changed](https://example.test/changed.png){width=360px} ",
    ],
    [
      "leading text",
      "prefix ![Changed](https://example.test/changed.png){width=360px}",
    ],
    [
      "trailing text",
      "![Changed](https://example.test/changed.png){width=360px} suffix",
    ],
  ])("rejects and preserves an image source edit with %s", (_label, submitted) => {
    const doc = "![Synthetic](./assets/mock.png){width=320px}\n\nEdit";
    const view = createView(doc);
    clickImage(view);
    const source = sourceInput(view);

    expect(source).not.toBeNull();
    if (!source) return;
    source.value = submitted;
    source.dispatchEvent(new Event("input", { bubbles: true }));

    expect(source.value).toBe(submitted);
    expect(view.state.doc.toString()).toBe(doc);
    expect(view.dom.querySelector(".markra-image-node-source-invalid"))
      .not.toBeNull();
  });

  it("deletes the complete owned image for an all-whitespace source edit", () => {
    const doc = "![Synthetic](./assets/mock.png){width=320px}\n\nEdit";
    const view = createView(doc);
    clickImage(view);
    const source = sourceInput(view);

    expect(source).not.toBeNull();
    if (!source) return;
    source.value = " \t ";
    source.dispatchEvent(new Event("input", { bubbles: true }));

    expect(view.state.doc.toString()).toBe("\n\nEdit");
    expect(view.dom.querySelector(".cm-markra-image")).toBeNull();
  });

  it("rejects a malformed image attribute source edit", () => {
    const doc = "![Synthetic](./assets/mock.png){width=320px}\n\nEdit";
    const view = createView(doc);
    clickImage(view);
    const source = sourceInput(view);

    expect(source).not.toBeNull();
    if (!source) return;
    source.value = "![Changed](https://example.test/changed.png){width}";
    source.dispatchEvent(new Event("input", { bubbles: true }));

    expect(view.state.doc.toString()).toBe(doc);
    expect(view.dom.querySelector(".markra-image-node-source-invalid"))
      .not.toBeNull();
  });

  it("does not reload an unchanged image while editing text above it", () => {
    const sourceSetter = vi.spyOn(
      HTMLImageElement.prototype,
      "src",
      "set",
    );
    try {
      const doc = [
        "Edit here",
        "",
        "![Synthetic alt](https://example.test/image.png)",
      ].join("\n");
      const view = createView(doc);
      const image = view.dom.querySelector<HTMLImageElement>(
        ".cm-markra-image",
      );
      const requestMeasure = vi.spyOn(view, "requestMeasure");
      const baselineView = createView(doc, null);
      const baselineRequestMeasure = vi.spyOn(
        baselineView,
        "requestMeasure",
      );
      sourceSetter.mockClear();
      requestMeasure.mockClear();
      baselineRequestMeasure.mockClear();

      baselineView.dispatch({
        changes: { from: "Edit here".length, insert: "!" },
        selection: { anchor: "Edit here!".length },
        userEvent: "input",
      });
      view.dispatch({
        changes: { from: "Edit here".length, insert: "!" },
        selection: { anchor: "Edit here!".length },
        userEvent: "input",
      });

      expect(view.dom.querySelector(".cm-markra-image")).toBe(image);
      expect(sourceSetter).not.toHaveBeenCalled();
      expect(requestMeasure).toHaveBeenCalledTimes(
        baselineRequestMeasure.mock.calls.length,
      );
    } finally {
      sourceSetter.mockRestore();
    }
  });

  it("marks a standalone Markdown image line for block layout", () => {
    const doc = "![Synthetic alt](https://example.test/image.png)\n\nEdit";
    const view = createView(doc);
    const image = view.dom.querySelector<HTMLImageElement>(".cm-markra-image");

    expect(image).not.toBeNull();
    expect(image?.closest(".cm-markra-image-line")).not.toBeNull();
  });

  it("keeps the preview visible and shows editable Markdown source when selected", () => {
    const doc = "Before ![Synthetic alt](./assets/mock.png) after\n\nEdit";
    const view = createView(doc);
    const image = view.dom.querySelector<HTMLImageElement>(".cm-markra-image");

    expect(image).not.toBeNull();
    image?.dispatchEvent(new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    }));

    expect(view.dom.querySelector(".cm-markra-image")).not.toBeNull();
    expect(view.dom.querySelector(".markra-image-node-selected")).not.toBeNull();
    expect(
      view.dom.querySelector<HTMLInputElement>(".markra-image-node-source")
        ?.value,
    ).toBe("![Synthetic alt](./assets/mock.png)");
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("clears a click-selected image when Escape comes from editor focus", () => {
    const doc = "Before ![Synthetic alt](./assets/mock.png) after";
    const view = createView(doc);
    clickImage(view);
    expect(view.dom.querySelector(".markra-image-node-selected")).not.toBeNull();

    const event = pressSelectedKey(view, "Escape");

    expect(event.defaultPrevented).toBe(true);
    expect(view.dom.querySelector(".markra-image-node-selected")).toBeNull();
    expect(sourceInput(view)).toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("cancels an active resize when Escape comes from editor focus", () => {
    const doc = "![a](x.png){width=100px}";
    const transactions = observeDocumentTransactions();
    const view = createView(
      doc,
      imagePreviewPlugin(),
      false,
      transactions.extension,
    );
    const resize = prepareImageResize(view, 100);
    dispatchPointerEvent(resize.handle, "pointerdown", { clientX: 100 });
    dispatchPointerEvent(resize.handle, "pointermove", { clientX: 180 });
    expect(resize.frame.style.width).toBe("180px");

    const event = pressSelectedKey(view, "Escape");

    expect(event.defaultPrevented).toBe(true);
    expect(resize.frame.style.width).toBe("100px");
    expect(resize.releasePointerCapture).toHaveBeenCalledWith(7);
    expect(view.state.doc.toString()).toBe(doc);
    expect(transactions.count).toBe(0);
    dispatchPointerEvent(resize.handle, "pointerup", { clientX: 180 });
    expect(view.state.doc.toString()).toBe(doc);
    expect(transactions.count).toBe(0);
  });

  it("moves Escape selection outside image ownership across unrelated updates", () => {
    const image = "![a](x.png){width=100px}";
    const doc = `${image}\nFollowing`;
    const view = createView(doc);
    clickImage(view);
    expect(view.state.selection.main.head).toBeGreaterThan(0);
    expect(view.state.selection.main.head).toBeLessThan(image.length);

    pressSelectedKey(view, "Escape");

    expect(view.state.selection.main.head).toBe(image.length);
    expect(view.dom.querySelector(".markra-image-node-selected")).toBeNull();
    view.dispatch({
      changes: { from: doc.length, insert: "!" },
      userEvent: "input",
    });
    expect(view.state.selection.main.head).toBe(image.length);
    expect(view.dom.querySelector(".markra-image-node-selected")).toBeNull();
    expect(sourceInput(view)).toBeNull();
  });

  it("opens an image in the shared media viewer from the enlarge button or a double click", () => {
    const doc =
      '![Synthetic detail](https://example.test/detail.png "Detailed preview")\n\nEdit';
    const view = createView(doc);
    const image = view.dom.querySelector<HTMLImageElement>(".cm-markra-image");
    const enlargeButton = view.dom.querySelector<HTMLButtonElement>(
      ".markra-image-viewer-button",
    );

    expect(image).not.toBeNull();
    expect(enlargeButton?.ariaLabel).toBe("Enlarge image");
    expect(enlargeButton?.querySelector("svg")).not.toBeNull();
    expect(image?.parentElement).toBe(enlargeButton?.parentElement);
    expect(image?.parentElement?.classList).toContain("markra-image-frame");

    enlargeButton?.click();
    let dialog = document.querySelector<HTMLElement>(
      ".markra-media-viewer-dialog",
    );
    let enlargedImage = dialog?.querySelector<HTMLImageElement>(
      ".markra-media-viewer-image",
    );

    expect(dialog?.getAttribute("role")).toBe("dialog");
    expect(dialog?.ariaLabel).toBe("Enlarged image");
    expect(enlargedImage?.getAttribute("src")).toBe(
      "https://example.test/detail.png",
    );
    expect(enlargedImage?.alt).toBe("Synthetic detail");
    expect(
      dialog?.querySelector(".markra-media-viewer-zoom-in-button"),
    ).not.toBeNull();

    dialog
      ?.querySelector<HTMLButtonElement>(".markra-media-viewer-close-button")
      ?.click();
    expect(document.querySelector(".markra-media-viewer-dialog")).toBeNull();

    image?.dispatchEvent(new MouseEvent("dblclick", {
      bubbles: true,
      cancelable: true,
    }));
    dialog = document.querySelector<HTMLElement>(
      ".markra-media-viewer-dialog",
    );
    enlargedImage = dialog?.querySelector<HTMLImageElement>(
      ".markra-media-viewer-image",
    );

    expect(dialog).not.toBeNull();
    expect(enlargedImage?.getAttribute("src")).toBe(
      "https://example.test/detail.png",
    );
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("opens the viewer in read-only mode without revealing editable source", () => {
    const doc = "![Synthetic detail](https://example.test/detail.png)";
    const view = createView(doc, imagePreviewPlugin(), true);
    const image = view.dom.querySelector<HTMLImageElement>(".cm-markra-image");

    image?.dispatchEvent(new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    }));
    expect(view.dom.querySelector(".markra-image-node-source")).toBeNull();

    image?.dispatchEvent(new MouseEvent("dblclick", {
      bubbles: true,
      cancelable: true,
    }));
    expect(document.querySelector(".markra-media-viewer-dialog")).not.toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("keeps one active viewer and closes it when the shown image changes", () => {
    const firstSource = "https://example.test/first.png";
    const secondSource = "https://example.test/second.png";
    const doc = [
      `![First synthetic image](${firstSource})`,
      "",
      `![Second synthetic image](${secondSource})`,
    ].join("\n");
    const view = createView(doc);
    const images = view.dom.querySelectorAll<HTMLImageElement>(
      ".cm-markra-image",
    );

    images[0]?.dispatchEvent(new MouseEvent("dblclick", {
      bubbles: true,
      cancelable: true,
    }));
    expect(
      document.querySelector<HTMLImageElement>(".markra-media-viewer-image")
        ?.getAttribute("src"),
    ).toBe(firstSource);

    images[1]?.dispatchEvent(new MouseEvent("dblclick", {
      bubbles: true,
      cancelable: true,
    }));
    expect(document.querySelectorAll(".markra-media-viewer-dialog")).toHaveLength(1);
    expect(
      document.querySelector<HTMLImageElement>(".markra-media-viewer-image")
        ?.getAttribute("src"),
    ).toBe(secondSource);

    const sourceFrom = view.state.doc.toString().indexOf(secondSource);
    view.dispatch({
      changes: {
        from: sourceFrom,
        to: sourceFrom + secondSource.length,
        insert: "https://example.test/updated.png",
      },
      userEvent: "input",
    });

    expect(document.querySelector(".markra-media-viewer-dialog")).toBeNull();
  });

  it("closes an active viewer when its editor is destroyed", () => {
    const view = createView(
      "![Synthetic detail](https://example.test/detail.png)",
    );
    view.dom.querySelector<HTMLImageElement>(".cm-markra-image")?.dispatchEvent(
      new MouseEvent("dblclick", { bubbles: true, cancelable: true }),
    );
    expect(document.querySelector(".markra-media-viewer-dialog")).not.toBeNull();

    view.destroy();
    views.splice(views.indexOf(view), 1);

    expect(document.querySelector(".markra-media-viewer-dialog")).toBeNull();
  });

  it("updates and deletes an image through its inline Markdown source", () => {
    const doc = "![Synthetic alt](./assets/mock.png)\n\nEdit";
    const view = createView(doc);
    view.dom.querySelector<HTMLImageElement>(".cm-markra-image")?.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true }),
    );
    const source = view.dom.querySelector<HTMLInputElement>(
      ".markra-image-node-source",
    );

    expect(source).not.toBeNull();
    if (!source) return;

    source.focus();
    source.value = "![Changed](https://example.test/changed.png)";
    source.dispatchEvent(new Event("input", { bubbles: true }));
    expect(view.state.doc.toString()).toBe(
      "![Changed](https://example.test/changed.png)\n\nEdit",
    );
    expect(
      view.dom.querySelector<HTMLImageElement>(".cm-markra-image")?.src,
    ).toBe("https://example.test/changed.png");

    const updatedSource = view.dom.querySelector<HTMLInputElement>(
      ".markra-image-node-source",
    );
    expect(updatedSource).not.toBeNull();
    if (!updatedSource) return;
    updatedSource.value = "";
    updatedSource.dispatchEvent(new Event("input", { bubbles: true }));

    expect(view.state.doc.toString()).toBe("\n\nEdit");
    expect(view.dom.querySelector(".cm-markra-image")).toBeNull();
  });

  it("moves into the blank line after an edited image on Enter", () => {
    const doc = "![Synthetic alt](./assets/mock.png)\n\nEdit";
    const view = createView(doc);
    view.dom.querySelector<HTMLImageElement>(".cm-markra-image")?.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true }),
    );
    const source = view.dom.querySelector<HTMLInputElement>(
      ".markra-image-node-source",
    );

    expect(source).not.toBeNull();
    source?.dispatchEvent(new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "Enter",
    }));

    expect(view.state.doc.toString()).toBe(doc);
    expect(view.state.selection.main.head).toBe(doc.indexOf("\n") + 1);
    expect(view.dom.querySelector(".markra-image-node-source")).toBeNull();
  });

  it("moves below a selected image when Enter comes from the editor", () => {
    const imageMarkdown = "![Synthetic alt](./assets/mock.png)";
    const doc = `${imageMarkdown}\nFollowing`;
    const view = createView(doc);
    view.dom.querySelector<HTMLImageElement>(".cm-markra-image")?.dispatchEvent(
      new MouseEvent("click", { bubbles: true, cancelable: true }),
    );
    const event = new KeyboardEvent("keydown", {
      bubbles: true,
      cancelable: true,
      key: "Enter",
    });

    expect(view.state.selection.main.head).toBe(1);
    view.contentDOM.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(view.state.doc.toString()).toBe(doc);
    expect(view.state.selection.main.head).toBe(imageMarkdown.length + 1);
    expect(view.dom.querySelector(".markra-image-node-source")).toBeNull();

    view.dispatch({
      changes: { from: imageMarkdown.length + 1, insert: "Plain " },
      selection: { anchor: imageMarkdown.length + 7 },
      userEvent: "input",
    });
    expect(view.state.doc.toString()).toBe(
      `${imageMarkdown}\nPlain Following`,
    );
    expect(view.dom.querySelector(".cm-markra-link")).toBeNull();
  });

  it("rejects executable and local protocols by default", () => {
    const executable = createView(
      "![Unsafe](javascript:alert%281%29)\n\nEdit",
    );
    const local = createView("![Local](file:///mock/private.png)\n\nEdit");

    expect(executable.dom.querySelector(".cm-markra-image")).toBeNull();
    expect(local.dom.querySelector(".cm-markra-image")).toBeNull();
  });

  it("allows common browser image sources but rejects SVG data URLs", () => {
    expect(resolveSafeImageSource("./assets/mock.png")).toBe(
      "./assets/mock.png",
    );
    expect(resolveSafeImageSource("blob:https://example.test/mock")).toBe(
      "blob:https://example.test/mock",
    );
    expect(resolveSafeImageSource("data:image/png;base64,iVBORw0KGgo=")).toBe(
      "data:image/png;base64,iVBORw0KGgo=",
    );
    expect(
      resolveSafeImageSource("data:image/svg+xml,%3Csvg%3E%3C/svg%3E"),
    ).toBeNull();
  });

  it("lets QingYu resolve application assets through a host callback", () => {
    const resolveSource = vi.fn((context) =>
      context.source.startsWith("markra://")
        ? "https://assets.example.test/mock.png"
        : null,
    );
    const view = createView(
      '![Asset](markra://images/mock.png "Asset preview")\n\nEdit',
      imagePreviewPlugin({
        className: "markra-image",
        resolveSource,
      }),
    );
    const image = view.dom.querySelector<HTMLImageElement>(".markra-image");

    expect(resolveSource).toHaveBeenCalledWith(
      expect.objectContaining({
        alt: "Asset",
        source: "markra://images/mock.png",
        title: "Asset preview",
        view,
      }),
    );
    expect(image?.getAttribute("src")).toBe(
      "https://assets.example.test/mock.png",
    );
  });
});
