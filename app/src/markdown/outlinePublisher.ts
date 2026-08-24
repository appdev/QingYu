import {getMarkdownOutlineWithPositions, type MarkdownOutlineItemWithPosition} from "./markra-core/markdown/markdown";

export class MarkdownOutlinePublisher {
    private readonly listeners = new Set<(items: readonly MarkdownOutlineItemWithPosition[]) => void>();

    constructor(private readonly getSource: () => string | undefined) {}

    subscribe(listener: (items: readonly MarkdownOutlineItemWithPosition[]) => void) {
        this.listeners.add(listener);
        const source = this.getSource();
        if (source !== undefined) listener(getMarkdownOutlineWithPositions(source));
        return () => this.listeners.delete(listener);
    }

    publish() {
        const source = this.getSource();
        if (source === undefined) return;
        const items = getMarkdownOutlineWithPositions(source);
        this.listeners.forEach((listener) => listener(items));
    }

    destroy() {
        this.listeners.clear();
    }
}
