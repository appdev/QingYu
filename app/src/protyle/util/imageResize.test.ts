import assert = require("node:assert/strict");
import test from "node:test";
import {
    calculateSiyuanImageWidth,
    startSiyuanImageResize,
    type SiyuanImageResizeEventTarget,
} from "./imageResize";

class FakeTarget implements SiyuanImageResizeEventTarget {
    private listeners = new Map<string, Set<(event?: {clientX: number}) => void>>();

    public addEventListener(type: string, listener: (event?: {clientX: number}) => void) {
        const listeners = this.listeners.get(type) || new Set();
        listeners.add(listener);
        this.listeners.set(type, listeners);
    }

    public removeEventListener(type: string, listener: (event?: {clientX: number}) => void) {
        this.listeners.get(type)?.delete(listener);
    }

    public dispatch(type: string, clientX = 0) {
        [...(this.listeners.get(type) || [])].forEach((listener) => listener({clientX}));
    }
}

test("uses SiYuan one-sided and centered resize geometry", () => {
    assert.equal(calculateSiyuanImageWidth(100, 20, false, 500), 120);
    assert.equal(calculateSiyuanImageWidth(100, 20, true, 500), 140);
    assert.equal(calculateSiyuanImageWidth(100, -200, false, 500), 17);
    assert.equal(calculateSiyuanImageWidth(100, 500, false, 300), 300);
});

test("previews and commits exactly once", () => {
    const target = new FakeTarget();
    const previews: number[] = [];
    const commits: number[] = [];
    startSiyuanImageResize({
        centerResize: false,
        initialClientX: 10,
        initialWidth: 100,
        maxRight: 300,
        minWidth: 17,
        onCancel: () => assert.fail("unexpected cancel"),
        onCommit: (width) => commits.push(width),
        onPreview: (width) => previews.push(width),
    }, target);
    target.dispatch("pointermove", 35);
    target.dispatch("pointerup");
    target.dispatch("pointerup");
    assert.deepEqual(previews, [125]);
    assert.deepEqual(commits, [125]);
});
