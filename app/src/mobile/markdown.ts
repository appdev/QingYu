import {App} from "../index";
import {MarkdownEditor} from "../markdown/MarkdownEditor";
import {closePanel} from "./util/closePanel";
import {setEditor} from "./util/setEmpty";
import {
    closeMobileMarkdownEditor,
    setMobileMarkdownEditor,
} from "./markdownState";

export {closeMobileMarkdownEditor, getMobileMarkdownEditor} from "./markdownState";

export const openMobileMarkdownFile = (app: App, notebookId: string, path: string, name: string) => {
    closeMobileMarkdownEditor();
    const containerElement = document.getElementById("editor");
    const hiddenElements = Array.from(containerElement.children) as HTMLElement[];
    hiddenElements.forEach((item) => item.classList.add("fn__none"));
    const markdownElement = document.createElement("div");
    markdownElement.className = "fn__flex-1 fn__flex-column";
    containerElement.append(markdownElement);
    const editor = new MarkdownEditor({
        app,
        element: markdownElement,
        notebookId,
        path,
    });
    setMobileMarkdownEditor(editor, hiddenElements);
    const toolbarNameElement = document.getElementById("toolbarName") as HTMLInputElement;
    toolbarNameElement.value = name;
    setEditor();
    closePanel();
};
