import assert = require("node:assert/strict");
import test from "node:test";

class FakeResizeObserver {
    public static instances: FakeResizeObserver[] = [];
    public observed: Element[] = [];
    public disconnected = false;

    constructor(private readonly callback: ResizeObserverCallback) {
        FakeResizeObserver.instances.push(this);
    }

    public observe(element: Element) {
        this.observed.push(element);
    }

    public disconnect() {
        this.disconnected = true;
        this.observed = [];
    }

    public trigger() {
        this.callback([], this as unknown as ResizeObserver);
    }
}

const withDom = async (run: (document: Document) => Promise<void>) => {
    const {JSDOM} = await import("jsdom");
    const dom = new JSDOM("<!doctype html><body></body>");
    const previousDocument = globalThis.document;
    const previousResizeObserver = globalThis.ResizeObserver;
    const previousRequestAnimationFrame = globalThis.requestAnimationFrame;
    const previousCancelAnimationFrame = globalThis.cancelAnimationFrame;
    let nextFrame = 1;
    const frames = new Map<number, FrameRequestCallback>();
    FakeResizeObserver.instances = [];
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    Object.defineProperty(globalThis, "ResizeObserver", {configurable: true, value: FakeResizeObserver});
    Object.defineProperty(globalThis, "requestAnimationFrame", {
        configurable: true,
        value: (callback: FrameRequestCallback) => {
            const id = nextFrame++;
            frames.set(id, callback);
            return id;
        },
    });
    Object.defineProperty(globalThis, "cancelAnimationFrame", {
        configurable: true,
        value: (id: number) => frames.delete(id),
    });
    try {
        await run(dom.window.document);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previousDocument});
        Object.defineProperty(globalThis, "ResizeObserver", {configurable: true, value: previousResizeObserver});
        Object.defineProperty(globalThis, "requestAnimationFrame", {
            configurable: true,
            value: previousRequestAnimationFrame,
        });
        Object.defineProperty(globalThis, "cancelAnimationFrame", {
            configurable: true,
            value: previousCancelAnimationFrame,
        });
        dom.window.close();
    }
    return frames;
};

const createDocuments = (document: Document, width: number, ratios: number[]) => {
    const documents = document.createElement("main") as unknown as HTMLElement;
    documents.className = "notebook-root__documents notebook-root__documents--masonry";
    Object.defineProperty(documents, "clientWidth", {configurable: true, value: width});
    ratios.forEach((ratio, index) => {
        const card = document.createElement("article");
        card.className = "notebook-root__document notebook-root__document--masonry";
        card.dataset.id = `document-${index}`;
        card.dataset.cardRatio = String(ratio);
        documents.append(card);
    });
    document.body.append(documents as unknown as Node);
    return documents;
};

test("masonry controller positions cards without changing DOM order", async () => {
    await withDom(async (document) => {
        const {NotebookRootMasonryController} = await import("./masonryController");
        const documents = createDocuments(document, 900, [2, 1, 3, 1, 1]);
        const originalCards = Array.from(documents.children);
        const controller = new NotebookRootMasonryController(documents);
        controller.layoutNow();
        const cards = Array.from(documents.querySelectorAll<HTMLElement>("article"));
        assert.deepEqual(Array.from(documents.children), originalCards);
        assert.equal(cards[0].style.left, "16px");
        assert.equal(cards[4].style.left, cards[1].style.left);
        assert.ok(Number.parseFloat(documents.style.height) > 0);
        controller.destroy();
    });
});

test("masonry controller coalesces resize frames and cancels pending work", async () => {
    const frames = await withDom(async (document) => {
        const {NotebookRootMasonryController} = await import("./masonryController");
        const documents = createDocuments(document, 900, [1]);
        const controller = new NotebookRootMasonryController(documents);
        const observer = FakeResizeObserver.instances.at(-1);
        assert.ok(observer);
        observer.trigger();
        observer.trigger();
        controller.schedule();
        controller.destroy();
        assert.equal(observer.disconnected, true);
    });
    assert.equal(frames.size, 0);
});

test("masonry controller leaves zero-width and empty containers safe", async () => {
    await withDom(async (document) => {
        const {NotebookRootMasonryController} = await import("./masonryController");
        const zeroWidth = createDocuments(document, 0, [1]);
        const zeroController = new NotebookRootMasonryController(zeroWidth);
        zeroController.layoutNow();
        assert.equal((zeroWidth.firstElementChild as HTMLElement).style.left, "");
        assert.equal(zeroWidth.style.height, "");
        zeroController.destroy();

        const empty = createDocuments(document, 900, []);
        empty.style.height = "123px";
        const emptyController = new NotebookRootMasonryController(empty);
        emptyController.layoutNow();
        assert.equal(empty.style.height, "");
        emptyController.destroy();
    });
});
