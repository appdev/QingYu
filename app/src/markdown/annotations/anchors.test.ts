import assert = require("node:assert/strict");
import {test} from "node:test";
import {anchorAt, reanchor} from "./anchors";
import {normalizeAnnotationText, parseAnnotations, type MarkdownAnnotation} from "./types";

const record = (text: string, from: number, to: number): MarkdownAnnotation => ({
    ...anchorAt(text, from, to), id: "a", note: "读书笔记", createdAt: 1, updatedAt: 1,
});
test("normalizes BOM and line endings, keeping UTF-16 source positions", () => {
    const text = normalizeAnnotationText("\uFEFF甲\r\n😀乙\r丙");
    assert.equal(text, "甲\n😀乙\n丙");
    assert.equal(anchorAt(text, 2, 5).quote, "😀乙");
});
test("recovers a uniquely contextualized quote and preserves ambiguous orphans", () => {
    const item = record("甲乙丙", 1, 2);
    assert.equal(reanchor(item, "序甲乙丙").from, 2);
    assert.equal(reanchor({...item, prefix: "", suffix: "", from: 99, to: 100}, "乙和乙").status, "orphaned");
    assert.equal(reanchor(item, "甲丙").status, "orphaned");
});
test("rejects unknown schemas and duplicate record identifiers", () => {
    const item = record("甲乙丙", 1, 2);
    const data = {schemaVersion: 1, documentId: "doc", revision: 0, contentHash: "", records: [item]};
    assert.deepEqual(parseAnnotations(data), data);
    assert.throws(() => parseAnnotations({...data, schemaVersion: 2}));
    assert.throws(() => parseAnnotations({...data, documentId: undefined}));
    assert.throws(() => parseAnnotations({...data, records: [{...item, id: undefined}]}));
    assert.throws(() => parseAnnotations({...data, records: [item, item]}));
});
