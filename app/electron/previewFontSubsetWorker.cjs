const {parentPort} = require("node:worker_threads");
const {subsetCSS} = require("./previewFontSubset.cjs");

let source;
let sourceID;
let pending = Promise.resolve();
parentPort.on("message", (message) => {
    pending = pending.then(async () => {
        try {
            if (typeof message.css === "string") {
                source = message.css;
                sourceID = message.sourceID;
            }
            if (sourceID !== message.sourceID) throw new Error("Unknown preview font source");
            const css = await subsetCSS(source, message.text);
            parentPort.postMessage({id: message.id, css});
        } catch {
            parentPort.postMessage({id: message.id});
        }
    });
});
