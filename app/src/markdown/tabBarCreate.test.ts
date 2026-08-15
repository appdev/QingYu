import * as assert from "node:assert/strict";
import {JSDOM} from "jsdom";
import test from "node:test";
import {createMarkdownFromTabBarAction} from "./tabBarCreate";

test("routes the tab bar add button and its icon to Markdown creation", async (t) => {
    const dom = new JSDOM(`<div>
        <span data-type="new" class="block__icon"><svg><use id="icon"></use></svg></span>
        <span data-type="more" class="block__icon" id="more"></span>
    </div>`);
    const app = {name: "test-app"};

    for (const selector of ["[data-type=\"new\"]", "#icon"]) {
        await t.test(selector, () => {
            const calls: unknown[] = [];
            const handled = createMarkdownFromTabBarAction(app, dom.window.document.querySelector(selector), async (actualApp) => {
                calls.push(actualApp);
                return true;
            });

            assert.equal(handled, true);
            assert.deepEqual(calls, [app]);
        });
    }

    let called = false;
    assert.equal(createMarkdownFromTabBarAction(app, dom.window.document.querySelector("#more"), async () => {
        called = true;
        return true;
    }), false);
    assert.equal(called, false);
});
