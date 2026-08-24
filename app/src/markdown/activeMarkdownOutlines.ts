export class ActiveMarkdownOutlines<T extends object> {
    private readonly outlines = new Map<string, Set<T>>();

    get(sourceKey: string) {
        const outlines = this.outlines.get(sourceKey);
        return outlines ? Array.from(outlines).at(-1) : undefined;
    }

    register(sourceKey: string, outline: T) {
        let outlines = this.outlines.get(sourceKey);
        if (!outlines) {
            outlines = new Set();
            this.outlines.set(sourceKey, outlines);
        }
        outlines.delete(outline);
        outlines.add(outline);
    }

    unregister(sourceKey: string, outline: T) {
        const outlines = this.outlines.get(sourceKey);
        if (!outlines?.delete(outline)) return;
        if (outlines.size === 0) this.outlines.delete(sourceKey);
    }

    migrate(previousSourceKey: string, sourceKey: string, outline: T) {
        this.unregister(previousSourceKey, outline);
        this.register(sourceKey, outline);
    }
}
