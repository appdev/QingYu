import type {EditorView} from "@codemirror/view";

export interface MarkdownScrollAnchor {
    position: number;
    viewportOffset: number;
}

const intersectsVertically = (
    element: Pick<DOMRect, "bottom" | "top">,
    viewport: Pick<DOMRect, "bottom" | "top">,
) =>
    element.bottom >= viewport.top && element.top <= viewport.bottom;

export class MarkdownDocumentScrollController {
    private restoreFrame: number | null = null;

    constructor(
        private readonly getView: () => EditorView | undefined,
        private readonly container: HTMLElement,
    ) {
    }

    public scrollContainer() {
        return this.container;
    }

    public captureAnchor(): MarkdownScrollAnchor | null {
        const view = this.getView();
        if (!view) return null;
        const viewport = this.container.getBoundingClientRect();
        const selectionPosition = view.state.selection.main.head;
        const selectionRect = view.coordsAtPos(selectionPosition);
        if (selectionRect && intersectsVertically(selectionRect, viewport)) {
            return {
                position: selectionPosition,
                viewportOffset: selectionRect.top - viewport.top,
            };
        }

        const position = view.posAtCoords({
            x: viewport.left + viewport.width / 2,
            y: viewport.top + viewport.height / 2,
        }, false);
        if (position === null) return null;
        const positionRect = view.coordsAtPos(position);
        if (!positionRect) return null;
        return {
            position,
            viewportOffset: positionRect.top - viewport.top,
        };
    }

    public restoreAnchor(anchor: MarkdownScrollAnchor) {
        if (this.restoreFrame !== null) {
            window.cancelAnimationFrame(this.restoreFrame);
        }
        this.restoreFrame = window.requestAnimationFrame(() => {
            this.restoreFrame = null;
            const view = this.getView();
            if (!view) return;
            const position = Math.max(0, Math.min(anchor.position, view.state.doc.length));
            const positionRect = view.coordsAtPos(position);
            if (!positionRect) return;
            const viewport = this.container.getBoundingClientRect();
            this.container.scrollTop += positionRect.top - viewport.top - anchor.viewportOffset;
        });
    }

    public scrollPositionIntoView(position: number, viewportOffset = 0) {
        this.restoreAnchor({position, viewportOffset});
    }

    public destroy() {
        if (this.restoreFrame === null) return;
        window.cancelAnimationFrame(this.restoreFrame);
        this.restoreFrame = null;
    }
}
