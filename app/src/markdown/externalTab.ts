export type MarkdownTabLocation =
    | {kind: "workspace"; notebookId: string; path: string}
    | {kind: "external"; capabilityId: string};

export const markdownLayoutData = (location: MarkdownTabLocation) => location.kind === "external"
    ? {instance: "MarkdownEditor", externalCapabilityId: location.capabilityId}
    : {instance: "MarkdownEditor", notebookId: location.notebookId, path: location.path};

export const collectExternalMarkdownCapabilityIds = (layout: unknown) => {
    const result = new Set<string>();
    const visit = (value: unknown) => {
        if (!value || typeof value !== "object") return;
        const record = value as Record<string, unknown>;
        if (record.instance === "MarkdownEditor" && typeof record.externalCapabilityId === "string") {
            result.add(record.externalCapabilityId);
        }
        Object.values(record).forEach(visit);
    };
    visit(layout);
    return [...result];
};
