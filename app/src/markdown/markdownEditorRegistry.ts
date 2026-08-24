type RegistryListener<T> = (editor: T | undefined, migratedKey?: string) => void;

export interface RegisteredMarkdownEditor {
    focusPosition(position: number): void;
    setOutlineOpen(open: boolean): void;
    subscribeOutline(listener: (items: readonly import("./outlineModel").MarkdownOutlineItemWithPosition[]) => void): () => void;
}

export class MarkdownEditorRegistry<T extends object> {
    private readonly editors = new Map<string, Set<T>>();
    private readonly listeners = new Map<string, Set<RegistryListener<T>>>();

    get(sourceKey: string) {
        const editors = this.editors.get(sourceKey);
        return editors ? Array.from(editors).at(-1) : undefined;
    }

    register(sourceKey: string, editor: T) {
        const migratedKeys: string[] = [];
        for (const [currentKey, currentEditors] of this.editors) {
            if (!currentEditors.has(editor) || currentKey === sourceKey) continue;
            currentEditors.delete(editor);
            if (currentEditors.size === 0) this.editors.delete(currentKey);
            migratedKeys.push(currentKey);
        }
        let editors = this.editors.get(sourceKey);
        if (!editors) {
            editors = new Set();
            this.editors.set(sourceKey, editors);
        }
        editors.delete(editor);
        editors.add(editor);
        migratedKeys.forEach((currentKey) => {
            const fallback = this.get(currentKey);
            this.listeners.get(currentKey)?.forEach((listener) => listener(fallback || editor, fallback ? undefined : sourceKey));
        });
        this.listeners.get(sourceKey)?.forEach((listener) => listener(editor));
    }

    unregister(editor: T) {
        for (const [sourceKey, currentEditors] of this.editors) {
            if (!currentEditors.delete(editor)) continue;
            if (currentEditors.size === 0) this.editors.delete(sourceKey);
            const fallback = this.get(sourceKey);
            this.listeners.get(sourceKey)?.forEach((listener) => listener(fallback));
        }
    }

    subscribe(sourceKey: string, listener: RegistryListener<T>) {
        let listeners = this.listeners.get(sourceKey);
        if (!listeners) {
            listeners = new Set();
            this.listeners.set(sourceKey, listeners);
        }
        listeners.add(listener);
        listener(this.get(sourceKey));
        return () => {
            listeners?.delete(listener);
            if (listeners?.size === 0) this.listeners.delete(sourceKey);
        };
    }
}

export class MarkdownEditorRegistration<T extends object> {
    private active = true;

    constructor(private readonly registry: MarkdownEditorRegistry<T>, private readonly editor: T) {}

    register(sourceKey: string) {
        if (this.active) this.registry.register(sourceKey, this.editor);
    }

    destroy() {
        if (!this.active) return;
        this.active = false;
        this.registry.unregister(this.editor);
    }
}

export const markdownEditorRegistry = new MarkdownEditorRegistry<RegisteredMarkdownEditor>();
