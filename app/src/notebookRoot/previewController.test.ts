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
