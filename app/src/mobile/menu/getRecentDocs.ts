import {fetchPost} from "../../util/fetch";
import {unicode2Emoji} from "../../emoji";
import {Constants} from "../../constants";
import {hasClosestByClassName} from "../../protyle/util/hasClosest";
import {openModel} from "./model";
import {openMobileFileById} from "../editor";
import {App} from "../../index";
import {openMobileMarkdownFile} from "../markdown";
import {openRecentDocument, RecentDocumentItem, renderRecentDocumentItems} from "../../markdown/recentDocuments";

export const getRecentDocs = (app: App) => {
    fetchPost("/api/storage/getRecentDocs", {sortBy: "viewedAt"}, (response) => {
        const items: RecentDocumentItem[] = response.data.map((item: Record<string, unknown>) => item.kind === "markdown" ? {
            kind: "markdown", notebook: item.notebook as string, path: item.path as string, title: item.title as string,
        } : {kind: "native", rootID: item.rootID as string, title: item.title as string});
        const html = renderRecentDocumentItems(items).replaceAll('<span class="b3-list-item__text">',
            `${unicode2Emoji(window.siyuan.storage[Constants.LOCAL_IMAGES].file, "b3-list-item__graphic", true)}<span class="b3-list-item__text">`);
        openModel({
            title: window.siyuan.languages.recentDocs,
            icon: "iconList",
            html: `<ul class="b3-list b3-list--mobile">${html}</ul>`,
            bindEvent(element: HTMLElement) {
                element.firstElementChild.addEventListener("click", (event) => {
                    const liElement = hasClosestByClassName(event.target as HTMLElement, "b3-list-item");
                    if (liElement) {
                        const item: RecentDocumentItem = liElement.dataset.markdownPath ? {
                            kind: "markdown",
                            notebook: liElement.dataset.markdownNotebook,
                            path: liElement.dataset.markdownPath,
                            title: liElement.querySelector(".b3-list-item__text").textContent,
                        } : {
                            kind: "native",
                            rootID: liElement.dataset.nodeId,
                            title: liElement.querySelector(".b3-list-item__text").textContent,
                        };
                        void openRecentDocument(app, item, {
                            openMarkdown: openMobileMarkdownFile,
                            openNative: async (currentApp, rootID) => {
                                openMobileFileById(currentApp, rootID, [Constants.CB_GET_SCROLL]);
                            },
                        });
                    }
                });
            }
        });
    });
};
