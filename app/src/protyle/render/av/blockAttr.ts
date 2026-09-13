import {bindDatabaseReadOnly} from "./readOnly";
import {fetchPost} from "../../../util/fetch";
import {getColIconByType} from "./col";
import {escapeAriaLabel, escapeAttr, escapeHtml} from "../../../util/escape";
import * as dayjs from "dayjs";
import {hasClosestByAttribute, hasClosestByClassName} from "../../util/hasClosest";
import {unicode2Emoji} from "../../../emoji";
import {Constants} from "../../../constants";
import {getCompressURL} from "../../../util/image";
import {openDatabaseRowByData} from "./openDatabaseRow";

export const getAVTemplateHTML = (content: string) => {
    if (window.siyuan.config.editor.allowHTMLBLockScript) {
        return content;
    }
    // 默认过滤危险标签和事件属性，避免数据库模板字段中的代码直接执行
    return window.DOMPurify.sanitize(content);
};

const genAVRollupHTML = (value: IAVCellValue) => {
    let html = "";
    const dataValue: IAVCellDateValue = value[value.type as "date"];
    switch (value.type) {
        case "block":
            if (value?.isDetached) {
                html = `<span>${escapeHtml(value.block?.content || window.siyuan.languages.untitled)}</span>`;
            } else {
                html = `<span data-type="block-ref" data-id="${value.block.id}" data-subtype="s" class="av__celltext--ref">${escapeHtml(value.block?.content || window.siyuan.languages.untitled)}</span>`;
            }
            break;
        case "text":
            html = escapeHtml(value.text.content);
            break;
        case "number":
            html = value.number.formattedContent || value.number.content.toString();
            break;
        case "date":
        case "updated":
        case "created":
            if (dataValue.formattedContent) {
                html = dataValue.formattedContent;
            } else {
                if (dataValue && dataValue.isNotEmpty) {
                    html = dayjs(dataValue.content).format(dataValue.isNotTime ? "YYYY-MM-DD" : "YYYY-MM-DD HH:mm");
                }
                if (dataValue && dataValue.hasEndDate && dataValue.isNotEmpty && dataValue.isNotEmpty2) {
                    html = `<svg class="av__cellicon"><use xlink:href="#iconForward"></use></svg>${dayjs(dataValue.content2).format(dataValue.isNotTime ? "YYYY-MM-DD" : "YYYY-MM-DD HH:mm")}`;
                }
            }
            if (html) {
                html = `<span class="av__celltext">${html}</span>`;
            }
            break;
        case "url":
            html = value.url.content ? `<a class="fn__a" href="${escapeAttr(value.url.content)}" target="_blank">${escapeHtml(value.url.content)}</a>` : "";
            break;
        case "phone":
            html = value.phone.content ? `<a class="fn__a" href="tel:${escapeAttr(value.phone.content)}" target="_blank">${escapeHtml(value.phone.content)}</a>` : "";
            break;
        case "email":
            html = value.email.content ? `<a class="fn__a" href="mailto:${escapeAttr(value.email.content)}" target="_blank">${escapeHtml(value.email.content)}</a>` : "";
            break;
    }
    return html;
};

export const genAVValueHTML = (value: IAVCellValue) => {
    let html = "";
    switch (value.type) {
        case "block":
            html = `<input data-id="${value.block.id}" value="${escapeAttr(value.block.content)}" type="text" class="b3-text-field b3-text-field--text fn__flex-1" placeholder="${window.siyuan.languages.empty}">`;
            break;
        case "text":
            html = `<textarea style="resize: vertical" rows="${(value.text?.content || "").split("\n").length}" class="b3-text-field b3-text-field--text fn__flex-1" placeholder="${window.siyuan.languages.empty}">${escapeHtml(value.text?.content || "")}</textarea>`;
            break;
        case "number":
            html = `<input value="${value.number.isNotEmpty ? value.number.content : ""}" type="number" class="b3-text-field b3-text-field--text fn__flex-1" placeholder="${window.siyuan.languages.empty}">
<span class="fn__space"></span><span class="fn__flex-center ft__on-surface b3-tooltips__w b3-tooltips" aria-label="${window.siyuan.languages.format}">${value.number.formattedContent}</span><span class="fn__space"></span>`;
            break;
        case "mSelect":
        case "select":
            value.mSelect?.forEach((item, index) => {
                if (value.type === "select" && index > 0) {
                    return;
                }
                html += `<span class="b3-chip b3-chip--middle" style="background-color:var(--b3-font-background${item.color});color:var(--b3-font-color${item.color})">${escapeHtml(item.content)}</span>`;
            });
            break;
        case "mAsset":
            value.mAsset?.forEach(item => {
                if (item.type === "image") {
                    html += `<img loading="lazy" class="av__cellassetimg ariaLabel" aria-label="${escapeAriaLabel(item.content)}" src="${getCompressURL(item.content)}">`;
                } else {
                    html += `<span class="b3-chip b3-chip--middle av__celltext--url ariaLabel" aria-label="${escapeAriaLabel(item.content)}" data-name="${escapeAttr(item.name)}" data-url="${escapeAttr(item.content)}">${escapeHtml(item.name || item.content)}</span>`;
                }
            });
            break;
        case "date":
            html = `<span class="av__celltext" data-value='${JSON.stringify(value[value.type])}' placeholder="${window.siyuan.languages.empty}">`;
            if (value[value.type] && value[value.type].isNotEmpty) {
                html += dayjs(value[value.type].content).format(value[value.type].isNotTime ? "YYYY-MM-DD" : "YYYY-MM-DD HH:mm");
            }
            if (value[value.type] && value[value.type].hasEndDate && value[value.type].isNotEmpty && value[value.type].isNotEmpty2) {
                html += `<svg class="av__cellicon"><use xlink:href="#iconForward"></use></svg>${dayjs(value[value.type].content2).format(value[value.type].isNotTime ? "YYYY-MM-DD" : "YYYY-MM-DD HH:mm")}`;
            }
            html += "</span>";
            break;
        case "created":
        case "updated":
            if (value[value.type].isNotEmpty) {
                html = `<span data-content="${value[value.type].content}">${dayjs(value[value.type].content).format("YYYY-MM-DD HH:mm")}</span>`;
            }
            break;
        case "url":
            html = `<input value="${escapeAttr(value.url.content)}" class="b3-text-field b3-text-field--text fn__flex-1" placeholder="${window.siyuan.languages.empty}">
<span class="fn__space"></span>
<a ${value.url.content ? `href="${escapeAttr(value.url.content)}"` : ""} target="_blank" aria-label="${window.siyuan.languages.openBy}" class="block__icon block__icon--show fn__flex-center b3-tooltips__w b3-tooltips"><svg><use xlink:href="#iconLink"></use></svg></a>`;
            break;
        case "phone":
            html = `<input value="${escapeAttr(value.phone.content)}" class="b3-text-field b3-text-field--text fn__flex-1" placeholder="${window.siyuan.languages.empty}">
<span class="fn__space"></span>
<a ${value.phone.content ? `href="tel:${escapeAttr(value.phone.content)}"` : ""} target="_blank" aria-label="${window.siyuan.languages.openBy}" class="block__icon block__icon--show fn__flex-center b3-tooltips__w b3-tooltips"><svg><use xlink:href="#iconPhone"></use></svg></a>`;
            break;
        case "checkbox":
            html = `<svg class="av__checkbox"><use xlink:href="#icon${value.checkbox.checked ? "Check" : "Uncheck"}"></use></svg>`;
            break;
        case "template":
            html = `<div class="fn__flex-1" placeholder="${window.siyuan.languages.empty}">${getAVTemplateHTML(value.template.content)}</div>`;
            break;
        case "email":
            html = `<input value="${escapeAttr(value.email.content)}" class="b3-text-field b3-text-field--text fn__flex-1" placeholder="${window.siyuan.languages.empty}">
<span class="fn__space"></span>
<a ${value.email.content ? `href="mailto:${escapeAttr(value.email.content)}"` : ""} target="_blank" aria-label="${window.siyuan.languages.openBy}" class="block__icon block__icon--show fn__flex-center b3-tooltips__w b3-tooltips"><svg><use xlink:href="#iconEmail"></use></svg></a>`;
            break;
        case "relation":
            value?.relation?.contents?.forEach((item, index) => {
                if (item && item.block) {
                    const rowID = value.relation.blockIDs[index];
                    if (item?.isDetached) {
                        html += `<span data-row-id="${rowID}" class="av__cell--relation"><span><svg style="height: 26px"><use xlink:href="#iconLine"></use></svg><span class="fn__space--5"></span></span><span class="av__celltext">${Lute.EscapeHTMLStr(item.block.content || window.siyuan.languages.untitled)}</span></span>`;
                    } else {
                        // data-block-id 用于更新 emoji
                        html += `<span data-row-id="${rowID}" class="av__cell--relation" data-block-id="${item.block.id}"><span class="b3-menu__avemoji" data-unicode="${item.block.icon || ""}">${unicode2Emoji(item.block.icon || window.siyuan.storage[Constants.LOCAL_IMAGES].file)}</span><span data-type="block-ref" data-id="${item.block.id}" data-subtype="s" class="av__celltext av__celltext--ref">${Lute.EscapeHTMLStr(item.block.content || window.siyuan.languages.untitled)}</span></span>`;
                    }
                }
            });
            if (html && html.endsWith(", ")) {
                html = html.substring(0, html.length - 2);
            }
            break;
        case "rollup":
            value?.rollup?.contents?.forEach((item) => {
                const rollupText = ["template", "select", "mSelect", "mAsset", "checkbox", "relation"].includes(item.type) ? genAVValueHTML(item) : genAVRollupHTML(item);
                if (rollupText) {
                    html += rollupText.replace("fn__flex-1", "") + ",&nbsp;";
                }
            });
            if (html && html.endsWith(",&nbsp;")) {
                html = html.substring(0, html.length - 7);
            }
            break;
    }
    return html;
};

let attributeViewRenderID = 0;

export const renderAVAttribute = (element: HTMLElement, id: string, protyle: IProtyle, cb?: (element: HTMLElement) => void,
                                  row?: { avID: string, itemID: string, valueID: string }) => {
    const renderID = (++attributeViewRenderID).toString();
    element.dataset.avAttributeRenderId = renderID;
    fetchPost("/api/av/getAttributeViewKeys", row ? {id, avID: row.avID, itemID: row.itemID, valueID: row.valueID} : {id}, (response) => {
        if (element.dataset.avAttributeRenderId !== renderID) {
            return;
        }
        let html = "";
        const tables = Array.isArray(response.data) ? response.data : [];
        tables.forEach((table: {
            keyValues: {
                key: {
                    type: TAVCol,
                    name: string,
                    desc: string,
                    icon: string,
                    id: string,
                    options?: {
                        name: string,
                        color: string
                    }[]
                },
                values: {
                    keyID: string,
                    id: string,
                    blockID: string,
                    isDetached?: boolean,
                    type: TAVCol & IAVCellValue
                }  []
            }[],
            blockIDs: string[],
            avID: string
            avName: string
        }) => {
            let innerHTML = `<div class="custom-attr__avheader">
    <div class="block__logo block__logo--icon popover__block" style="max-width:calc(100% - 40px)" data-id='${JSON.stringify(table.blockIDs)}'>
        <svg class="block__logoicon"><use xlink:href="#iconDatabase"></use></svg>
        <span class="fn__ellipsis">${table.avName || window.siyuan.languages.database}</span>
    </div>
    <div class="fn__flex-1"></div>
    <span data-type="remove" data-row-id="${table.keyValues && table.keyValues[0].values[0].blockID}" class="block__icon block__icon--warning block__icon--show b3-tooltips__w b3-tooltips" aria-label="${window.siyuan.languages.removeAV}"><svg><use xlink:href="#iconTrashcan"></use></svg></span>
</div>`;
            table.keyValues?.forEach(item => {
                innerHTML += `<div class="block__icons av__row" data-id="${id}" data-col-id="${item.key.id}">
    <div class="block__icon" draggable="true"><svg><use xlink:href="#iconDrag"></use></svg></div>
    <div class="block__logo block__logo--icon ariaLabel fn__pointer" data-type="editCol" data-position="parentW" aria-label="${escapeAriaLabel(item.key.name)}<div class='ft__on-surface'>${escapeAriaLabel(item.key.desc)}</div>">
        ${item.key.icon ? unicode2Emoji(item.key.icon, "block__logoicon", true) : `<svg class="block__logoicon"><use xlink:href="#${getColIconByType(item.key.type)}"></use></svg>`}
        <span>${escapeHtml(item.key.name)}</span>
    </div>
    <div data-av-id="${table.avID}" data-col-id="${item.values[0].keyID}" data-row-id="${item.values[0].blockID}" data-id="${item.values[0].id}" data-type="${item.values[0].type}"${item.values[0].isDetached ? ' data-detached="true"' : ""}
data-options="${item.key?.options ? escapeAttr(JSON.stringify(item.key.options)) : "[]"}" 
${["text", "number", "date", "url", "phone", "template", "email"].includes(item.values[0].type) ? "" : `placeholder="${window.siyuan.languages.empty}"`}  
class="fn__flex-1 fn__flex${["url", "text", "number", "email", "phone", "block"].includes(item.values[0].type) ? "" : " custom-attr__avvalue"}${["created", "updated"].includes(item.values[0].type) ? " custom-attr__avvalue--readonly" : ""}">${genAVValueHTML(item.values[0])}</div>
</div>`;
            });
            innerHTML += `<div class="fn__hr"></div>
<button data-type="addColumn" class="b3-button b3-button--cancel"><svg><use xlink:href="#iconAdd"></use></svg>${window.siyuan.languages.newCol}</button>
<div class="fn__hr--b"></div><div class="fn__hr--b"></div>`;
            html += `<div data-av-id="${table.avID}" data-av-type="table" data-node-id="${id}" data-type="NodeAttributeView">${innerHTML}</div>`;

            if (element.innerHTML) {
                // 防止 blockElement 找不到
                const blockElement = element.querySelector(`[data-node-id="${id}"][data-av-id="${table.avID}"]`);
                if (blockElement) {
                    blockElement.innerHTML = innerHTML;
                } else {
                    element.insertAdjacentHTML("beforeend", `<div data-av-id="${table.avID}" data-av-type="table" data-node-id="${id}" data-type="NodeAttributeView">${innerHTML}</div>`);
                }
            }
        });
        if (!element.dataset.avReadEvents) {
            element.dataset.avReadEvents = "true";
            element.addEventListener("click", (event) => {
                const backlinkToggleElement = hasClosestByAttribute(event.target as HTMLElement, "data-type", "av-backlinks-toggle");
                if (backlinkToggleElement) {
                    const backlinksElement = hasClosestByClassName(backlinkToggleElement, "custom-attr__avbacklinks");
                    if (backlinksElement) {
                        const expanded = backlinksElement.dataset.expanded !== "true";
                        backlinksElement.dataset.expanded = expanded.toString();
                        backlinkToggleElement.setAttribute("aria-expanded", expanded.toString());
                        backlinkToggleElement.querySelector("use").setAttribute("xlink:href", expanded ? "#iconDown" : "#iconRight");
                        event.stopPropagation();
                        return;
                    }
                }
                const backlinkOpenElement = hasClosestByAttribute(event.target as HTMLElement, "data-type", "av-backlink-open");
                if (backlinkOpenElement) {
                    openDatabaseRowByData(protyle, {
                        avID: backlinkOpenElement.dataset.avId,
                        databaseBlockID: backlinkOpenElement.dataset.databaseBlockId,
                        notebookID: backlinkOpenElement.dataset.boxId,
                        itemID: backlinkOpenElement.dataset.itemId,
                        valueID: backlinkOpenElement.dataset.valueId,
                        title: backlinkOpenElement.dataset.title,
                        boundBlockID: backlinkOpenElement.dataset.boundBlockId,
                        isDetached: backlinkOpenElement.dataset.detached === "true",
                    });
                    event.stopPropagation();
                    return;
                }
            });
        }
        element.innerHTML = html;
        bindDatabaseReadOnly(element);
        renderAttributeViewBacklinks(element, id, renderID, row, cb);
    });
};

const renderAttributeViewBacklinks = (element: HTMLElement, id: string, renderID: string,
                                      row?: { avID: string, itemID: string, valueID: string },
                                      cb?: (element: HTMLElement) => void) => {
    const oldBacklinksElement = element.querySelector<HTMLElement>(".custom-attr__avbacklinks");
    const expanded = oldBacklinksElement?.dataset.expanded === "true";
    fetchPost("/api/av/getAttributeViewBacklinks", row ? {
        id,
        avID: row.avID,
        itemID: row.itemID,
        valueID: row.valueID,
    } : {id}, (response) => {
        if (element.dataset.avAttributeRenderId !== renderID) {
            return;
        }
        const currentBacklinksElement = element.querySelector<HTMLElement>(".custom-attr__avbacklinks");
        const currentExpanded = currentBacklinksElement ? currentBacklinksElement.dataset.expanded === "true" : expanded;
        currentBacklinksElement?.remove();
        const data = response.data as {
            total: number,
            items: {
                avID: string,
                avName: string,
                databaseBlockID: string,
                boxID: string,
                databasePath: string,
                itemID: string,
                valueID: string,
                title: string,
                icon: string,
                boundBlockID: string,
                isDetached: boolean,
            }[]
        };
        if (data?.total > 0) {
            const countLabel = window.siyuan.languages.avBacklinks.replace("${count}", data.total.toString());
            let itemsHTML = "";
            data.items.forEach((item) => {
                const title = item.title || window.siyuan.languages.untitled;
                const databasePath = item.databasePath ? `${item.databasePath} / ${item.avName}` : item.avName;
                itemsHTML += `<button type="button" class="custom-attr__avbacklink" data-type="av-backlink-open" data-av-id="${escapeAttr(item.avID)}" data-database-block-id="${escapeAttr(item.databaseBlockID)}" data-box-id="${escapeAttr(item.boxID)}" data-item-id="${escapeAttr(item.itemID)}" data-value-id="${escapeAttr(item.valueID)}" data-title="${escapeAttr(title)}" data-bound-block-id="${escapeAttr(item.boundBlockID)}" data-detached="${item.isDetached}">
    ${item.icon ? `<span class="custom-attr__avbacklinkicon">${unicode2Emoji(item.icon, "", true)}</span>` : ""}
    <span class="fn__flex-1 fn__ellipsis">
        <span class="custom-attr__avbacklinktitle fn__ellipsis">${escapeHtml(title)}</span>
        <span class="custom-attr__avbacklinkpath fn__ellipsis">${escapeHtml(databasePath || window.siyuan.languages.database)}</span>
    </span>
    <span class="custom-attr__avbacklinkopen b3-tooltips b3-tooltips__w" aria-label="${escapeAttr(window.siyuan.languages.openBy)}"><svg><use xlink:href="#iconOpen"></use></svg></span>
</button>`;
            });
            element.insertAdjacentHTML("afterbegin", `<div class="custom-attr__avbacklinks" data-expanded="${currentExpanded}">
    <button type="button" class="custom-attr__avbacklinks-toggle" data-type="av-backlinks-toggle" aria-expanded="${currentExpanded}">
        <svg><use xlink:href="${currentExpanded ? "#iconDown" : "#iconRight"}"></use></svg>
        <svg><use xlink:href="#iconLink"></use></svg>
        <span class="fn__flex-1">${escapeHtml(countLabel)}</span>
    </button>
    <div class="custom-attr__avbacklinks-body">
        ${itemsHTML}
    </div>
</div>`);
        }
        cb?.(element);
    });
};

export const isCustomAttr = (cellElement: Element) => {
    return !!cellElement.getAttribute("data-av-id");
};
