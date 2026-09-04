import {calculateNotebookRootMasonryLayout} from "./masonryLayout";

export class NotebookRootMasonryController {
    private readonly observer: ResizeObserver;
    private frame = 0;

    constructor(private readonly documents: HTMLElement) {
        this.observer = new ResizeObserver(() => this.schedule());
        this.observer.observe(documents);
    }

    public layoutNow() {
        const width = this.documents.clientWidth;
        if (width <= 0) return;
        const cards = Array.from(
            this.documents.querySelectorAll<HTMLElement>(".notebook-root__document--masonry"),
        );
        if (cards.length === 0) {
            this.documents.style.removeProperty("height");
            return;
        }
        const ratios = cards.map((card) => {
            const ratio = Number.parseFloat(card.dataset.cardRatio || "");
            return Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
        });
        const layout = calculateNotebookRootMasonryLayout({containerWidth: width, ratios});
        cards.forEach((card, index) => {
            const placement = layout.placements[index];
            card.style.left = `${placement.left}px`;
            card.style.top = `${placement.top}px`;
            card.style.width = `${placement.width}px`;
        });
        this.documents.style.height = `${layout.height}px`;
    }

    public schedule() {
        if (this.frame) return;
        this.frame = requestAnimationFrame(() => {
            this.frame = 0;
            this.layoutNow();
        });
    }

    public destroy() {
        this.observer.disconnect();
        if (this.frame) cancelAnimationFrame(this.frame);
        this.frame = 0;
    }
}
