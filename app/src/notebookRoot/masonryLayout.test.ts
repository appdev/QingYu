import assert = require("node:assert/strict");
import test from "node:test";
import {
    calculateNotebookRootMasonryLayout,
    notebookRootMasonryColumnCount,
} from "./masonryLayout";

const closeTo = (actual: number, expected: number) =>
    assert.ok(Math.abs(actual - expected) < 0.001, `${actual} is not close to ${expected}`);

test("stable masonry assigns equal columns from left to right", () => {
    const layout = calculateNotebookRootMasonryLayout({
        containerWidth: 1280,
        ratios: [1, 1, 1, 1, 1, 1],
    });
    [16, 269.6, 523.2, 776.8, 1030.4].forEach((left, index) =>
        closeTo(layout.placements[index].left, left));
    closeTo(layout.placements[5].left, 16);
    layout.placements.forEach((item, index, items) => {
        if (index > 0) assert.ok(item.top >= items[index - 1].top);
    });
});

test("stable masonry fills the shortest column without changing source order", () => {
    const layout = calculateNotebookRootMasonryLayout({
        containerWidth: 900,
        ratios: [2, 1, 3, 1, 1, 1],
    });
    assert.equal(layout.columnCount, 4);
    const firstRowTop = layout.placements[0].top;
    assert.deepEqual(layout.placements.slice(0, 4).map((item) => item.top), Array(4).fill(firstRowTop));
    closeTo(layout.placements[4].left, layout.placements[1].left);
    closeTo(layout.placements[5].left, layout.placements[3].left);
    layout.placements.forEach((item, index, items) => {
        if (index > 0) assert.ok(item.top >= items[index - 1].top);
    });
});

test("stable masonry follows every responsive column boundary", () => {
    const cases = [
        [451, 1],
        [451.01, 2],
        [671, 2],
        [671.01, 3],
        [891, 3],
        [891.01, 4],
        [1111, 4],
        [1111.01, 5],
    ];
    cases.forEach(([width, columns]) => assert.equal(notebookRootMasonryColumnCount(width), columns));
    assert.equal(notebookRootMasonryColumnCount(Number.NaN), 1);
    assert.equal(notebookRootMasonryColumnCount(Number.POSITIVE_INFINITY), 1);
});

test("stable masonry keeps one-column documents in source order", () => {
    const layout = calculateNotebookRootMasonryLayout({containerWidth: 400, ratios: [1, 2, 0.5]});
    assert.equal(layout.columnCount, 1);
    assert.deepEqual(layout.placements.map((item) => item.left), [16, 16, 16]);
    assert.ok(layout.placements[0].top < layout.placements[1].top);
    assert.ok(layout.placements[1].top < layout.placements[2].top);
});

test("stable masonry handles empty and invalid dimensions safely", () => {
    const empty = calculateNotebookRootMasonryLayout({containerWidth: 1280, ratios: []});
    assert.equal(empty.height, 0);
    assert.deepEqual(empty.placements, []);

    const invalid = calculateNotebookRootMasonryLayout({
        containerWidth: Number.NaN,
        ratios: [Number.NaN, 0, Number.POSITIVE_INFINITY],
    });
    assert.equal(invalid.columnCount, 1);
    invalid.placements.forEach((item) => {
        assert.ok(Number.isFinite(item.left));
        assert.ok(Number.isFinite(item.top));
        assert.ok(Number.isFinite(item.width));
        assert.ok(Number.isFinite(item.height));
        assert.ok(item.left >= 0 && item.top >= 0 && item.width >= 0 && item.height >= 0);
    });
});
