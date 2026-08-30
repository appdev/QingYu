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
    private lastAnchor: MarkdownScrollAnchor | null = null;

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
        if (!view) return this.lastAnchor;
        const viewport = this.container.getBoundingClientRect();
        if (viewport.width <= 0 || viewport.height <= 0) return this.lastAnchor;
        try {
            const selectionPosition = view.state.selection.main.head;
            const selectionRect = view.coordsAtPos(selectionPosition);
            if (selectionRect && intersectsVertically(selectionRect, viewport)) {
                this.lastAnchor = {
                    position: selectionPosition,
                    viewportOffset: selectionRect.top - viewport.top,
                };
                return this.lastAnchor;
            }

            const position = view.posAtCoords({
                x: viewport.left + viewport.width / 2,
                y: viewport.top + viewport.height / 2,
            }, false);
            if (position === null) return this.lastAnchor;
            const positionRect = view.coordsAtPos(position);
            if (!positionRect) return this.lastAnchor;
            this.lastAnchor = {
                position,
                viewportOffset: positionRect.top - viewport.top,
            };
            return this.lastAnchor;
        } catch {
            return this.lastAnchor;
        }
    }

    public restoreAnchor(anchor: MarkdownScrollAnchor) {
        this.lastAnchor = {...anchor};
        if (this.restoreFrame !== null) {
            window.cancelAnimationFrame(this.restoreFrame);
        }
        const restoreVisiblePosition = (remainingCorrections: number) => {
            this.restoreFrame = null;
            const view = this.getView();
            if (!view) return;
            const viewport = this.container.getBoundingClientRect();
            if (viewport.width <= 0 || viewport.height <= 0) return;
            try {
                const position = Math.max(0, Math.min(anchor.position, view.state.doc.length));
                const positionRect = view.coordsAtPos(position);
                const positionTop = positionRect?.top ?? view.lineBlockAt(position).top + view.documentTop;
                this.container.scrollTop += positionTop - viewport.top - anchor.viewportOffset;
                if (remainingCorrections > 0) {
                    this.restoreFrame = window.requestAnimationFrame(() => restoreVisiblePosition(remainingCorrections - 1));
                }
            } catch {
                // CodeMirror 视口尚未稳定时保留锚点，等待下一次可见状态恢复。
            }
        };
        this.restoreFrame = window.requestAnimationFrame(() => {
            // 重新配置后 CodeMirror 会在下一帧完成区块测量；在测量完成后再恢复锚点，避免按旧高度滚动。
            this.restoreFrame = window.requestAnimationFrame(() => restoreVisiblePosition(3));
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
