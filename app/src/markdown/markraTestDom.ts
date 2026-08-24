import {JSDOM} from "jsdom";

const TEST_GLOBALS = [
    "window", "document", "navigator", "MutationObserver", "ResizeObserver", "requestAnimationFrame",
    "cancelAnimationFrame", "CSSStyleSheet", "DOMParser", "Element", "Event", "FocusEvent", "HTMLElement", "KeyboardEvent", "MouseEvent", "Node", "Range", "Window",
    "HTMLImageElement", "HTMLInputElement", "HTMLTextAreaElement", "SVGElement", "getComputedStyle",
] as const;

export const installMarkdownTestDom = () => {
    const previous = new Map<string, PropertyDescriptor | undefined>();
    TEST_GLOBALS.forEach((name) => previous.set(name, Object.getOwnPropertyDescriptor(globalThis, name)));
    const dom = new JSDOM("<!doctype html><body></body>", {pretendToBeVisual: true});
    Object.defineProperties(dom.window.Range.prototype, {
        getBoundingClientRect: {
            configurable: true,
            value: () => ({bottom: 0, height: 0, left: 0, right: 0, top: 0, width: 0, x: 0, y: 0}),
        },
        getClientRects: {
            configurable: true,
            value: (): DOMRect[] => [],
        },
    });
    class TestResizeObserver {
        public observe() {}
        public unobserve() {}
        public disconnect() {}
    }
    const values: Record<(typeof TEST_GLOBALS)[number], unknown> = {
        window: dom.window,
        document: dom.window.document,
        navigator: dom.window.navigator,
        MutationObserver: dom.window.MutationObserver,
        ResizeObserver: TestResizeObserver,
        requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
        cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
        CSSStyleSheet: dom.window.CSSStyleSheet,
        DOMParser: dom.window.DOMParser,
        Element: dom.window.Element,
        Event: dom.window.Event,
        FocusEvent: dom.window.FocusEvent,
        HTMLElement: dom.window.HTMLElement,
        HTMLImageElement: dom.window.HTMLImageElement,
        HTMLInputElement: dom.window.HTMLInputElement,
        HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
        KeyboardEvent: dom.window.KeyboardEvent,
        MouseEvent: dom.window.MouseEvent,
        Node: dom.window.Node,
        Range: dom.window.Range,
        SVGElement: dom.window.SVGElement,
        Window: dom.window.Window,
        getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    };
    TEST_GLOBALS.forEach((name) => Object.defineProperty(globalThis, name, {
        configurable: true,
        value: values[name],
        writable: true,
    }));
    return () => {
        dom.window.close();
        TEST_GLOBALS.forEach((name) => {
            const descriptor = previous.get(name);
            if (descriptor) {
                Object.defineProperty(globalThis, name, descriptor);
            } else {
                delete (globalThis as Record<string, unknown>)[name];
            }
        });
    };
};
