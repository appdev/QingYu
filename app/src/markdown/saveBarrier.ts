export interface MarkdownFlushable {
    flush(): Promise<boolean>;
}

const pendingBarriers = new Map<string, Promise<void>>();
const pendingMarkdownFlushes = new Set<Promise<boolean>>();

export const trackMarkdownFlush = (flush: Promise<boolean>) => {
    pendingMarkdownFlushes.add(flush);
    void flush.then(
        () => pendingMarkdownFlushes.delete(flush),
        () => pendingMarkdownFlushes.delete(flush),
    );
    return flush;
};

export const flushMarkdownEditors = async (editors: readonly MarkdownFlushable[]) => {
    const editorFlushes = editors.map((editor) => trackMarkdownFlush(editor.flush()));
    const flushes = [...new Set([...pendingMarkdownFlushes, ...editorFlushes])];
    const results = await Promise.all(flushes.map(async (flush) => {
        try {
            return await flush;
        } catch {
            return false;
        }
    }));
    return results.every(Boolean);
};

export const handleMarkdownSaveBarrier = (
    data: {id: string},
    sessionId: string,
    editors: readonly MarkdownFlushable[],
) => {
    const key = `${data.id}\u0000${sessionId}`;
    const existing = pendingBarriers.get(key);
    if (existing) {
        return existing;
    }
    const pending = (async () => {
        const success = await flushMarkdownEditors(editors);
        try {
            await fetch("/api/asset/ackMarkdownSaveBarrier", {
                body: JSON.stringify({id: data.id, sessionId, success}),
                method: "POST",
            });
        } catch {
            // Kernel 会在确认超时后安全终止本次资源扫描。
        } finally {
            pendingBarriers.delete(key);
        }
    })();
    pendingBarriers.set(key, pending);
    return pending;
};
