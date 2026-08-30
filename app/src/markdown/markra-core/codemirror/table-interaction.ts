import {
    StateEffect,
    StateField,
    type EditorState,
    type Extension,
} from "@codemirror/state";
import type {EditorView} from "@codemirror/view";

export interface MarkdownTableInteractionState {
    readonly activeTableId: string | null;
    readonly hoverTableId: string | null;
}

export const setActiveMarkdownTable = StateEffect.define<string | null>();
export const setHoveredMarkdownTable = StateEffect.define<string | null>();

export class MarkdownTableInteractionController {
    private current: MarkdownTableInteractionState = {activeTableId: null, hoverTableId: null};

    read() {
        return this.current;
    }

    update(value: MarkdownTableInteractionState) {
        this.current = value;
    }

    restore(view: EditorView) {
        view.dispatch({effects: [
            setActiveMarkdownTable.of(this.current.activeTableId),
            setHoveredMarkdownTable.of(this.current.hoverTableId),
        ]});
    }
}

export const createMarkdownTableInteractionController = (
    controller = new MarkdownTableInteractionController(),
): {
    readonly extension: Extension;
    readonly field: StateField<MarkdownTableInteractionState>;
} => {
    const field = StateField.define<MarkdownTableInteractionState>({
        create() {
            return controller.read();
        },
        update(value, transaction) {
            let activeTableId = value.activeTableId;
            let hoverTableId = value.hoverTableId;
            for (const effect of transaction.effects) {
                if (effect.is(setActiveMarkdownTable)) {
                    activeTableId = effect.value;
                } else if (effect.is(setHoveredMarkdownTable)) {
                    hoverTableId = effect.value;
                }
            }
            const next = activeTableId === value.activeTableId && hoverTableId === value.hoverTableId
                ? value
                : {activeTableId, hoverTableId};
            // Compartment 重配置时旧字段仍会收到更新，不能让它覆盖新字段刚恢复的交互状态。
            if (!transaction.reconfigured) controller.update(next);
            return next;
        },
    });
    return {extension: field, field};
};

export const markdownTableInteraction = (
    state: EditorState,
    field: StateField<MarkdownTableInteractionState>,
) => state.field(field, false) ?? {activeTableId: null, hoverTableId: null};

export const markdownActiveTableId = (
    state: EditorState,
    field: StateField<MarkdownTableInteractionState>,
) => markdownTableInteraction(state, field).activeTableId;

export const markdownHoveredTableId = (
    state: EditorState,
    field: StateField<MarkdownTableInteractionState>,
) => markdownTableInteraction(state, field).hoverTableId;
