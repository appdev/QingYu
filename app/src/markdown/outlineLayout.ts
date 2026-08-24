export interface MarkdownLayoutSource {
    externalCapabilityId?: string;
    instance?: string;
    notebookId?: string;
    path?: string;
}

export const markdownLayoutSourceKey = (source: MarkdownLayoutSource) => {
    if (source.instance !== "MarkdownEditor") return null;
    if (source.externalCapabilityId) return `external:${source.externalCapabilityId}`;
    if (source.notebookId && source.path) return `workspace:${source.notebookId}:${source.path}`;
    return null;
};

export const canRestoreMarkdownOutline = (
    sourceKey: string,
    active: boolean,
    serializedSources: readonly string[],
) => active || serializedSources.some((serialized) => {
    try {
        return markdownLayoutSourceKey(JSON.parse(serialized) as MarkdownLayoutSource) === sourceKey;
    } catch {
        return false;
    }
});

export const serializeMarkdownOutline = (sourceKey: string) => ({
    instance: "MarkdownOutline" as const,
    sourceKey,
});

export const collectMarkdownLayoutSourceKeys = (layout: unknown) => {
    const keys = new Set<string>();
    const visit = (value: unknown) => {
        if (!value || typeof value !== "object") return;
        const item = value as MarkdownLayoutSource & {children?: unknown};
        const sourceKey = markdownLayoutSourceKey(item);
        if (sourceKey) keys.add(sourceKey);
        if (Array.isArray(item.children)) item.children.forEach(visit);
        else visit(item.children);
    };
    visit(layout);
    return keys;
};
