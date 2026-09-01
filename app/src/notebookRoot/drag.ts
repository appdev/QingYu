import {fetchSyncPost} from "../util/fetch";
import {moveMarkdownFileTo} from "../markdown/fileActions";
import type {NotebookRootDocument} from "./types";
export {classifyNotebookRootDrop} from "./rules";
import {classifyNotebookRootDrop, NOTEBOOK_ROOT_DOCUMENT_MIME} from "./rules";

export class NotebookRootDragController {
    private dragged?: HTMLElement;
    private accepted = false;

    constructor(private readonly options: {
        element: HTMLElement;
        notebook: string;
        sortMode: number;
        documents: NotebookRootDocument[];
        reload(): Promise<void>;
    }) {
        options.element.querySelectorAll<HTMLElement>(".notebook-root__document").forEach((card) => this.bindCard(card));
        options.element.addEventListener("dragover", this.dragOver);
        options.element.addEventListener("drop", this.dropOnRoot);
    }

    public destroy() {
        this.options.element.removeEventListener("dragover", this.dragOver);
        this.options.element.removeEventListener("drop", this.dropOnRoot);
    }

    private bindCard(card: HTMLElement) {
        card.draggable = true;
        card.addEventListener("dragstart", (event) => {
            this.dragged = card;
            this.accepted = false;
            card.classList.add("notebook-root__document--dragging");
            const rect = card.getBoundingClientRect();
            event.dataTransfer.setDragImage(card,
                Math.min(rect.width, Math.max(0, event.clientX - rect.left)),
                Math.min(rect.height, Math.max(0, event.clientY - rect.top)));
            const payload = JSON.stringify({
                kind: card.dataset.kind,
                notebook: card.dataset.notebook,
                path: card.dataset.path,
                id: card.dataset.id,
            });
            event.dataTransfer.setData(NOTEBOOK_ROOT_DOCUMENT_MIME, payload);
            event.dataTransfer.effectAllowed = "move";
            window.siyuan.dragTitle = card.querySelector(".notebook-root__document-title")?.textContent?.trim() || "";
            window.siyuan.dragElement = document.createElement("div");
            window.siyuan.dragElement.dataset.notebookRootDocument = payload;
        });
        card.addEventListener("dragover", (event) => {
            if (!event.dataTransfer.types.includes(NOTEBOOK_ROOT_DOCUMENT_MIME)) return;
            event.preventDefault();
        });
        card.addEventListener("drop", (event) => {
            event.preventDefault();
            event.stopPropagation();
            void this.handleDrop(event, card);
        });
        card.addEventListener("dragend", () => {
            if (window.siyuan.dragElement?.dataset.notebookRootDropAccepted === "true") {
                this.accepted = true;
            }
            document.querySelectorAll(".file-tree__notebook-drop-target").forEach((item) => {
                item.classList.remove("file-tree__notebook-drop-target");
            });
            card.classList.remove("notebook-root__document--dragging");
            if (!this.accepted) {
                card.classList.add("notebook-root__document--spring-back");
                setTimeout(() => card.classList.remove("notebook-root__document--spring-back"), 220);
            }
            this.dragged = undefined;
            if (window.siyuan.dragElement?.dataset.notebookRootDocument) {
                window.siyuan.dragElement = undefined;
                window.siyuan.dragTitle = "";
            }
        });
    }

    private dragOver = (event: DragEvent) => {
        if (event.dataTransfer.types.includes(NOTEBOOK_ROOT_DOCUMENT_MIME)) event.preventDefault();
    };

    private dropOnRoot = (event: DragEvent) => {
        event.preventDefault();
        void this.handleDrop(event);
    };

    private async handleDrop(event: DragEvent, targetCard?: HTMLElement) {
        let source: {kind: "sy" | "markdown", notebook: string, path: string, id: string};
        try {
            source = JSON.parse(event.dataTransfer.getData(NOTEBOOK_ROOT_DOCUMENT_MIME));
        } catch {
            return;
        }
        const action = classifyNotebookRootDrop(source, {notebook: this.options.notebook, root: true}, this.options.sortMode);
        if (action === "spring-back") return;
        if (action === "reorder") {
            if (!targetCard || targetCard === this.dragged) return;
            const cards = Array.from(this.options.element.querySelectorAll<HTMLElement>(".notebook-root__document"));
            const paths = cards.filter((card) => card !== this.dragged).map((card) => card.dataset.path);
            const targetIndex = paths.indexOf(targetCard.dataset.path);
            paths.splice(targetIndex, 0, source.path);
            const response = await fetchSyncPost("/api/filetree/changeSort", {
                notebook: this.options.notebook,
                paths,
                operationID: `notebook-root-${Date.now()}`,
            });
            this.accepted = response.code === 0;
        } else if (source.kind === "markdown") {
            this.accepted = await moveMarkdownFileTo(source.notebook, source.path, this.options.notebook, "/");
        } else {
            const response = await fetchSyncPost("/api/filetree/moveDocs", {
                fromPaths: [source.path],
                toNotebook: this.options.notebook,
                toPath: "/",
            });
            this.accepted = response.code === 0;
        }
        await this.options.reload();
    }
}
