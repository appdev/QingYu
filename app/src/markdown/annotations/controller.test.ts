import assert = require("node:assert/strict");
import {test} from "node:test";
import {MarkdownAnnotationController} from "./controller";
import {anchorAt} from "./anchors";

test("requires acknowledgement and preserves edits made while a snapshot is saving", () => {
    const controller = new MarkdownAnnotationController(undefined, "甲乙丙");
    assert.equal(controller.snapshot([]), undefined);
    const record = {...anchorAt("甲乙丙", 0, 2), id: "a", note: "first", createdAt: 1, updatedAt: 1};
    const snapshot = controller.snapshot([record]);
    assert.throws(() => controller.saved(snapshot, undefined));
    controller.saved(snapshot, {...snapshot, revision: 1});
    const next = controller.snapshot([{...record, note: "typed during save"}]);
    assert.equal(next.revision, 1);
    assert.equal(next.records[0].note, "typed during save");
    assert.equal(snapshot.records[0].note, "first");
    assert.deepEqual(controller.snapshot([]).records, []);
});
