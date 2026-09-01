import {Constants} from "../constants";
import {setStorageVal} from "../protyle/util/compatibility";
import type {NotebookRootView} from "./types";

const validViews = new Set<NotebookRootView>(["large", "masonry", "list"]);

export const notebookRootView = (notebookID: string): NotebookRootView => {
    const values = window.siyuan.storage[Constants.LOCAL_NOTEBOOK_ROOT_VIEW] as Record<string, unknown> | undefined;
    const value = values?.[notebookID];
    return validViews.has(value as NotebookRootView) ? value as NotebookRootView : "masonry";
};

export const setNotebookRootView = (notebookID: string, view: NotebookRootView) => {
    const values = {
        ...((window.siyuan.storage[Constants.LOCAL_NOTEBOOK_ROOT_VIEW] || {}) as Record<string, NotebookRootView>),
        [notebookID]: view,
    };
    window.siyuan.storage[Constants.LOCAL_NOTEBOOK_ROOT_VIEW] = values;
    setStorageVal(Constants.LOCAL_NOTEBOOK_ROOT_VIEW, values);
};

export const removeNotebookRootView = (notebookID: string) => {
    const values = {...((window.siyuan.storage[Constants.LOCAL_NOTEBOOK_ROOT_VIEW] || {}) as Record<string, NotebookRootView>)};
    delete values[notebookID];
    window.siyuan.storage[Constants.LOCAL_NOTEBOOK_ROOT_VIEW] = values;
    setStorageVal(Constants.LOCAL_NOTEBOOK_ROOT_VIEW, values);
};
