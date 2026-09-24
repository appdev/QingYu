import {notebookRootDocumentKey, notebookRootElementKey} from "./documentKey";
import {notebookRootNeedsMarkdownIdentity} from "./rules";
import {documentCardPreviewAppearanceKey} from "./theme";
import {documentCardPreviewFormat} from "./previewCapture";

interface PreviewDescriptor {
    cacheKey: string;
    url: string;
    exists: boolean;
    theme: "light" | "dark";
    appearanceKey: string;
    size: "medium" | "small";
    format?: "png" | "webp";
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
    placeholder?: HTMLElement;
}

export interface DocumentCardPreviewControllerOptions {
    onIdentityCreated?: (identity: {
        notebook: string;
        path: string;
        documentID: string;
        revision: string;
    }) => void;
    request?: (url: string, data?: any, headers?: Record<string, string>) => Promise<IWebSocketData>;
    renderPreview?: (input: import("./previewRenderer").PreviewRenderInput) => Promise<Blob>;
}

const sessionPreviewDescriptors = new Map<string, PreviewDescriptor>();
const maximumSessionPreviewDescriptors = 512;

export const documentCardPreviewSessionKey = (
    reference: PreviewReference,
    theme: PreviewDescriptor["theme"],
    appearanceKey: string,
    size: PreviewDescriptor["size"],
) => [
    reference.kind,
    reference.notebook,
    reference.id || reference.path,
    reference.revision || "",
    reference.updated || 0,
    reference.sourceSize || 0,
    theme,
    appearanceKey,
    size,
    documentCardPreviewFormat(),
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
    private readonly visible = new Map<string, boolean>();
    private readonly probeQueue: PreviewJob[] = [];
    private readonly probing = new Set<PreviewJob>();
    private readonly prepared = new Map<PreviewJob, {generation: number, descriptor: PreviewDescriptor}>();
    private scrollUntil = 0;
    private scrollTimer?: number;
    private activeJob?: PreviewJob;
    private destroyed = false;
    private generation = 0;
    private appearanceRefreshToken = 0;
    private appearanceKeyPromise?: Promise<string>;

    private readonly onScroll = (event: Event) => {
        const scroller = event.target as Element;
        if (!scroller?.contains || !Array.from(this.targets.values()).some((target) => scroller.contains(target))) return;
        this.scrollUntil = Date.now() + 200;
        window.clearTimeout(this.scrollTimer);
        this.scrollTimer = window.setTimeout((): void => {
            this.scrollUntil = 0;
            void this.drain();
        }, 200);
    };

    constructor(options: DocumentCardPreviewControllerOptions = {}) {
        this.options = options;
        document.addEventListener("scroll", this.onScroll, {capture: true, passive: true});
        this.observer = new IntersectionObserver((entries) => entries.forEach((entry) => {
            const element = entry.target as HTMLElement;
            const key = notebookRootElementKey(element);
            const job = this.jobs.get(key);
            if (this.targets.get(key) !== element || !job) return;
            this.visible.set(key, entry.isIntersecting);
            if (entry.isIntersecting && !element.dataset.previewReady && this.activeJob !== job) {
                this.enqueueProbe(job);
            } else if (!entry.isIntersecting && this.activeJob !== job) {
                const index = this.queue.indexOf(key);
                if (index > -1) this.queue.splice(index, 1);
            }
        }), {rootMargin: "40px"});
    }

    public rebind(elements: Iterable<HTMLElement>) {
        this.observer.disconnect();
        this.targets.clear();
        this.visible.clear();
        for (const element of elements) {
            const key = notebookRootElementKey(element);
            element.dataset.previewKey = key;
            this.targets.set(key, element);
            const reference = previewReferenceFromElement(element);
            if (!this.jobs.has(key) || JSON.stringify(this.jobs.get(key).reference) !== JSON.stringify(reference)) {
                this.prepared.delete(this.jobs.get(key));
                const placeholder = element.querySelector<HTMLElement>(".notebook-root__placeholder")?.cloneNode(true) as HTMLElement;
                this.jobs.set(key, {key, reference, placeholder});
            }
            this.observer.observe(element);
        }
        this.jobs.forEach((job, key) => {
            if (!this.targets.has(key)) {
                this.jobs.delete(key);
                this.prepared.delete(job);
            }
        });
    }

    public async refreshAppearance() {
        const refreshToken = ++this.appearanceRefreshToken;
        const appearanceKey = await documentCardPreviewAppearanceKey();
        const previousAppearanceKey = await this.currentAppearanceKey();
        if (this.destroyed || refreshToken !== this.appearanceRefreshToken || appearanceKey === previousAppearanceKey) {
            return;
        }
        this.appearanceKeyPromise = Promise.resolve(appearanceKey);
        this.generation++;
        this.queue.length = 0;
        this.prepared.clear();
        this.probeQueue.length = 0;
        this.targets.forEach((target, key) => {
            const preview = target.querySelector<HTMLElement>(".notebook-root__preview");
            const placeholder = this.jobs.get(key)?.placeholder;
            if (preview && placeholder) {
                preview.replaceWith(placeholder.cloneNode(true));
            }
            delete target.dataset.previewReady;
            target.dataset.previewState = "loading";
            this.observer.unobserve(target);
            this.observer.observe(target);
        });
    }

    public destroy() {
        this.destroyed = true;
        document.removeEventListener("scroll", this.onScroll, true);
        window.clearTimeout(this.scrollTimer);
        this.observer.disconnect();
        this.queue.length = 0;
        this.targets.clear();
        this.jobs.clear();
        this.prepared.clear();
        this.probeQueue.length = 0;
        this.visible.clear();
    }

    private target(job: PreviewJob) {
        const target = this.targets.get(job.key);
        return target?.isConnected ? target : undefined;
    }

    private currentAppearanceKey() {
        this.appearanceKeyPromise ||= documentCardPreviewAppearanceKey();
        return this.appearanceKeyPromise;
    }

    private async drain() {
        if (this.destroyed || this.activeJob || this.queue.length === 0 || Date.now() < this.scrollUntil) return;
        const key = this.queue.shift();
        const job = key ? this.jobs.get(key) : undefined;
        if (!job) {
            void this.drain();
            return;
        }
        this.activeJob = job;
        const generation = this.generation;
        void this.render(job).finally(() => {
            if (this.activeJob === job) this.activeJob = undefined;
            if (generation !== this.generation && !this.isStale(job, this.generation) &&
                !this.target(job)?.dataset.previewReady) this.enqueueProbe(job);
            void this.drain();
        });
    }

    private enqueue(job: PreviewJob) {
        if (!this.queue.includes(job.key)) this.queue.push(job.key);
        void this.drain();
    }

    private enqueueProbe(job: PreviewJob) {
        if (!this.probing.has(job) && !this.probeQueue.includes(job) && !this.queue.includes(job.key)) {
            this.probeQueue.push(job);
        }
        this.drainProbes();
    }

    private drainProbes() {
        // 缓存查询不等待截图完成，但限制并发，避免同时读取大量文档。
        while (!this.destroyed && this.probing.size < 4 && this.probeQueue.length) {
            const job = this.probeQueue.shift();
            const generation = this.generation;
            if (this.isStale(job, generation)) continue;
            this.probing.add(job);
            void this.probe(job, generation).finally(() => {
                this.probing.delete(job);
                if (!this.destroyed && generation !== this.generation && !this.isStale(job, this.generation)) {
                    this.enqueueProbe(job);
                }
                this.drainProbes();
            });
        }
    }

    private async probe(job: PreviewJob, generation: number) {
        try {
            await this.refreshAppearance();
            if (this.isStale(job, generation)) return;
            if (notebookRootNeedsMarkdownIdentity(job.reference.kind, job.reference.identityState,
                Boolean(job.reference.identityConflict))) {
                this.enqueue(job);
                return;
            }
            const theme = window.siyuan.config.appearance.mode === 1 ? "dark" : "light";
            const appearanceKey = await this.currentAppearanceKey();
            const sessionKey = documentCardPreviewSessionKey(job.reference, theme, appearanceKey, "medium");
            let descriptor = sessionPreviewDescriptors.get(sessionKey);
            if (!descriptor) {
                const request = this.options.request || (await import("../util/fetch")).fetchSyncPost;
                if (this.isStale(job, generation)) return;
                const response = await request("/api/notebook/prepareDocumentCardPreview", {
                    reference: job.reference, theme, appearanceKey, size: "medium", format: documentCardPreviewFormat(),
                });
                if (response.code !== 0) throw new Error(response.msg || "preview preparation failed");
                descriptor = response.data as PreviewDescriptor;
            }
            if (this.isStale(job, generation)) return;
            if (descriptor.exists) {
                cacheSessionPreviewDescriptor(sessionKey, descriptor);
                await this.installImage(job.key, descriptor.url, generation);
            } else {
                this.prepared.set(job, {generation, descriptor});
                this.enqueue(job);
            }
        } catch {
            if (!this.isStale(job, generation)) this.target(job).dataset.previewState = "failed";
        }
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
            this.visible.set(formalKey, this.visible.get(temporaryKey));
            this.visible.delete(temporaryKey);
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
        const generation = this.generation;
        try {
            await waitForPreviewPaint();
            await waitForPreviewIdle();
            if (this.isStale(job, generation)) return;
            if (Date.now() < this.scrollUntil) {
                this.enqueue(job);
                return;
            }
            const request = this.options.request || (await import("../util/fetch")).fetchSyncPost;
            if (this.isStale(job, generation)) return;
            if (notebookRootNeedsMarkdownIdentity(job.reference.kind, job.reference.identityState,
                Boolean(job.reference.identityConflict))) {
                if (window.siyuan.config.readonly) {
                    const target = this.target(job);
                    if (target) target.dataset.previewState = "failed";
                    return;
                }
                const {createMarkdownManagementOperationID} = await import("../markdown/documentManagement");
                if (this.isStale(job, generation)) return;
                const identity = await request("/api/markdown/ensureDocumentIdentity", {
                    notebook: job.reference.notebook,
                    path: job.reference.path,
                    revision: job.reference.revision,
                    operationID: createMarkdownManagementOperationID(),
                    forceNew: Boolean(job.reference.identityConflict),
                });
                if (identity.code !== 0) throw new Error(identity.msg || "document identity creation failed");
                if (this.isStale(job, generation)) return;
                this.migrateJobIdentity(job, {
                    documentID: identity.data.documentID,
                    revision: identity.data.revision,
                });
            }
            const size = "medium";
            const theme = window.siyuan.config.appearance.mode === 1 ? "dark" : "light";
            const appearanceKey = await this.currentAppearanceKey();
            if (this.isStale(job, generation)) return;
            const sessionKey = documentCardPreviewSessionKey(job.reference, theme, appearanceKey, size);
            for (let attempt = 0; attempt < 2; attempt++) {
                if (this.isStale(job, generation)) return;
                let descriptor = attempt === 0 ? sessionPreviewDescriptors.get(sessionKey) : undefined;
                const prepared = this.prepared.get(job);
                if (!descriptor && attempt === 0 && prepared?.generation === generation) descriptor = prepared.descriptor;
                if (!descriptor) {
                    const prepared = await request("/api/notebook/prepareDocumentCardPreview", {
                        reference: job.reference,
                        theme,
                        appearanceKey,
                        size,
                        format: documentCardPreviewFormat(),
                    });
                    if (prepared.code !== 0) throw new Error(prepared.msg || "preview preparation failed");
                    if (this.isStale(job, generation)) return;
                    descriptor = prepared.data as PreviewDescriptor;
                    if (descriptor.exists) {
                        cacheSessionPreviewDescriptor(sessionKey, descriptor);
                    }
                }
                if (descriptor.exists) {
                    await this.installImage(job.key, descriptor.url, generation);
                    return;
                }
                const renderPreview = this.options.renderPreview ||
                    (await import("./previewRenderer")).renderDocumentCardPreview;
                const shouldContinue = () => !this.isStale(job, generation) && Date.now() >= this.scrollUntil;
                if (!shouldContinue()) {
                    if (!this.isStale(job, generation)) this.enqueue(job);
                    return;
                }
                const blob = await renderPreview({reference: job.reference, size, shouldContinue});
                if (this.isStale(job, generation)) return;
                const formData = new FormData();
                formData.append("reference", JSON.stringify(job.reference));
                formData.append("descriptor", JSON.stringify(descriptor));
                formData.append("file", blob, `${descriptor.cacheKey}.${descriptor.format || "webp"}`);
                const stored = await request("/api/notebook/storeDocumentCardPreview", formData);
                if (stored.code === 409 && attempt === 0) continue;
                if (stored.code !== 0) throw new Error(stored.msg || "preview store failed");
                cacheSessionPreviewDescriptor(sessionKey, {...descriptor, exists: true});
                if (this.isStale(job, generation)) return;
                await this.installImage(job.key, descriptor.url, generation);
                return;
            }
        } catch (error) {
            if ((error as Error).name === "AbortError") {
                await this.refreshAppearance();
                if (!this.isStale(job, generation)) this.enqueue(job);
                return;
            }
            const target = this.isStale(job, generation) ? undefined : this.target(job);
            if (target) target.dataset.previewState = "failed";
        } finally {
            this.prepared.delete(job);
        }
    }

    private isStale(job: PreviewJob, generation: number) {
        return this.destroyed || generation !== this.generation || !this.target(job) ||
            this.jobs.get(job.key) !== job || this.visible.get(job.key) === false;
    }

    private async installImage(key: string, url: string, generation = this.generation) {
        const job = this.jobs.get(key);
        const image = await decodeDocumentCardPreviewImage(url);
        const target = this.targets.get(key);
        if (this.destroyed || generation !== this.generation || !target?.isConnected ||
            this.jobs.get(key) !== job || this.visible.get(key) === false) return;
        if (installDocumentCardPreviewImage(target, url, image)) {
            target.dataset.previewReady = "true";
            target.dataset.previewState = "ready";
        }
    }
}
