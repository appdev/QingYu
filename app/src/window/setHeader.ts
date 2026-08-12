import {isWindow} from "../util/functions";
import {getAllTabs} from "../layout/getAll";
import {Editor} from "../editor";
import {Asset} from "../asset";
import {Constants} from "../constants";
import {MarkdownEditor} from "../markdown/MarkdownEditor";

export const setModelsHash = () => {
    if (!isWindow()) {
        return;
    }
    let hash = "";
    getAllTabs().forEach(tab => {
        if (!tab.model) {
            const initTab = tab.headElement.getAttribute("data-initdata");
            if (initTab) {
                const initTabData = JSON.parse(initTab);
                if (initTabData.instance === "Editor") {
                    hash += initTabData.rootId + Constants.ZWSP;
                }
            }
        } else if (tab.model instanceof Editor) {
            hash += tab.model.editor.protyle.block.rootID + Constants.ZWSP;
        } else if (tab.model instanceof Asset) {
            hash += tab.model.path + Constants.ZWSP;
        } else if (tab.model instanceof MarkdownEditor) {
            hash += tab.model.notebookId + tab.model.path + Constants.ZWSP;
        }
    });
    window.location.hash = hash;
};
