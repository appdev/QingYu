const assert = require("node:assert/strict");
const test = require("node:test");
const {
    createMarkdownManagementCoordinator,
    shouldUnregisterMarkdownRendererNavigation,
} = require("./markdownManagementCoordinator");

const ref = {kind: "markdown", notebook: "box", path: "/a.md"};

test("waits for every ready renderer and commits only with the prepared generation lease", async () => {
    const sent = [];
    const coordinator = createMarkdownManagementCoordinator({timeout: 100});
    const generation1 = coordinator.register(1, "workspace", (payload) => sent.push([1, payload]));
    const generation2 = coordinator.register(2, "workspace", (payload) => sent.push([2, payload]));

    const pending = coordinator.prepare(1, {workspace: "workspace", operationID: "op-1", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1, operationID: "op-1", ok: true, matched: true, matches: 1, revision: "r1"});
    coordinator.ack(2, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation2, operationID: "op-1", ok: true, matched: true, matches: 1, revision: "r1"});

    const prepared = await pending;
    assert.equal(prepared.ok, true);
    assert.equal(prepared.revision, "r1");
    assert.equal(typeof prepared.lease, "string");
    assert.equal(sent.length, 2);
    const committing = coordinator.commit(1, {workspace: "workspace", operationID: "op-1", lease: prepared.lease,
        mutation: {kind: "save", from: ref, to: ref, revision: "r2"}});
    coordinator.ack(1, "workspace", {phase: "commit", workspace: "workspace", generation: generation1, operationID: "op-1", ok: true});
    coordinator.ack(2, "workspace", {phase: "commit", workspace: "workspace", generation: generation2, operationID: "op-1", ok: true});
    assert.deepEqual(await committing, {ok: true});
});

test("fails closed for readonly, revision disagreement, timeout, and destroyed renderers", async () => {
    const coordinator = createMarkdownManagementCoordinator({timeout: 5});
    const generation1 = coordinator.register(1, "workspace", () => undefined);
    let generation2 = coordinator.register(2, "workspace", () => undefined);

    let pending = coordinator.prepare(1, {workspace: "workspace", operationID: "readonly", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1, operationID: "readonly", ok: false, matched: true, matches: 1});
    coordinator.ack(2, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation2, operationID: "readonly", ok: true, matched: false, matches: 0});
    assert.equal((await pending).ok, false);

    pending = coordinator.prepare(1, {workspace: "workspace", operationID: "mismatch", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1, operationID: "mismatch", ok: true, matched: true, matches: 1, revision: "r1"});
    coordinator.ack(2, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation2, operationID: "mismatch", ok: true, matched: true, matches: 1, revision: "r2"});
    assert.equal((await pending).ok, false);

    pending = coordinator.prepare(1, {workspace: "workspace", operationID: "destroyed", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1, operationID: "destroyed", ok: true, matched: true, matches: 1, revision: "r1"});
    coordinator.unregister(2);
    assert.equal((await pending).ok, false);

    generation2 = coordinator.register(2, "workspace", () => undefined);
    pending = coordinator.prepare(1, {workspace: "workspace", operationID: "timeout", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1, operationID: "timeout", ok: true, matched: true, matches: 1, revision: "r1"});
    assert.equal((await pending).ok, false);
});

test("presence sums matching editors without requiring a revision", async () => {
    const coordinator = createMarkdownManagementCoordinator({timeout: 100});
    const generation1 = coordinator.register(1, "workspace", () => undefined);
    const generation2 = coordinator.register(2, "workspace", () => undefined);
    const pending = coordinator.prepare(1, {workspace: "workspace", operationID: "presence", ref, mode: "presence"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "presence", workspace: "workspace", generation: generation1, operationID: "presence", ok: true, matched: false, matches: 0});
    coordinator.ack(2, "workspace", {phase: "prepare", mode: "presence", workspace: "workspace", generation: generation2, operationID: "presence", ok: true, matched: true, matches: 2});
    assert.deepEqual(await pending, {ok: true, matches: 2});
});

test("ignores acknowledgements with stale workspace or prepare mode", async () => {
    const coordinator = createMarkdownManagementCoordinator({timeout: 5});
    const generation = coordinator.register(1, "workspace", () => undefined);
    let pending = coordinator.prepare(1, {workspace: "workspace", operationID: "wrong-workspace", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "other", generation,
        operationID: "wrong-workspace", ok: true, matched: true, matches: 1, revision: "r1"});
    assert.equal((await pending).ok, false);
    pending = coordinator.prepare(1, {workspace: "workspace", operationID: "wrong-mode", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "presence", workspace: "workspace", generation,
        operationID: "wrong-mode", ok: true, matched: true, matches: 1, revision: "r1"});
    assert.equal((await pending).ok, false);
});

test("reload, navigation, destroy, and late participants invalidate prepare or commit", async () => {
    const coordinator = createMarkdownManagementCoordinator({timeout: 5});
    let generation1 = coordinator.register(1, "workspace", () => undefined);
    const generation2 = coordinator.register(2, "workspace", () => undefined);

    let pending = coordinator.prepare(1, {workspace: "workspace", operationID: "reload", ref, mode: "flush"});
    generation1 = coordinator.register(1, "workspace", () => undefined);
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1 - 1, operationID: "reload", ok: true, matched: true, matches: 1, revision: "r1"});
    assert.equal((await pending).ok, false);

    pending = coordinator.prepare(1, {workspace: "workspace", operationID: "late", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1, operationID: "late", ok: true, matched: true, matches: 1, revision: "r1"});
    coordinator.ack(2, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation2, operationID: "late", ok: true, matched: true, matches: 1, revision: "r1"});
    const prepared = await pending;
    coordinator.register(3, "workspace", () => undefined);
    assert.equal((await coordinator.commit(1, {workspace: "workspace", operationID: "late", lease: prepared.lease,
        mutation: {kind: "save", from: ref, to: ref, revision: "r2"}})).ok, false);

    pending = coordinator.prepare(1, {workspace: "workspace", operationID: "nav", ref, mode: "flush"});
    coordinator.unregister(2);
    assert.equal((await pending).ok, false);
});

test("consumes a lease before broadcasting commit and rejects a concurrent duplicate", async () => {
    const sent = [];
    const coordinator = createMarkdownManagementCoordinator({timeout: 20});
    const generation1 = coordinator.register(1, "workspace", (payload) => sent.push([1, payload]));
    const generation2 = coordinator.register(2, "workspace", (payload) => sent.push([2, payload]));
    const preparing = coordinator.prepare(1, {workspace: "workspace", operationID: "atomic", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation1,
        operationID: "atomic", ok: true, matched: true, matches: 1, revision: "r1"});
    coordinator.ack(2, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: generation2,
        operationID: "atomic", ok: true, matched: true, matches: 1, revision: "r1"});
    const prepared = await preparing;
    const request = {workspace: "workspace", operationID: "atomic", lease: prepared.lease,
        mutation: {kind: "save", from: ref, to: ref, revision: "r2"}};

    const first = coordinator.commit(1, request);
    const second = await Promise.race([
        coordinator.commit(1, request),
        new Promise((resolve) => setTimeout(() => resolve({timeout: true}), 5)),
    ]);

    assert.deepEqual(second, {ok: false});
    assert.equal(sent.filter(([, payload]) => payload.phase === "commit").length, 2);
    coordinator.ack(1, "workspace", {phase: "commit", workspace: "workspace", generation: generation1,
        operationID: "atomic", ok: true});
    coordinator.ack(2, "workspace", {phase: "commit", workspace: "workspace", generation: generation2,
        operationID: "atomic", ok: true});
    assert.deepEqual(await first, {ok: true});
});

test("commit timeout and renderer send exceptions consume the lease without allowing retries", async () => {
    const sent = [];
    const coordinator = createMarkdownManagementCoordinator({timeout: 5});
    const generation = coordinator.register(1, "workspace", (payload) => sent.push(payload));
    let preparing = coordinator.prepare(1, {workspace: "workspace", operationID: "commit-timeout", ref, mode: "flush"});
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation,
        operationID: "commit-timeout", ok: true, matched: true, matches: 1, revision: "r1"});
    let prepared = await preparing;
    const timeoutRequest = {workspace: "workspace", operationID: "commit-timeout", lease: prepared.lease,
        mutation: {kind: "save", from: ref, to: ref, revision: "r2"}};
    assert.deepEqual(await coordinator.commit(1, timeoutRequest), {ok: false});
    assert.deepEqual(await coordinator.commit(1, timeoutRequest), {ok: false});
    assert.equal(sent.filter((payload) => payload.phase === "commit").length, 1);

    coordinator.register(1, "workspace", (payload) => {
        if (payload.phase === "commit") throw new Error("renderer unavailable");
        sent.push(payload);
    });
    preparing = coordinator.prepare(1, {workspace: "workspace", operationID: "commit-throws", ref, mode: "flush"});
    const nextGeneration = sent.at(-1).generation;
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation: nextGeneration,
        operationID: "commit-throws", ok: true, matched: true, matches: 1, revision: "r1"});
    prepared = await preparing;
    const throwingRequest = {workspace: "workspace", operationID: "commit-throws", lease: prepared.lease,
        mutation: {kind: "save", from: ref, to: ref, revision: "r2"}};
    assert.deepEqual(await coordinator.commit(1, throwingRequest), {ok: false});
    assert.deepEqual(await coordinator.commit(1, throwingRequest), {ok: false});
});

test("only a non-in-place main-frame navigation unregisters a renderer", () => {
    assert.equal(shouldUnregisterMarkdownRendererNavigation(false, true), true);
    assert.equal(shouldUnregisterMarkdownRendererNavigation(true, true), false);
    assert.equal(shouldUnregisterMarkdownRendererNavigation(false, false), false);
});

test("a duplicate ready handshake keeps the generation and does not interrupt prepare", async () => {
    const coordinator = createMarkdownManagementCoordinator({timeout: 20});
    const sent = [];
    const generation = coordinator.register(1, "workspace", (payload) => sent.push(payload));
    const preparing = coordinator.prepare(1, {workspace: "workspace", operationID: "ready-again", ref, mode: "flush"});

    const repeatedGeneration = coordinator.register(1, "workspace", (payload) => sent.push(payload));
    coordinator.ack(1, "workspace", {phase: "prepare", mode: "flush", workspace: "workspace", generation,
        operationID: "ready-again", ok: true, matched: true, matches: 1, revision: "r1"});

    assert.equal(repeatedGeneration, generation);
    assert.equal((await preparing).ok, true);
});
