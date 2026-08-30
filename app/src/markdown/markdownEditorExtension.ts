import type {Compartment} from "@codemirror/state";
import type {EditorView} from "@codemirror/view";
import {
    createSiyuanMarkraExtension,
    type SiyuanMarkraExtensionOptions,
} from "./markraExtension";
import type {MarkdownScrollAnchor} from "./documentScroll";

export interface MarkdownReconfigureContinuity {
    captureAnchor(): MarkdownScrollAnchor | null;
    restoreAnchor(anchor: MarkdownScrollAnchor): void;
}

export const reconfigureSiyuanMarkraExtension = (
    view: EditorView,
    compartment: Compartment,
    options: SiyuanMarkraExtensionOptions,
    continuity?: MarkdownReconfigureContinuity,
) => {
    const anchor = continuity?.captureAnchor();
    view.dispatch({
        effects: compartment.reconfigure(createSiyuanMarkraExtension(options)),
    });
    options.tableInteraction?.restore(view);
    if (anchor) continuity.restoreAnchor(anchor);
};
