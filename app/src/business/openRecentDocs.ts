import {fetchPost, fetchSyncPost} from "../util/fetch";
import {unicode2Emoji} from "../emoji";
import {Constants} from "../constants";
import {escapeHtml} from "../util/escape";
import {isWindow} from "../util/functions";
import {setStorageVal, updateHotkeyTip} from "../protyle/util/compatibility";
import {getAllDocks} from "../layout/getAll";
import {Dialog} from "../dialog";
import {focusByRange} from "../protyle/util/selection";
import {hasClosestByClassName} from "../protyle/util/hasClosest";
import {hideElements} from "../protyle/ui/hideElements";
import {openMarkdownFile} from "../editor/util";
import {openRecentDocument, RecentDocumentItem, renderRecentDocumentItems} from "../markdown/recentDocuments";
import type {App} from "../index";
import {getAllModels, getAllWnds} from "../layout/getAll";
import type {Wnd} from "../layout/Wnd";

const recentDocumentsApp = (app?: App) => {
    if (app) return app;
    const modelApp = getAllModels().markdown[0]?.app || getAllModels().editor[0]?.app;
    if (modelApp) return modelApp;
    const wnds: Wnd[] = [];
    if (window.siyuan.layout.centerLayout) getAllWnds(window.siyuan.layout.centerLayout, wnds);
    return (wnds[0] as unknown as {app?: App})?.app;
};

const recentDocumentItems = (data: Record<string, unknown>[]): RecentDocumentItem[] => data.map((item) => item.kind === "markdown" ? {
    kind: "markdown",
    notebook: item.notebook as string,
    path: item.path as string,
    title: item.title as string,
    icon: item.icon as string,
    viewedAt: item.viewedAt as number,
    closedAt: item.closedAt as number,
    openAt: item.openAt as number,
    updated: item.updated as number,
} : {
    kind: "native",
    rootID: item.rootID as string,
    title: item.title as string,
    icon: item.icon as string,
    viewedAt: item.viewedAt as number,
    closedAt: item.closedAt as number,
    openAt: item.openAt as number,
    updated: item.updated as number,
});

const renderRecentDocsContent = async (data: Record<string, unknown>[], element: Element, key?: string) => {
    const items = recentDocumentItems(data).filter((item) => !key || item.title.toLowerCase().includes(key.toLowerCase()));
    const tabHtml = renderRecentDocumentItems(items).replaceAll('<span class="b3-list-item__text">',
        `${unicode2Emoji(window.siyuan.storage[Constants.LOCAL_IMAGES].file, "b3-list-item__graphic", true)}<span class="b3-list-item__text">`);
    let switchPath = "";
    if (tabHtml) {
        const firstItem = items[0];
        if (firstItem.kind === "markdown") {
            const notebook = window.siyuan.notebooks.find((item) => item.id === firstItem.notebook);
            switchPath = escapeHtml(`${notebook?.name || firstItem.notebook}${firstItem.path}`);
        } else {
            const pathResponse = await fetchSyncPost("/api/filetree/getFullHPathByID", {id: firstItem.rootID});
            switchPath = escapeHtml(pathResponse.data);
        }
    }
    let dockHtml = "";
    if (!isWindow()) {
        let docIndex = 0;
        getAllDocks().forEach((item) => {
            if (!key || item.title.toLowerCase().includes(key.toLowerCase())) {
                dockHtml += `<li data-type="${item.type}" data-index="${docIndex}" class="b3-list-item${!switchPath ? " b3-list-item--focus" : ""}">
    <svg class="b3-list-item__graphic"><use xlink:href="#${item.icon}"></use></svg>
    <span class="b3-list-item__text">${item.title}</span>
    <span class="b3-list-item__meta">${updateHotkeyTip(item.hotkey)}</span>
</li>`;
                if (!switchPath) {
                    switchPath = item.title;
                }
                docIndex++;
            }
        });
        dockHtml = '<ul class="b3-list b3-list--background" style="overflow: auto;width: 200px;">' + dockHtml + "</ul>";
    }

    const pathElement = element.querySelector(".switch-doc__path");
    pathElement.innerHTML = switchPath;
    pathElement.previousElementSibling.innerHTML = `<div class="fn__flex fn__flex-1" style="overflow: auto;">
    ${dockHtml}
    <ul style="${isWindow() ? "border-left: 0;" : ""}min-width: 360px;" class="b3-list b3-list--background fn__flex-1">${tabHtml}</ul>
</div>`;
};

export const openRecentDocs = (app?: App) => {
    const openRecentDocsDialog = window.siyuan.dialogs.find(item => {
        if (item.element.getAttribute("data-key") === Constants.DIALOG_RECENTDOCS) {
            return true;
        }
    });
    if (openRecentDocsDialog) {
        hideElements(["dialog"]);
        return;
    }
    const sortBy = window.siyuan.storage[Constants.LOCAL_RECENT_DOCS].type as TRecentDocsSort;
    fetchPost("/api/storage/getRecentDocs", {sortBy}, (response) => {
        let range: Range;
        if (getSelection().rangeCount > 0) {
            range = getSelection().getRangeAt(0);
        }
        const dialog = new Dialog({
            positionId: Constants.DIALOG_RECENTDOCS,
            title: `<div class="fn__flex">
<div class="fn__flex-center">${window.siyuan.languages.recentDocs}</div>
<div class="fn__flex-1"></div>
<input placeholder="${window.siyuan.languages.search}" class="b3-text-field fn__size200">
<span class="fn__space"></span>
<div class="fn__flex-center">
    <select class="b3-select" id="recentDocsSort">
        <option value="viewedAt">${window.siyuan.languages.recentViewed}</option>
        <option value="updated">${window.siyuan.languages.recentModified}</option>
        <option value="openAt">${window.siyuan.languages.recentOpened}</option>
        <option value="closedAt">${window.siyuan.languages.recentClosed}</option>
    </select>
</div>
</div>`,
            content: `<div class="fn__flex-column switch-doc">
    <div class="fn__flex fn__flex-1" style="overflow: auto;"></div>
    <div class="switch-doc__path"></div>
</div>`,
            height: "80vh",
            destroyCallback: () => {
                if (range && range.getBoundingClientRect().height !== 0) {
                    focusByRange(range);
                }
            }
        });
        const sortSelect = dialog.element.querySelector("#recentDocsSort") as HTMLSelectElement;
        sortSelect.value = sortBy;
        const searchElement = dialog.element.querySelector("input");
        searchElement.focus();
        searchElement.addEventListener("compositionend", () => {
            renderRecentDocsContent(response.data, dialog.element, searchElement.value);
        });
        searchElement.addEventListener("input", (event: InputEvent) => {
            if (event.isComposing) {
                return;
            }
            renderRecentDocsContent(response.data, dialog.element, searchElement.value);
        });
        dialog.element.setAttribute("data-key", Constants.DIALOG_RECENTDOCS);
        dialog.element.addEventListener("click", (event) => {
            const liElement = hasClosestByClassName(event.target as HTMLElement, "b3-list-item");
            if (liElement) {
                dialog.element.querySelector(".b3-list-item--focus").classList.remove("b3-list-item--focus");
                liElement.classList.add("b3-list-item--focus");
                if (liElement.dataset.markdownPath) {
                    const currentApp = recentDocumentsApp(app);
                    if (currentApp) void openRecentDocument(currentApp, {
                        kind: "markdown",
                        notebook: liElement.dataset.markdownNotebook,
                        path: liElement.dataset.markdownPath,
                        title: liElement.querySelector(".b3-list-item__text").textContent,
                    }, {openMarkdown: openMarkdownFile, openNative: async () => undefined});
                    hideElements(["dialog"]);
                } else {
                    window.dispatchEvent(new KeyboardEvent("keydown", {key: "Enter"}));
                }
                event.stopPropagation();
                event.preventDefault();
            }
        });
        dialog.element.addEventListener("keydown", (event: KeyboardEvent) => {
            if (event.key.startsWith("Arrow")) {
                window.setTimeout(() => {
                    const item = dialog.element.querySelector<HTMLElement>(".b3-list-item--focus[data-markdown-path]");
                    if (!item) return;
                    const notebook = window.siyuan.notebooks.find((notebook) => notebook.id === item.dataset.markdownNotebook);
                    dialog.element.querySelector(".switch-doc__path").textContent =
                        `${notebook?.name || item.dataset.markdownNotebook}${item.dataset.markdownPath}`;
                });
                return;
            }
            if (event.key !== "Enter") return;
            const item = dialog.element.querySelector<HTMLElement>(".b3-list-item--focus[data-markdown-path]");
            if (!item) return;
            event.preventDefault();
            event.stopPropagation();
            const currentApp = recentDocumentsApp(app);
            if (currentApp) void openRecentDocument(currentApp, {
                kind: "markdown",
                notebook: item.dataset.markdownNotebook,
                path: item.dataset.markdownPath,
                title: item.querySelector(".b3-list-item__text").textContent,
            }, {openMarkdown: openMarkdownFile, openNative: async () => undefined});
            hideElements(["dialog"]);
        });

        // 添加排序下拉框事件监听
        sortSelect.addEventListener("change", () => {
            // 重新调用 API 获取排序后的数据
            fetchPost("/api/storage/getRecentDocs", {sortBy: sortSelect.value}, (newResponse) => {
                response = newResponse;
                renderRecentDocsContent(newResponse.data, dialog.element, searchElement.value);
            });
            window.siyuan.storage[Constants.LOCAL_RECENT_DOCS].type = sortSelect.value;
            setStorageVal(Constants.LOCAL_RECENT_DOCS, window.siyuan.storage[Constants.LOCAL_RECENT_DOCS]);
        });

        renderRecentDocsContent(response.data, dialog.element);
    });
};
