let previous: Promise<unknown> = Promise.resolve();

export const assertDocumentCardPreviewActive = (shouldContinue?: () => boolean) => {
    if (shouldContinue && !shouldContinue()) throw new DOMException("Preview cancelled", "AbortError");
};

// 多个卡片页共享一个渲染通道，避免同时进行截图转换。
export const queueDocumentCardPreviewRender = <T>(shouldContinue: (() => boolean) | undefined, render: () => Promise<T>) => {
    const next = previous.then(() => {
        assertDocumentCardPreviewActive(shouldContinue);
        return render();
    });
    previous = next.catch(() => undefined);
    return next;
};
