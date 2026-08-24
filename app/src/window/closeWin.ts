import {App} from "../index";
import {Constants} from "../constants";
import {ipcRenderer} from "electron";
import {getAllModels} from "../layout/getAll";

export const closeWindow = async (app: App) => {
    for (const editor of getAllModels().markdown.filter((item) => item.externalCapabilityId)) {
        if (!await editor.prepareClose()) return;
    }
    for (let i = 0; i < app.plugins.length; i++) {
        const plugin = app.plugins[i];
        try {
            await plugin.onunload();
        } catch (e) {
            console.error(e);
        }
        try {
            await plugin.kernel?.destroy();
        } catch (e) {
            console.error(e);
        }
    }
    ipcRenderer.send(Constants.SIYUAN_CMD, "destroy");
};
