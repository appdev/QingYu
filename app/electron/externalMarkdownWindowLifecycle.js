const createExternalMarkdownWindowLifecycle = ({initialExternalRequest = false} = {}) => {
    let mode = initialExternalRequest ? "external-only" : "normal";
    let deferredMainClose = false;

    return {
        noteExternalRequestBeforeReady() {
            if (mode === "normal") mode = "external-only";
        },
        isExternalOnly() {
            return mode === "external-only";
        },
        shouldShowStartupWindows() {
            return mode !== "external-only";
        },
        promote() {
            if (mode === "external-only") mode = "promoted";
        },
        deferMainClose() {
            deferredMainClose = true;
        },
        consumeDeferredMainClose() {
            const result = deferredMainClose;
            deferredMainClose = false;
            return result;
        },
        shouldExitAfterLastExternalWindow() {
            return mode === "external-only";
        },
    };
};

module.exports = {createExternalMarkdownWindowLifecycle};
