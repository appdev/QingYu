interface InitialMarkdownMetadata {
    status: "none" | "malformed" | "valid";
    title?: string | null;
}

export const shouldInitializeMarkdownTitle = (
    sourceKind: "workspace" | "external",
    metadata: InitialMarkdownMetadata,
    fileStem: string,
) => sourceKind === "workspace" &&
    (metadata.status === "none" || metadata.status === "valid" && metadata.title !== fileStem);
