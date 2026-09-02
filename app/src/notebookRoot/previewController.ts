import {notebookRootDocumentKey, notebookRootElementKey} from "./documentKey";
import {notebookRootNeedsMarkdownIdentity} from "./rules";

interface PreviewDescriptor {
    cacheKey: string;
    url: string;
    exists: boolean;
    theme: "light" | "dark";
    size: "medium" | "small";
}

export interface PreviewReference {
    kind: "sy" | "markdown";
    notebook: string;
    path: string;
    id: string;
    identityState?: string;
    identityConflict?: boolean;
    revision?: string;
    updated?: number;
    sourceSize?: number;
}

interface PreviewJob {
    key: string;
    reference: PreviewReference;
}

export interface DocumentCardPreviewControllerOptions {
    onIdentityCreated?: (identity: {
        notebook: string;
        path: string;
        documentID: string;
        revision: string;
    }) => void;
}

const sessionPreviewDescriptors = new Map<string, PreviewDescriptor>();
const maximumSessionPreviewDescriptors = 512;

export const documentCardPreviewSessionKey = (
    reference: PreviewReference,
    theme: PreviewDescriptor["theme"],
    size: PreviewDescriptor["size"],
) => [
    reference.kind,
    reference.notebook,
    reference.id || reference.path,
    reference.revision || "",
    reference.updated || 0,
    reference.sourceSize || 0,
    theme,
    size,
].join("\u001f");

const cacheSessionPreviewDescriptor = (key: string, descriptor: PreviewDescriptor) => {
    sessionPreviewDescriptors.delete(key);
    sessionPreviewDescriptors.set(key, descriptor);
    if (sessionPreviewDescriptors.size > maximumSessionPreviewDescriptors) {
        sessionPreviewDescriptors.delete(sessionPreviewDescriptors.keys().next().value);
    }
};

const waitForPreviewPaint = () => new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
});

const waitForPreviewIdle = () => new Promise<void>((resolve) => {
    if ("requestIdleCallback" in window) {
        window.requestIdleCallback(() => resolve(), {timeout: 2000});
    } else {
        globalThis.setTimeout(resolve, 64);
    }
});

const createDocumentCardPreviewImage = (url: string) => {
    const image = document.createElement("img");
    image.className = "notebook-root__preview";
    image.alt = "";
    image.decoding = "async";
    image.draggable = false;
    image.setAttribute("fetchpriority", "low");
    image.src = url;
    return image;
};

const decodeDocumentCardPreviewImage = async (url: string) => {
    const image = createDocumentCardPreviewImage(url);
    if (typeof image.decode === "function") {
        try {
            await image.decode();
        } catch {
            // 图片加载失败时仍交给浏览器展示，避免永久停留在占位状态。
        }
    }
    return image;
};

export const installDocumentCardPreviewImage = (element: HTMLElement, url: string, decodedImage?: HTMLImageElement) => {
    const previewBox = element.querySelector<HTMLElement>(".notebook-root__preview-box");
    const placeholder = previewBox?.querySelector(".notebook-root__placeholder");
    if (!previewBox || !placeholder) {
        return false;
    }
    const image = decodedImage || createDocumentCardPreviewImage(url);
    placeholder.replaceWith(image);
    return true;
};

const previewReferenceFromElement = (element: HTMLElement): PreviewReference => ({
    kind: element.dataset.kind as "sy" | "markdown",
    notebook: element.dataset.notebook || "",
    path: element.dataset.path || "",
    id: element.dataset.id || "",
    identityState: element.dataset.identityState,
    identityConflict: element.dataset.identityConflict === "true",
    revision: element.dataset.revision,
    updated: Number(element.dataset.updated) || 0,
    sourceSize: Number(element.dataset.sourceSize) || 0,
});

export class DocumentCardPreviewController {
    private readonly observer: IntersectionObserver;
    private readonly options: DocumentCardPreviewControllerOptions;
    private readonly targets = new Map<string, HTMLElement>();
    private readonly jobs = new Map<string, PreviewJob>();
    private readonly queue: string[] = [];
    private activeJob?: PreviewJob;
    private destroyed = false;

    constructor(options: DocumentCardPreviewControllerOptions = {}) {
        this.options = options;
        this.observer = new IntersectionObserver((entries) => entries.forEach((entry) => {
            const element = entry.target as HTMLElement;
            const key = notebookRootElementKey(element);
            const job = this.jobs.get(key);
            if (this.targets.get(key) !== element || !job) return;
            if (entry.isIntersecting && !element.dataset.previewReady &&
                !this.queue.includes(key) && this.activeJob !== job) {
                this.queue.push(key);
                void this.drain();
            } else if (!entry.isIntersecting && this.activeJob !== job) {
                const index = this.queue.indexOf(key);
                if (index > -1) this.queue.splice(index, 1);
            }
        }), {rootMargin: "40px"});
    }

    public rebind(elements: Iterable<HTMLElement>) {
        this.observer.disconnect();
        this.targets.clear();
        for (const element of elements) {
            const key = notebookRootElementKey(element);
            element.dataset.previewKey = key;
            this.targets.set(key, element);
            if (!this.jobs.has(key)) {
                this.jobs.set(key, {key, reference: previewReferenceFromElement(element)});
            }
            this.observer.observe(element);
        }
    }

    public destroy() {
        this.destroyed = true;
        this.observer.disconnect();
        this.queue.length = 0;
        this.targets.clear();
        this.jobs.clear();
    }

    private target(job: PreviewJob) {
        const target = this.targets.get(job.key);
        return target?.isConnected ? target : undefined;
    }

    private async drain() {
        if (this.destroyed || this.activeJob || this.queue.length === 0) return;
        const key = this.queue.shift();
        const job = key ? this.jobs.get(key) : undefined;
        if (!job) {
            void this.drain();
            return;
        }
        this.activeJob = job;
        void this.render(job).finally(() => {
            if (this.activeJob === job) this.activeJob = undefined;
            void this.drain();
        });
    }

    private migrateJobIdentity(job: PreviewJob, identity: {documentID: string, revision: string}) {
        const temporaryKey = job.key;
        job.reference.id = identity.documentID;
        job.reference.identityState = "valid";
        job.reference.identityConflict = false;
        job.reference.revision = identity.revision;
        const formalKey = notebookRootDocumentKey(job.reference);
        if (formalKey !== temporaryKey) {
            const target = this.targets.get(temporaryKey);
            this.jobs.delete(temporaryKey);
            this.jobs.set(formalKey, job);
            this.targets.delete(temporaryKey);
            if (target) {
                target.dataset.id = identity.documentID;
                target.dataset.identityState = "valid";
                target.dataset.identityConflict = "false";
                target.dataset.revision = identity.revision;
                target.dataset.previewKey = formalKey;
                this.targets.set(formalKey, target);
            }
            this.queue.forEach((key, index) => {
                if (key === temporaryKey) this.queue[index] = formalKey;
            });
            job.key = formalKey;
        }
        this.options.onIdentityCreated?.({
            notebook: job.reference.notebook,
            path: job.reference.path,
            documentID: identity.documentID,
            revision: identity.revision,
        });
    }

    private async render(job: PreviewJob) {
        try {
            await waitForPreviewPaint();
            await waitForPreviewIdle();
            if (this.destroyed || !this.target(job)) return;
            const {fetchSyncPost} = await import("../util/fetch");
            if (this.destroyed || !this.target(job)) return;
            if (notebookRootNeedsMarkdownIdentity(job.reference.kind, job.reference.identityState,
                Boolean(job.reference.identityConflict))) {
                if (window.siyuan.config.readonly) {
                    const target = this.target(job);
                    if (target) target.dataset.previewState = "failed";
                    return;
                }
                const {createMarkdownManagementOperationID} = await import("../markdown/documentManagement");
                if (this.destroyed || !this.target(job)) return;
                const identity = await fetchSyncPost("/api/markdown/ensureDocumentIdentity", {
                    notebook: job.reference.notebook,
                    path: job.reference.path,
                    revision: job.reference.revision,
                    operationID: createMarkdownManagementOperationID(),
                    forceNew: Boolean(job.reference.identityConflict),
                });
                if (identity.code !== 0) throw new Error(identity.msg || "document identity creation failed");
                if (this.destroyed || !this.target(job)) return;
                this.migrateJobIdentity(job, {
                    documentID: identity.data.documentID,
                    revision: identity.data.revision,
                });
            }
            const size = "medium";
            const theme = window.siyuan.config.appearance.mode === 1 ? "dark" : "light";
            const sessionKey = documentCardPreviewSessionKey(job.reference, theme, size);
            let descriptor = sessionPreviewDescriptors.get(sessionKey);
            if (!descriptor) {
                const prepared = await fetchSyncPost("/api/notebook/prepareDocumentCardPreview", {
                    reference: job.reference,
                    theme,
                    size,
                });
                if (prepared.code !== 0) throw new Error(prepared.msg || "preview preparation failed");
                if (this.destroyed || !this.target(job)) return;
                descriptor = prepared.data as PreviewDescriptor;
                if (descriptor.exists) {
                    cacheSessionPreviewDescriptor(sessionKey, descriptor);
                }
            }
            if (descriptor.exists) {
                await this.installImage(job.key, descriptor.url);
                return;
            }
            const {renderDocumentCardPreview} = await import("./previewRenderer");
            if (this.destroyed || !this.target(job)) return;
            const blob = await renderDocumentCardPreview({reference: job.reference, size});
            if (this.destroyed || !this.target(job)) return;
            const formData = new FormData();
            formData.append("reference", JSON.stringify(job.reference));
            formData.append("descriptor", JSON.stringify(descriptor));
            formData.append("file", blob, `${descriptor.cacheKey}.webp`);
            const stored = await fetchSyncPost("/api/notebook/storeDocumentCardPreview", formData);
            if (stored.code !== 0) throw new Error(stored.msg || "preview store failed");
            cacheSessionPreviewDescriptor(sessionKey, {...descriptor, exists: true});
            if (this.destroyed) return;
            await this.installImage(job.key, descriptor.url);
        } catch {
            const target = this.target(job);
            if (target) target.dataset.previewState = "failed";
        }
    }

    private async installImage(key: string, url: string) {
        const image = await decodeDocumentCardPreviewImage(url);
        const target = this.targets.get(key);
        if (this.destroyed || !target?.isConnected) return;
        if (installDocumentCardPreviewImage(target, url, image)) {
            target.dataset.previewReady = "true";
            target.dataset.previewState = "ready";
        }
    }
}
