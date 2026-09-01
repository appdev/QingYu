import {notebookRootNeedsMarkdownIdentity} from "./rules";

interface PreviewDescriptor {
    cacheKey: string;
    url: string;
    exists: boolean;
    theme: "light" | "dark";
    size: "medium" | "small";
}

interface PreviewJob {
    element: HTMLElement;
    reference: {
        kind: "sy" | "markdown",
        notebook: string,
        path: string,
        id: string,
        identityState?: string,
        identityConflict?: boolean,
        revision?: string,
        updated?: number,
        sourceSize?: number,
    };
}

const sessionPreviewDescriptors = new Map<string, PreviewDescriptor>();
const maximumSessionPreviewDescriptors = 512;

export const documentCardPreviewSessionKey = (
    reference: PreviewJob["reference"],
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

export class DocumentCardPreviewController {
    private readonly observer: IntersectionObserver;
    private readonly jobs = new Map<Element, PreviewJob>();
    private readonly queue: PreviewJob[] = [];
    private readonly urls = new Set<string>();
    private running = 0;
    private destroyed = false;

    constructor() {
        this.observer = new IntersectionObserver((entries) => entries.forEach((entry) => {
            const job = this.jobs.get(entry.target);
            if (entry.isIntersecting && job && !job.element.dataset.previewReady && !this.queue.includes(job)) {
                this.queue.push(job);
                void this.drain();
            } else if (!entry.isIntersecting && job && this.running < 1) {
                const index = this.queue.indexOf(job);
                if (index > -1) this.queue.splice(index, 1);
            }
        }), {rootMargin: "40px"});
    }

    public observe(element: HTMLElement, reference: PreviewJob["reference"]) {
        const job = {element, reference};
        this.jobs.set(element, job);
        this.observer.observe(element);
    }

    public destroy() {
        this.destroyed = true;
        this.observer.disconnect();
        this.queue.length = 0;
        this.jobs.clear();
        this.urls.forEach((url) => URL.revokeObjectURL(url));
        this.urls.clear();
    }

    private async drain() {
        while (!this.destroyed && this.running < 1 && this.queue.length > 0) {
            const job = this.queue.shift();
            this.running++;
            void this.render(job).finally(() => {
                this.running--;
                void this.drain();
            });
        }
    }

    private async render(job: PreviewJob) {
        try {
            await waitForPreviewPaint();
            await waitForPreviewIdle();
            if (this.destroyed || !job.element.isConnected) return;
            const {fetchSyncPost} = await import("../util/fetch");
            if (notebookRootNeedsMarkdownIdentity(job.reference.kind, job.reference.identityState,
                Boolean(job.reference.identityConflict))) {
                if (window.siyuan.config.readonly) {
                    job.element.dataset.previewState = "failed";
                    return;
                }
                const {createMarkdownManagementOperationID} = await import("../markdown/documentManagement");
                const identity = await fetchSyncPost("/api/markdown/ensureDocumentIdentity", {
                    notebook: job.reference.notebook,
                    path: job.reference.path,
                    revision: job.reference.revision,
                    operationID: createMarkdownManagementOperationID(),
                    forceNew: Boolean(job.reference.identityConflict),
                });
                if (identity.code !== 0) throw new Error(identity.msg || "document identity creation failed");
                if (this.destroyed || !job.element.isConnected) return;
                job.reference.id = identity.data.documentID;
                job.reference.identityState = "valid";
                job.reference.identityConflict = false;
                job.reference.revision = identity.data.revision;
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
                descriptor = prepared.data as PreviewDescriptor;
                if (descriptor.exists) {
                    cacheSessionPreviewDescriptor(sessionKey, descriptor);
                }
            }
            if (descriptor.exists) {
                await this.installImage(job, descriptor.url);
                return;
            }
            const {renderDocumentCardPreview} = await import("./previewRenderer");
            const blob = await renderDocumentCardPreview({reference: job.reference, size});
            if (this.destroyed || !job.element.isConnected) return;
            const formData = new FormData();
            formData.append("reference", JSON.stringify(job.reference));
            formData.append("descriptor", JSON.stringify(descriptor));
            formData.append("file", blob, `${descriptor.cacheKey}.webp`);
            const stored = await fetchSyncPost("/api/notebook/storeDocumentCardPreview", formData);
            if (stored.code !== 0) throw new Error(stored.msg || "preview store failed");
            cacheSessionPreviewDescriptor(sessionKey, {...descriptor, exists: true});
            const url = URL.createObjectURL(blob);
            this.urls.add(url);
            await this.installImage(job, url);
        } catch {
            job.element.dataset.previewState = "failed";
        }
    }

    private async installImage(job: PreviewJob, url: string) {
        const image = await decodeDocumentCardPreviewImage(url);
        if (this.destroyed || !job.element.isConnected) return;
        if (installDocumentCardPreviewImage(job.element, url, image)) {
            job.element.dataset.previewReady = "true";
            job.element.dataset.previewState = "ready";
        }
    }
}
