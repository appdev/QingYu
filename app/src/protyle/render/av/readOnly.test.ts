import assert = require("node:assert/strict");
import {afterEach, beforeEach, test} from "node:test";
import {installMarkdownTestDom} from "../../../markdown/markraTestDom";
import {bindDatabaseReadOnly} from "./readOnly";

let cleanup: () => void;
beforeEach(() => { cleanup = installMarkdownTestDom(); });
afterEach(() => cleanup());

test("database reading removes edit controls and protects paginated content", async () => {
    const database = document.createElement("div");
    database.innerHTML = `<div contenteditable="true" class="av__title">Archive</div>
        <button data-type="av-add">New</button><button data-type="av-filter">Filter</button>
        <button data-type="av-load-more">More</button><div data-type="av-search" contenteditable="plaintext-only"></div>
        <div data-type="editCol">Field name</div><div draggable="true"><a href="https://example.com">Source</a></div>`;
    bindDatabaseReadOnly(database);
    assert.equal(database.getAttribute("contenteditable"), "false");
    assert.equal(database.querySelector('[data-type="av-add"]'), null);
    assert.equal(database.querySelector('[data-type="av-filter"]'), null);
    assert.ok(database.querySelector('[data-type="av-load-more"]'));
    assert.ok(database.querySelector("a"));
    assert.ok(database.textContent.includes("Field name"));
    assert.equal(database.querySelector('[data-type="editCol"]'), null);
    assert.equal(database.querySelector(".av__title").getAttribute("contenteditable"), "false");
    assert.equal(database.querySelector('[data-type="av-search"]').getAttribute("contenteditable"), "plaintext-only");
    database.insertAdjacentHTML("beforeend", '<input value="Existing"><button data-type="av-row-more">Edit</button>');
    await Promise.resolve();
    assert.equal(database.querySelector("input").readOnly, true);
    assert.equal(database.querySelector('[data-type="av-row-more"]'), null);
});

test("database blocks mutations while allowing search and link navigation", () => {
    const database = document.createElement("div");
    document.body.append(database);
    database.innerHTML = '<div class="value">Value</div><div data-type="av-search" contenteditable="true"></div><a href="#target">Source</a>';
    let writes = 0;
    let links = 0;
    let searches = 0;
    bindDatabaseReadOnly(database);
    database.addEventListener("change", () => writes++);
    database.addEventListener("drop", () => writes++);
    database.addEventListener("input", () => searches++);
    database.addEventListener("click", () => links++);
    const value = database.querySelector(".value");
    for (const type of ["beforeinput", "change", "drop", "paste", "cut"]) {
        const event = new window.Event(type, {bubbles: true, cancelable: true});
        value.dispatchEvent(event);
        assert.equal(event.defaultPrevented, true, type);
    }
    database.querySelector('[data-type="av-search"]').dispatchEvent(new window.Event("input", {bubbles: true}));
    database.querySelector("a").dispatchEvent(new window.MouseEvent("click", {bubbles: true}));
    assert.equal(writes, 0);
    assert.equal(searches, 1);
    assert.equal(links, 1);
});
