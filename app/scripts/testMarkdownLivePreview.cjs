const assert = require("node:assert/strict");

const wait = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

const connect = async () => {
    const targets = await fetch("http://127.0.0.1:9222/json/list").then((response) => response.json());
    const target = targets.find((item) => item.type === "page" && /QingYu|轻语/u.test(item.title));
    assert.ok(target?.webSocketDebuggerUrl, "A running QingYu page with remote debugging is required");
    const socket = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((resolve, reject) => {
        socket.addEventListener("open", resolve, {once: true});
        socket.addEventListener("error", reject, {once: true});
    });
    let messageId = 0;
    const pending = new Map();
    socket.addEventListener("message", (event) => {
        const message = JSON.parse(String(event.data));
        const callback = pending.get(message.id);
        if (callback) {
            pending.delete(message.id);
            callback(message);
        }
    });
    const call = (method, params = {}) => new Promise((resolve, reject) => {
        const id = ++messageId;
        pending.set(id, (message) => message.error ? reject(new Error(message.error.message)) : resolve(message.result));
        socket.send(JSON.stringify({id, method, params}));
    });
    return {call, socket};
};

const evaluate = async (call, expression) => {
    const result = await call("Runtime.evaluate", {expression, returnByValue: true, awaitPromise: true});
    if (result.exceptionDetails) {
        throw new Error(result.exceptionDetails.text);
    }
    return result.result.value;
};

const main = async () => {
    const {call, socket} = await connect();
    try {
        const initial = await evaluate(call, `(() => {
            const editor = document.querySelector(".layout__wnd--active .markdown-editor") ||
                document.querySelector(".markdown-editor");
            const cmElement = editor?.querySelector(".cm-editor");
            const view = editor?.__markdownEditorView;
            if (!editor || !view) {
                return undefined;
            }
            window.__markdownLivePreviewTestView = view;
            const source = view.state.doc.toString();
            return {
                length: source.length,
                markdownTables: (source.match(
                    /^\\s*\\|?\\s*:?-{3,}:?\\s*(?:\\|\\s*:?-{3,}:?\\s*)+\\|?\\s*$/gmu
                ) || []).length,
                mermaid: (source.match(/^\s*(?:\x60{3,}|~{3,})mermaid\s*$/gimu) || []).length,
                editorClientWidth: editor.clientWidth,
                editorScrollWidth: editor.scrollWidth,
                mode: cmElement.getAttribute("data-markdown-mode"),
            };
        })()`);
        assert.ok(initial, "Open a Markdown document in QingYu before running this test");
        assert.equal(initial.mode, "visual");
        assert.equal(initial.editorScrollWidth, initial.editorClientWidth);

        await evaluate(call, "document.querySelector(\".layout__wnd--active [data-type='markdown-source']\")?.click()");
        await wait(100);
        const sourceMode = await evaluate(call, `(() => {
            const editor = document.querySelector(".layout__wnd--active .markdown-editor") ||
                document.querySelector(".markdown-editor");
            const cmElement = editor.querySelector(".cm-editor");
            return {
                sameView: editor.__markdownEditorView === window.__markdownLivePreviewTestView,
                length: editor.__markdownEditorView.state.doc.length,
                mode: cmElement.getAttribute("data-markdown-mode"),
                gutters: editor.querySelectorAll(".cm-gutters").length,
            };
        })()`);
        assert.equal(sourceMode.sameView, true);
        assert.equal(sourceMode.length, initial.length);
        assert.equal(sourceMode.mode, "source");

        await call("Input.dispatchKeyEvent", {type: "keyDown", key: "a", code: "KeyA", modifiers: 4});
        await call("Input.dispatchKeyEvent", {type: "keyUp", key: "a", code: "KeyA", modifiers: 4});
        const selection = await evaluate(call, `(() => {
            const range = window.__markdownLivePreviewTestView.state.selection.main;
            return {from: range.from, to: range.to};
        })()`);
        assert.deepEqual(selection, {from: 0, to: initial.length});

        await evaluate(call, "document.querySelector(\".layout__wnd--active [data-type='markdown-preview']\")?.click()");
        await wait(100);
        const finalState = await evaluate(call, `(() => {
            const editor = document.querySelector(".layout__wnd--active .markdown-editor") ||
                document.querySelector(".markdown-editor");
            const cmElement = editor.querySelector(".cm-editor");
            const result = {
                sameView: editor.__markdownEditorView === window.__markdownLivePreviewTestView,
                length: editor.__markdownEditorView.state.doc.length,
                mode: cmElement.getAttribute("data-markdown-mode"),
            };
            delete window.__markdownLivePreviewTestView;
            return result;
        })()`);
        assert.deepEqual(finalState, {sameView: true, length: initial.length, mode: "visual"});
        console.log("Markdown live preview client test passed", {...initial, sourceGutters: sourceMode.gutters});
    } finally {
        socket.close();
    }
};

main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
});
