import assert = require("node:assert/strict");
import test from "node:test";

class FakeIntersectionObserver {
    constructor(public callback: IntersectionObserverCallback) {}
    public observed: Element[] = [];
    public disconnectCount = 0;

    public observe(element: Element) {
        this.observed.push(element);
    }

    public unobserve(element: Element) {
        this.observed = this.observed.filter((item) => item !== element);
    }

    public disconnect() {
        this.disconnectCount++;
        this.observed = [];
    }
}

const createCard = (document: Document, id = "document-id") => {
    const card = document.createElement("article") as unknown as HTMLElement;
    card.className = "notebook-root__document";
    card.dataset.kind = "markdown";
    card.dataset.notebook = "box";
    card.dataset.path = "/draft.md";
    card.dataset.id = id;
    card.dataset.identityState = id ? "valid" : "missing";
    card.dataset.identityConflict = "false";
    card.dataset.revision = "revision-a";
    card.innerHTML = "<div class=\"notebook-root__preview-box\"><div class=\"notebook-root__placeholder\"></div></div>";
    document.body.append(card as unknown as Node);
    return card;
};

const withDom = async (run: (dom: Awaited<ReturnType<typeof importDom>>) => Promise<void>) => {
    const dom = await importDom();
    const previousDocument = globalThis.document;
    const previousWindow = globalThis.window;
    const previousObserver = globalThis.IntersectionObserver;
    const previousFrame = globalThis.requestAnimationFrame;
    Object.defineProperty(globalThis, "requestAnimationFrame", {
        configurable: true, value: (callback: FrameRequestCallback) => globalThis.setTimeout(callback, 0),
    });
    Object.assign(dom.window, {
        siyuan: {config: {appearance: {mode: 0}, readonly: false}},
        requestIdleCallback: (callback: IdleRequestCallback) => globalThis.setTimeout(callback, 0),
    });
    Object.defineProperty(globalThis, "document", {configurable: true, value: dom.window.document});
    Object.defineProperty(globalThis, "window", {configurable: true, value: dom.window});
    Object.defineProperty(globalThis, "IntersectionObserver", {
        configurable: true,
        value: FakeIntersectionObserver,
    });
    try {
        await run(dom);
    } finally {
        Object.defineProperty(globalThis, "document", {configurable: true, value: previousDocument});
        Object.defineProperty(globalThis, "window", {configurable: true, value: previousWindow});
        Object.defineProperty(globalThis, "IntersectionObserver", {configurable: true, value: previousObserver});
        Object.defineProperty(globalThis, "requestAnimationFrame", {configurable: true, value: previousFrame});
        dom.window.close();
    }
};

const importDom = async () => {
    const {JSDOM} = await import("jsdom");
    return new JSDOM("<!doctype html><body></body>");
};

test("preview controller reuses jobs while rebinding targets", async () => {
    await withDom(async (dom) => {
        const {DocumentCardPreviewController} = await import("./previewController");
        const {notebookRootElementKey} = await import("./documentKey");
        const controller = new DocumentCardPreviewController();
        const oldCard = createCard(dom.window.document);
        controller.rebind([oldCard]);
        const key = notebookRootElementKey(oldCard);
        const internals = controller as unknown as {
            jobs: Map<string, object>;
            targets: Map<string, HTMLElement>;
        };
        const originalJob = internals.jobs.get(key);
        const newCard = createCard(dom.window.document);
        oldCard.remove();
        controller.rebind([newCard]);
        assert.strictEqual(internals.jobs.get(key), originalJob);
        assert.strictEqual(internals.targets.get(key), newCard);
        assert.equal(internals.jobs.size, 1);
        controller.destroy();
    });
});

test("in-flight image decoding installs only into the latest target", async () => {
    await withDom(async (dom) => {
        const {DocumentCardPreviewController} = await import("./previewController");
        const {notebookRootElementKey} = await import("./documentKey");
        let resolveDecode: () => void;
        const decode = new Promise<void>((resolve) => {
            resolveDecode = resolve;
        });
        Object.defineProperty(dom.window.HTMLImageElement.prototype, "decode", {
            configurable: true,
            value: () => decode,
        });
        const controller = new DocumentCardPreviewController();
        const oldCard = createCard(dom.window.document);
        controller.rebind([oldCard]);
        const key = notebookRootElementKey(oldCard);
        const internals = controller as unknown as {
            installImage: (key: string, url: string) => Promise<void>;
        };
        const installing = internals.installImage(key, "/preview.webp");
        const newCard = createCard(dom.window.document);
        oldCard.remove();
        controller.rebind([newCard]);
        resolveDecode!();
        await installing;
        assert.equal(oldCard.querySelector("img"), null);
        assert.ok(newCard.querySelector("img"));
        assert.equal(newCard.dataset.previewReady, "true");
        controller.destroy();
    });
});

test("theme refresh restores placeholders without rebuilding document cards", async () => {
    await withDom(async (dom) => {
        Object.assign(dom.window, {
            siyuan: {config: {appearance: {mode: 0, themeLight: "daylight", themeDark: "midnight", themeVer: "1"}}},
        });
        dom.window.document.documentElement.style.setProperty("--b3-theme-background", "rgb(255, 255, 255)");
        const {DocumentCardPreviewController} = await import("./previewController");
        const {notebookRootElementKey} = await import("./documentKey");
        const controller = new DocumentCardPreviewController();
        const card = createCard(dom.window.document);
        controller.rebind([card]);
        const key = notebookRootElementKey(card);
        const internals = controller as unknown as {
            currentAppearanceKey: () => Promise<string>;
            installImage: (key: string, url: string) => Promise<void>;
        };
        await internals.currentAppearanceKey();
        await internals.installImage(key, "/preview.webp");
        assert.ok(card.querySelector("img"));
        dom.window.document.documentElement.style.setProperty("--b3-theme-background", "rgb(250, 248, 240)");
        await controller.refreshAppearance();
        assert.strictEqual(card, dom.window.document.querySelector("article"));
        assert.equal(card.querySelector("img"), null);
        assert.ok(card.querySelector(".notebook-root__placeholder"));
        assert.equal(card.dataset.previewReady, undefined);
        assert.equal(card.dataset.previewState, "loading");
        controller.destroy();
    });
});

test("Markdown identity migration updates the stable job key and owning listing", async () => {
    await withDom(async (dom) => {
        const {DocumentCardPreviewController} = await import("./previewController");
        const {notebookRootDocumentKey, notebookRootElementKey} = await import("./documentKey");
        const listing = [{
            kind: "markdown" as const,
            notebook: "box",
            path: "/draft.md",
            documentID: "",
            identityState: "missing",
            identityConflict: false,
            revision: "revision-a",
        }];
        const identityUpdates: object[] = [];
        const controller = new DocumentCardPreviewController({
            onIdentityCreated: (identity) => {
                identityUpdates.push(identity);
                const document = listing.find((item) => item.notebook === identity.notebook && item.path === identity.path);
                if (!document) return;
                document.documentID = identity.documentID;
                document.identityState = "valid";
                document.identityConflict = false;
                document.revision = identity.revision;
            },
        });
        const card = createCard(dom.window.document, "");
        controller.rebind([card]);
        const temporaryKey = notebookRootElementKey(card);
        const internals = controller as unknown as {
            jobs: Map<string, {key: string}>;
            targets: Map<string, HTMLElement>;
            migrateJobIdentity: (job: object, identity: {documentID: string, revision: string}) => void;
        };
        const job = internals.jobs.get(temporaryKey);
        assert.ok(job);
        internals.migrateJobIdentity(job, {documentID: "formal-id", revision: "revision-b"});
        const formalKey = notebookRootDocumentKey({kind: "markdown", notebook: "box", id: "formal-id", path: "/draft.md"});
        assert.equal(job.key, formalKey);
        assert.strictEqual(internals.jobs.get(formalKey), job);
        assert.equal(internals.jobs.has(temporaryKey), false);
        assert.strictEqual(internals.targets.get(formalKey), card);
        assert.equal(card.dataset.id, "formal-id");
        assert.equal(card.dataset.previewKey, formalKey);
        assert.deepEqual(identityUpdates, [{
            notebook: "box",
            path: "/draft.md",
            documentID: "formal-id",
            revision: "revision-b",
        }]);
        assert.equal(notebookRootDocumentKey({
            kind: listing[0].kind,
            notebook: listing[0].notebook,
            id: listing[0].documentID,
            path: listing[0].path,
        }), formalKey);
        controller.destroy();
    });
});

test("preview controller retries one stale store with a fresh descriptor", async () => {
    await withDom(async (dom) => {
        const previousRequestAnimationFrame = globalThis.requestAnimationFrame;
        Object.defineProperty(globalThis, "requestAnimationFrame", {
            configurable: true,
            value: (callback: FrameRequestCallback) => globalThis.setTimeout(callback, 0),
        });
        Object.assign(dom.window, {
            siyuan: {config: {appearance: {mode: 0}, readonly: false}},
            requestIdleCallback: (callback: IdleRequestCallback) => globalThis.setTimeout(callback, 0),
        });
        try {
            const {DocumentCardPreviewController} = await import("./previewController");
            let prepareCount = 0;
            let storeCount = 0;
            let renderCount = 0;
            let installCount = 0;
            const controller = new DocumentCardPreviewController({
                request: async (url) => {
                    if (url.endsWith("prepareDocumentCardPreview")) {
                        prepareCount++;
                        return {
                            code: 0,
                            msg: "",
                            data: {
                                cacheKey: `cache-${prepareCount}`,
                                url: `/preview-${prepareCount}.webp`,
                                exists: false,
                                theme: "light",
                                appearanceKey: "appearance",
                                size: "medium",
                            },
                        } as IWebSocketData;
                    }
                    storeCount++;
                    return {code: storeCount === 1 ? 409 : 0, msg: "", data: null} as IWebSocketData;
                },
                renderPreview: async () => {
                    renderCount++;
                    return new Blob(["preview"], {type: "image/webp"});
                },
            });
            const card = createCard(dom.window.document);
            card.dataset.kind = "sy";
            controller.rebind([card]);
            const internals = controller as unknown as {
                jobs: Map<string, object>;
                appearanceKeyPromise: Promise<string>;
                render: (job: object) => Promise<void>;
                installImage: (key: string, url: string, generation: number) => Promise<void>;
            };
            internals.appearanceKeyPromise = Promise.resolve("appearance");
            internals.installImage = async () => {
                installCount++;
            };
            await internals.render(internals.jobs.values().next().value);
            assert.equal(prepareCount, 2);
            assert.equal(storeCount, 2);
            assert.equal(renderCount, 2);
            assert.equal(installCount, 1);
            assert.notEqual(card.dataset.previewState, "failed");
            controller.destroy();
        } finally {
            Object.defineProperty(globalThis, "requestAnimationFrame", {
                configurable: true,
                value: previousRequestAnimationFrame,
            });
        }
    });
});

const until = async (condition: () => boolean) => {
    const deadline = Date.now() + 2000;
    while (!condition() && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 2));
    assert.ok(condition(), "condition should settle before deadline");
};
const response = (exists = false) => ({code: 0, msg: "", data: {
    exists, cacheKey: "test", url: "/test.webp", theme: "light", size: "medium", appearanceKey: "test",
}} as IWebSocketData);
const intersect = (controller: any, card: HTMLElement, visible = true) => {
    controller.observer.callback([{target: card, isIntersecting: visible}]);
};
const deferred = () => {
    let release: () => void;
    const promise = new Promise<void>((resolve) => { release = resolve; });
    return {promise, release};
};

test("cached cards bypass an unfinished cold render", async () => withDom(async (dom) => {
    const {DocumentCardPreviewController} = await import("./previewController");
    const gate = deferred();
    let started = false;
    const controller = new DocumentCardPreviewController({
        request: async (url, data) => response(url.includes("prepare") && data.reference.id === "cached-bypass"),
        renderPreview: async () => { started = true; await gate.promise; return new Blob(["image"]); },
    });
    try {
        const cold = createCard(dom.window.document, "cold-bypass");
        const cached = createCard(dom.window.document, "cached-bypass");
        controller.rebind([cold, cached]);
        intersect(controller, cold);
        await until(() => started);
        intersect(controller, cached);
        await until(() => cached.dataset.previewReady === "true");
        assert.equal(cold.dataset.previewReady, undefined);
    } finally {
        controller.destroy();
        gate.release();
        await until(() => !(controller as any).activeJob);
    }
}));

test("cache probes are bounded to four and scrolling postpones cold rendering", async () => withDom(async (dom) => {
    const {DocumentCardPreviewController} = await import("./previewController");
    const gate = deferred();
    let concurrent = 0;
    let maximum = 0;
    let renders = 0;
    const controller = new DocumentCardPreviewController({
        request: async () => {
            maximum = Math.max(maximum, ++concurrent);
            await gate.promise;
            concurrent--;
            return response();
        },
        renderPreview: async () => { renders++; return new Blob(["image"]); },
    });
    try {
        const cards = Array.from({length: 8}, (_, i) => createCard(dom.window.document, `bounded-${i}`));
        controller.rebind(cards);
        cards.forEach((card) => intersect(controller, card));
        await until(() => concurrent === 4);
        dom.window.document.body.dispatchEvent(new dom.window.Event("scroll"));
        gate.release();
        await new Promise((resolve) => setTimeout(resolve, 80));
        assert.equal(renders, 0);
        await until(() => renders > 0);
        assert.equal(maximum, 4);
    } finally {
        controller.destroy();
        gate.release();
        await until(() => !(controller as any).activeJob && (controller as any).probing.size === 0);
    }
}));

test("leaving viewport cancels active work without storing", async () => withDom(async (dom) => {
    const {DocumentCardPreviewController} = await import("./previewController");
    const gate = deferred();
    let started = false;
    let stores = 0;
    const controller = new DocumentCardPreviewController({
        request: async (url) => { if (url.includes("store")) stores++; return response(); },
        renderPreview: async ({shouldContinue}) => {
            started = true;
            await gate.promise;
            assert.equal(shouldContinue(), false);
            throw new DOMException("cancelled", "AbortError");
        },
    });
    const card = createCard(dom.window.document, "leave-viewport");
    controller.rebind([card]);
    intersect(controller, card);
    await until(() => started);
    intersect(controller, card, false);
    gate.release();
    await until(() => !(controller as any).activeJob);
    assert.equal(stores, 0);
    assert.notEqual(card.dataset.previewState, "failed");
    controller.destroy();
}));

test("theme changes restart the visible active job", async () => withDom(async (dom) => {
    const {DocumentCardPreviewController} = await import("./previewController");
    const gate = deferred();
    let renders = 0;
    const controller = new DocumentCardPreviewController({
        request: async () => response(),
        renderPreview: async () => {
            if (++renders === 1) { await gate.promise; throw new DOMException("cancelled", "AbortError"); }
            return new Blob(["image"]);
        },
    });
    const card = createCard(dom.window.document, "theme-active");
    controller.rebind([card]);
    intersect(controller, card);
    await until(() => renders === 1);
    dom.window.document.documentElement.style.setProperty("--b3-theme-background", "#123456");
    await controller.refreshAppearance();
    intersect(controller, card);
    gate.release();
    await until(() => card.dataset.previewReady === "true");
    assert.equal(renders, 2);
    controller.destroy();
}));
