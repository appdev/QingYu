import type {Compartment} from "@codemirror/state";
import type {EditorView} from "@codemirror/view";
import {
    createSiyuanMarkraExtension,
    type SiyuanMarkraExtensionOptions,
} from "./markraExtension";

export const reconfigureSiyuanMarkraExtension = (
    view: EditorView,
    compartment: Compartment,
    options: SiyuanMarkraExtensionOptions,
) => {
    view.dispatch({
        effects: compartment.reconfigure(createSiyuanMarkraExtension(options)),
    });
};
