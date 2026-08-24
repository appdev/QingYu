import assert = require("node:assert/strict");
import {test} from "node:test";
import {MarkdownOutlinePublisher} from "./outlinePublisher";

test("publishes the first outline after a subscriber connects before the document is ready", () => {
    const document = {source: undefined as string | undefined};
    const publisher = new MarkdownOutlinePublisher(() => document.source);
    const emissions: string[][] = [];
    publisher.subscribe((items) => emissions.push(items.map((item) => item.title)));
    assert.deepEqual(emissions, []);
    document.source = "# Ready\n\n## Child";
    publisher.publish();
    assert.deepEqual(emissions, [["Ready", "Child"]]);
});

test("subscribers connected after load receive the current outline immediately", () => {
    const publisher = new MarkdownOutlinePublisher(() => "# Ready");
    const emissions: string[][] = [];
    publisher.subscribe((items) => emissions.push(items.map((item) => item.title)));
    assert.deepEqual(emissions, [["Ready"]]);
});
