import assert = require("node:assert/strict");
import test from "node:test";

class FakeIntersectionObserver {
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
