import {Model} from "../layout/Model";
import type {App} from "../index";
import type {Tab} from "../layout/Tab";
import type {MarkdownEditor} from "./MarkdownEditor";
import {markdownEditorRegistry, type RegisteredMarkdownEditor} from "./markdownEditorRegistry";
import {MarkdownOutlineView} from "./markdownOutlineView";
import {ActiveMarkdownOutlines} from "./activeMarkdownOutlines";

const activeOutlines = new ActiveMarkdownOutlines<MarkdownOutline>();

export const getMarkdownOutlineBySourceKey = (sourceKey: string) => activeOutlines.get(sourceKey);

export class MarkdownOutline extends Model {
    public readonly element: HTMLElement;
    public sourceKey: string;
    private unsubscribe?: () => void;
    private unsubscribeRegistry?: () => void;
    private editor?: RegisteredMarkdownEditor;
    private readonly view: MarkdownOutlineView;
    private preserveEditorStateOnDestroy = false;

    constructor(options: {
        app: App;
        tab?: Tab;
        element?: HTMLElement;
        sourceKey: string;
        editor: MarkdownEditor | (() => MarkdownEditor | undefined);
    }) {
        super({app: options.app});
        this.element = options.tab?.panelElement || options.element;
        this.sourceKey = options.sourceKey;
        activeOutlines.register(this.sourceKey, this);
        this.view = new MarkdownOutlineView(this.element, {
            filter: window.siyuan.languages.filterKeywordEnter,
            outline: window.siyuan.languages.outline,
        }, (position) => this.editor?.focusPosition(position));
        const initialEditor = typeof options.editor === "function" ? options.editor() : options.editor;
        if (initialEditor) this.connectEditor(initialEditor);
        this.subscribeRegistry();
    }

    private subscribeRegistry() {
        this.unsubscribeRegistry?.();
        this.unsubscribeRegistry = markdownEditorRegistry.subscribe(this.sourceKey, (editor, migratedKey) => {
            if (migratedKey) {
                const previousSourceKey = this.sourceKey;
                this.sourceKey = migratedKey;
                activeOutlines.migrate(previousSourceKey, this.sourceKey, this);
                this.subscribeRegistry();
                return;
            }
            if (editor) this.connectEditor(editor);
            else if (this.editor) this.invalidate();
        });
    }

    private connectEditor(editor: RegisteredMarkdownEditor) {
        if (this.editor === editor && this.unsubscribe) return;
        this.unsubscribe?.();
        this.editor = editor;
        this.editor.setOutlineOpen(true);
        this.unsubscribe = editor.subscribeOutline((items) => {
            this.view.update(items);
        });
    }

    public close(preserveEditorState = false, isSaveLayout = true) {
        this.preserveEditorStateOnDestroy = preserveEditorState;
        void this.parent?.parent.removeTab(this.parent.id, false, false, isSaveLayout);
    }

    private invalidate() {
        this.unsubscribe?.();
        this.unsubscribe = undefined;
        this.editor = undefined;
        if (this.parent) this.parent.close();
        else this.destroy();
    }

    public destroy() {
        activeOutlines.unregister(this.sourceKey, this);
        if (!this.preserveEditorStateOnDestroy) this.editor?.setOutlineOpen(false);
        this.unsubscribeRegistry?.();
        this.unsubscribeRegistry = undefined;
        this.unsubscribe?.();
        this.unsubscribe = undefined;
        this.editor = undefined;
        this.view.destroy();
    }
}
