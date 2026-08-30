import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {EditorState} from "@codemirror/state";
import {EditorView} from "@codemirror/view";
import {MarkdownDocumentScrollController} from "./documentScroll";
import {installMarkdownTestDom} from "./markraTestDom";

let cleanup: () => void;
let view: EditorView;
let container: HTMLElement;

const waitForAnchorRestore = async () => {
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve(undefined))));
};

const rect = (top: number, bottom: number, left = 0, right = 400): DOMRect => ({
    bottom,
    height: bottom - top,
    left,
    right,
    top,
    width: right - left,
    x: left,
    y: top,
    toJSON: () => ({}),
});

beforeEach(() => {
    cleanup = installMarkdownTestDom();
    container = document.body.appendChild(document.createElement("div"));
    Object.defineProperty(container, "clientHeight", {configurable: true, value: 200});
    container.getBoundingClientRect = () => rect(20, 220);
    view = new EditorView({
        parent: container,
        state: EditorState.create({doc: "0123456789", selection: {anchor: 2}}),
    });
    Object.defineProperty(view, "inView", {configurable: true, value: true});
});

afterEach(() => {
    view.destroy();
    cleanup();
});

test("captures a visible selection as the semantic scroll anchor", () => {
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => rect(80, 100),
    });
    const controller = new MarkdownDocumentScrollController(() => view, container);

    assert.deepEqual(controller.captureAnchor(), {position: 2, viewportOffset: 60});
});

test("uses the viewport center when the selection is outside the viewport", () => {
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: (position: number) => position === 2 ? rect(300, 320) : rect(115, 135),
    });
    Object.defineProperty(view, "posAtCoords", {
        configurable: true,
        value: () => 5,
    });
    const controller = new MarkdownDocumentScrollController(() => view, container);

    assert.deepEqual(controller.captureAnchor(), {position: 5, viewportOffset: 95});
});

test("returns the last visible anchor without measuring a hidden editor", () => {
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => rect(80, 100),
    });
    const controller = new MarkdownDocumentScrollController(() => view, container);
    assert.deepEqual(controller.captureAnchor(), {position: 2, viewportOffset: 60});

    Object.defineProperty(view, "inView", {configurable: true, value: false});
    container.getBoundingClientRect = () => rect(0, 0, 0, 0);
    Object.defineProperty(view, "posAtCoords", {
        configurable: true,
        value: () => {
            throw new Error("hidden editor must not be measured");
        },
    });

    assert.deepEqual(controller.captureAnchor(), {position: 2, viewportOffset: 60});
});

test("returns null when an editor is hidden before any anchor is captured", () => {
    Object.defineProperty(view, "inView", {configurable: true, value: false});
    container.getBoundingClientRect = () => rect(0, 0, 0, 0);
    Object.defineProperty(view, "posAtCoords", {
        configurable: true,
        value: () => {
            throw new Error("hidden editor must not be measured");
        },
    });
    const controller = new MarkdownDocumentScrollController(() => view, container);

    assert.equal(controller.captureAnchor(), null);
});

test("keeps the last anchor when CodeMirror cannot resolve viewport coordinates", () => {
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => rect(80, 100),
    });
    const controller = new MarkdownDocumentScrollController(() => view, container);
    assert.deepEqual(controller.captureAnchor(), {position: 2, viewportOffset: 60});

    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => rect(300, 320),
    });
    Object.defineProperty(view, "posAtCoords", {
        configurable: true,
        value: () => {
            throw new TypeError("Cannot read properties of undefined (reading 'isText')");
        },
    });

    assert.deepEqual(controller.captureAnchor(), {position: 2, viewportOffset: 60});
});

test("uses a restored anchor without measuring its hidden editor", async () => {
    Object.defineProperty(view, "inView", {configurable: true, value: false});
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => {
            throw new Error("hidden editor must not be measured");
        },
    });
    container.scrollTop = 100;
    const controller = new MarkdownDocumentScrollController(() => view, container);

    controller.restoreAnchor({position: 7, viewportOffset: 24});

    assert.deepEqual(controller.captureAnchor(), {position: 7, viewportOffset: 24});
    await waitForAnchorRestore();
    assert.equal(container.scrollTop, 100);
});

test("restores a clamped document position at the previous viewport offset", async () => {
    let requestedPosition = -1;
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: (position: number) => {
            requestedPosition = position;
            return rect(150, 170);
        },
    });
    container.scrollTop = 100;
    const controller = new MarkdownDocumentScrollController(() => view, container);

    controller.restoreAnchor({position: 999, viewportOffset: 60});
    await waitForAnchorRestore();

    assert.equal(requestedPosition, view.state.doc.length);
    assert.equal(container.scrollTop, 170);
});

test("restores an offscreen anchor through CodeMirror document geometry", async () => {
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: (): DOMRect | null => null,
    });
    Object.defineProperty(view, "lineBlockAt", {
        configurable: true,
        value: () => ({top: 480}),
    });
    Object.defineProperty(view, "documentTop", {configurable: true, value: -80});
    container.scrollTop = 100;
    const controller = new MarkdownDocumentScrollController(() => view, container);

    controller.restoreAnchor({position: 7, viewportOffset: 60});
    await waitForAnchorRestore();

    assert.equal(container.scrollTop, 420);
});

test("cancels a pending restoration when destroyed", async () => {
    Object.defineProperty(view, "coordsAtPos", {
        configurable: true,
        value: () => rect(150, 170),
    });
    container.scrollTop = 100;
    const controller = new MarkdownDocumentScrollController(() => view, container);

    controller.restoreAnchor({position: 2, viewportOffset: 60});
    controller.destroy();
    await waitForAnchorRestore();

    assert.equal(container.scrollTop, 100);
});
