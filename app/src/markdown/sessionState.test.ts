import assert = require("node:assert/strict");
import {test} from "node:test";
import {
    normalizeMarkdownEditorSessionState,
    restoreMarkdownEditorSession,
    serializeMarkdownEditorSessionState,
} from "./sessionState";

test("clamps restored positions and rejects invalid modes", () => {
    assert.deepEqual(normalizeMarkdownEditorSessionState({
        mode: "bad", anchor: 99, head: -2, scroll: {position: 50, viewportOffset: 12},
    }, 10), {
        mode: "visual", selection: {anchor: 10, head: 0}, scroll: {position: 10, viewportOffset: 12},
        typewriterMode: false, typewriterModeConfigured: false,
    });
});

test("restores scroll and cue only after editor configuration is complete", () => {
    const calls: string[] = [];
    restoreMarkdownEditorSession({
        mode: "source",
        selection: {anchor: 8, head: 8},
        scroll: {position: 8, viewportOffset: 24},
        typewriterMode: false,
    }, {
        configure: () => calls.push("configure"),
        cue: (position) => calls.push(`cue:${position}`),
        restoreScroll: () => calls.push("scroll"),
    });
    assert.deepEqual(calls, ["configure", "scroll", "cue:8"]);
});

test("round-trips layout JSON and keeps legacy layouts out of typewriter mode", () => {
    const state = normalizeMarkdownEditorSessionState({
        mode: "source", scroll: {position: 18, viewportOffset: 3}, selection: {anchor: 2, head: 4},
        typewriterMode: false, typewriterModeConfigured: true,
    }, 100);
    assert.deepEqual(normalizeMarkdownEditorSessionState(JSON.parse(JSON.stringify(state)), 100), state);
    assert.equal(normalizeMarkdownEditorSessionState({mode: "visual", selection: {anchor: 0, head: 0}}, 0).typewriterMode, false);
    assert.equal(normalizeMarkdownEditorSessionState({typewriterMode: true}, 0).typewriterMode, false);
    assert.equal(normalizeMarkdownEditorSessionState({
        typewriterMode: true,
        typewriterModeConfigured: true,
    }, 0).typewriterMode, true);
});

test("persists restored Markdown editors in visual mode", () => {
    assert.deepEqual(serializeMarkdownEditorSessionState({
        mode: "source",
        selection: {anchor: 2, head: 4},
        scroll: {position: 18, viewportOffset: 3},
        typewriterMode: true,
        typewriterModeConfigured: true,
    }), {
        mode: "visual",
        selection: {anchor: 2, head: 4},
        scroll: {position: 18, viewportOffset: 3},
        typewriterMode: true,
        typewriterModeConfigured: true,
    });
});
