import type {
    MarkdownDocument,
    MarkdownDocumentSource,
    MarkdownMutationResult,
    MarkdownRenameRequest,
    MarkdownSaveRequest,
    MarkdownSavedAsset,
} from "./documentSource";

type Invoke = (channel: string, payload: Record<string, unknown>) => Promise<unknown>;

interface ExternalDocumentSourceOptions {
    capabilityId: string;
    readOnly: boolean | (() => boolean);
    invoke: Invoke;
}

const CHANNEL = "siyuan-external-markdown";
const mutationError = (): MarkdownMutationResult => ({status: "error", code: "READ_ONLY"});
const encodeResourcePath = (source: string) => source.split("/").map(encodeURIComponent).join("/");

export const createExternalMarkdownDocumentSource = ({
    capabilityId,
    readOnly,
    invoke,
}: ExternalDocumentSourceOptions): MarkdownDocumentSource => {
    let resourceToken = "";
    const isReadOnly = () => typeof readOnly === "function" ? readOnly() : readOnly;
    return {
    kind: "external",
    titlePersistence: "frontmatter-and-name",
    key: `external:${capabilityId}`,
    get readOnly() { return isReadOnly(); },
    async load() {
        const result = await invoke(CHANNEL, {action: "read", capabilityId}) as {
            status: "ok" | "error";
            document?: MarkdownDocument;
            code?: string;
        };
        if (result.status !== "ok" || !result.document) throw new Error(result.code || "READ_FAILED");
        resourceToken = result.document.resourceToken || "";
        return result.document;
    },
    save(request: MarkdownSaveRequest) {
        if (isReadOnly()) return Promise.resolve(mutationError());
        return invoke(CHANNEL, {action: "save", capabilityId, request}) as Promise<MarkdownMutationResult>;
    },
    rename(request: MarkdownRenameRequest) {
        if (isReadOnly()) return Promise.resolve(mutationError());
        return invoke(CHANNEL, {action: "rename", capabilityId, request}) as Promise<MarkdownMutationResult>;
    },
    resolveImageSource(source: string) {
        const value = source.trim().replace(/^<|>$/gu, "");
        if (!value || /^(?:javascript|vbscript):/iu.test(value)) return null;
        if (/^data:/iu.test(value)) {
            return /^data:image\/(?:avif|gif|jpeg|png|svg\+xml|webp);/iu.test(value) ? value : null;
        }
        if (/^(?:https?|blob):/iu.test(value)) {
            try {
                return new URL(value).toString();
            } catch {
                return null;
            }
        }
        if (/^[a-z][a-z\d+.-]*:/iu.test(value) || value.startsWith("/") || value.includes("\\")) return null;
        if (!resourceToken) return null;
        return `qingyu-external-resource://resource/${encodeURIComponent(capabilityId)}/${encodeURIComponent(resourceToken)}/${encodeResourcePath(value)}`;
    },
    async openLink(target: string) {
        await invoke(CHANNEL, {action: "openLink", capabilityId, target});
    },
    async saveAssets(files: readonly File[]): Promise<readonly MarkdownSavedAsset[]> {
        if (isReadOnly()) throw new Error("READ_ONLY");
        const assets = await Promise.all(files.map(async (file) => ({
            name: file.name,
            mimeType: file.type,
            bytes: new Uint8Array(await file.arrayBuffer()),
        })));
        const result = await invoke(CHANNEL, {action: "saveAssets", capabilityId, assets});
        if (!Array.isArray(result)) throw new Error((result as {code?: string})?.code || "ASSET_SAVE_FAILED");
        return result as readonly MarkdownSavedAsset[];
    },
    };
};
