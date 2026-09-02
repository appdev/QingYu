import assert = require("node:assert/strict");
import test from "node:test";

const rect = (top: number, left = 0, height = 100) => ({
    top,
    left,
    right: left + 100,
    bottom: top + height,
    width: 100,
    height,
    x: left,
    y: top,
    toJSON: () => ({}),
});

test("layout hydration moves the same decoded image and retains missing placeholders", async () => {
    const {JSDOM} = await import("jsdom");
    const {captureNotebookRootLayoutSnapshot, hydrateNotebookRootLayout} = await import("./layoutSwitch");
    const dom = new JSDOM(`<div class="notebook-root"><main class="notebook-root__documents">
        <article class="notebook-root__document notebook-root__document--selected" data-kind="sy" data-notebook="box" data-id="one" data-path="/one.sy">
            <div class="notebook-root__preview-box"><img class="notebook-root__preview" src="/one.webp"><div class="notebook-root__image-fader"></div></div>
        </article>
        <article class="notebook-root__document" data-kind="sy" data-notebook="box" data-id="two" data-path="/two.sy"><div class="notebook-root__preview-box"><div class="notebook-root__placeholder"></div></div></article>
    </main></div>`);
    const root = dom.window.document.querySelector(".notebook-root") as unknown as HTMLElement;
    const oldDocuments = dom.window.document.querySelector("main") as unknown as HTMLElement;
    Object.defineProperty(root, "getBoundingClientRect", {value: () => rect(0, 0, 500)});
    oldDocuments.querySelectorAll<HTMLElement>("article").forEach((element, index) => {
        Object.defineProperty(element, "getBoundingClientRect", {value: () => rect(index * 120)});
    });
    const image = oldDocuments.querySelector("img") as unknown as HTMLImageElement;
    const snapshot = captureNotebookRootLayoutSnapshot(root, oldDocuments);
    const next = dom.window.document.createElement("main") as unknown as HTMLElement;
    next.innerHTML = `<article class="notebook-root__document" data-kind="sy" data-notebook="box" data-id="one" data-path="/one.sy"><div class="notebook-root__preview-box"><div class="notebook-root__placeholder"></div><div class="notebook-root__image-fader"></div></div></article>
        <article class="notebook-root__document" data-kind="sy" data-notebook="box" data-id="two" data-path="/two.sy"><div class="notebook-root__preview-box"><div class="notebook-root__placeholder"></div></div></article>`;
    hydrateNotebookRootLayout(next, snapshot);
    const first = next.querySelectorAll<HTMLElement>("article")[0];
    const second = next.querySelectorAll<HTMLElement>("article")[1];
    assert.strictEqual(first.querySelector("img"), image);
    assert.equal(first.querySelector(".notebook-root__placeholder"), null);
    assert.equal(first.dataset.previewReady, "true");
    assert.equal(first.dataset.previewState, "ready");
    assert.equal(first.classList.contains("notebook-root__document--selected"), true);
    assert.ok(second.querySelector(".notebook-root__placeholder"));
    dom.window.close();
});

test("layout snapshots use spatial order and restore the document anchor", async () => {
    const {JSDOM} = await import("jsdom");
    const {captureNotebookRootLayoutSnapshot, restoreNotebookRootScrollAnchor} = await import("./layoutSwitch");
    const dom = new JSDOM(`<div class="notebook-root"><main>
        <article class="notebook-root__document" data-kind="sy" data-notebook="box" data-id="one"></article>
        <article class="notebook-root__document" data-kind="sy" data-notebook="box" data-id="two"></article>
        <article class="notebook-root__document" data-kind="sy" data-notebook="box" data-id="three"></article>
    </main></div>`);
    const root = dom.window.document.querySelector(".notebook-root") as unknown as HTMLElement;
    const documents = dom.window.document.querySelector("main") as unknown as HTMLElement;
    const cards = documents.querySelectorAll<HTMLElement>("article");
    Object.defineProperty(root, "getBoundingClientRect", {value: () => rect(100, 0, 500)});
    Object.defineProperty(root, "scrollTop", {configurable: true, writable: true, value: 500});
    Object.defineProperty(cards[0], "getBoundingClientRect", {configurable: true, value: () => rect(220, 0)});
    Object.defineProperty(cards[1], "getBoundingClientRect", {configurable: true, value: () => rect(124, 80)});
    Object.defineProperty(cards[2], "getBoundingClientRect", {configurable: true, value: () => rect(124, 20)});
    const snapshot = captureNotebookRootLayoutSnapshot(root, documents);
    assert.equal(snapshot.anchorKey, "sy\u001fbox\u001fthree");
    assert.equal(snapshot.anchorOffset, 24);

    Object.defineProperty(cards[2], "getBoundingClientRect", {configurable: true, value: () => rect(171, 20)});
    restoreNotebookRootScrollAnchor(root, documents, snapshot);
    assert.equal(root.scrollTop, 547);

    const missing = {...snapshot, anchorKey: "sy\u001fbox\u001fmissing", scrollTop: 321};
    restoreNotebookRootScrollAnchor(root, documents, missing);
    assert.equal(root.scrollTop, 321);
    dom.window.close();
});
