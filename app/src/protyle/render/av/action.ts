import {hasClosestBlock, hasClosestByClassName} from "../../util/hasClosest";
import {addDragFill, cellValueIsEmpty, getCellText, renderCell, renderCellAttr, updateHeaderCell} from "./cell";
import {avRender} from "./render";
import {Constants} from "../../../constants";
import {writeText} from "../../util/compatibility";
import {previewAttrViewImages} from "../../preview/image";
import {removeCompressURL} from "../../../util/image";
import {openDatabaseRowByData} from "./openDatabaseRow";

const isDetachedDatabaseCell = (cellElement: HTMLElement) => {
    return cellElement.dataset.detached === "true" || !cellElement.querySelector(".av__celltext--ref");
};

const openDatabaseRow = (protyle: IProtyle, target: HTMLElement, blockElement: HTMLElement) => {
    const cellElement = hasClosestByClassName(target, "av__cell") as HTMLElement;
    const rowElement = hasClosestByClassName(target, "av__row") || hasClosestByClassName(target, "av__gallery-item");
    if (!cellElement || !rowElement) {
        return;
    }
    openDatabaseRowByData(protyle, {
        avID: blockElement.dataset.avId,
        databaseBlockID: blockElement.dataset.nodeId,
        notebookID: protyle.notebookId,
        itemID: rowElement.getAttribute("data-id"),
        valueID: cellElement.dataset.id,
        title: cellElement.querySelector(".av__celltext")?.textContent.trim(),
        boundBlockID: cellElement.querySelector<HTMLElement>(".av__celltext--ref")?.dataset.id,
        isDetached: isDetachedDatabaseCell(cellElement),
    });
};

export const avClick = (protyle: IProtyle, event: MouseEvent & {target: HTMLElement}) => {
    const blockElement = hasClosestBlock(event.target) as HTMLElement;
    if (blockElement?.dataset.type !== "NodeAttributeView") return false;
    let target = event.target;
    while (target && target !== blockElement) {
        const type = target.dataset.type;
        if (type === "av-row-open") {
            openDatabaseRow(protyle, target, blockElement);
        } else if (type === "av-group-fold") {
            const open = target.firstElementChild.classList.toggle("av__group-arrow--open");
            target.parentElement.nextElementSibling.classList.toggle("fn__none", !open);
        } else if (type === "av-load-more") {
            const body = target.closest<HTMLElement>(".av__body");
            const size = parseInt(body.querySelector('[data-type="set-page-size"]')?.getAttribute("data-size") || "50");
            body.dataset.pageSize = (parseInt(body.dataset.pageSize || "50") + size).toString();
            blockElement.removeAttribute("data-render");
            void avRender(blockElement, protyle);
        } else if (target.classList.contains("item") && target.parentElement.classList.contains("layout-tab-bar")) {
            if (!target.classList.contains("item--focus")) {
                blockElement.setAttribute(Constants.CUSTOM_SY_AV_VIEW, target.dataset.id);
                blockElement.removeAttribute("data-render");
                void avRender(blockElement, protyle);
            }
        } else if (target.classList.contains("av__cellassetimg")) {
            previewAttrViewImages(removeCompressURL(target.getAttribute("src")), blockElement.dataset.avId,
                blockElement.getAttribute(Constants.CUSTOM_SY_AV_VIEW),
                blockElement.querySelector('[data-type="av-search"]')?.textContent.trim() || "");
        } else if (type === "copy") {
            writeText(getCellText(hasClosestByClassName(target, "av__cell")));
        } else if (type === "av-search-icon") {
            const search = blockElement.querySelector<HTMLElement>('[data-type="av-search"]');
            search.style.width = "128px";
            search.style.paddingLeft = "";
            search.style.marginRight = "1em";
            search.closest(".av__views")?.classList.add("av__views--show");
            search.focus();
        } else {
            target = target.parentElement;
            continue;
        }
        event.preventDefault();
        event.stopPropagation();
        return true;
    }
    return false;
};

export const avContextmenu: (protyle: IProtyle, rowElement: HTMLElement, position: IPosition) => boolean = () => false;

export const updateAttrViewCellAnimation = (cellElement: HTMLElement, value: IAVCellValue, headerValue?: {
    icon?: string,
    name?: string,
    pin?: boolean,
    type?: TAVCol
}) => {
    // 属性面板更新列名
    if (!cellElement) {
        return;
    }
    if (headerValue) {
        updateHeaderCell(cellElement, headerValue);
    } else {
        const hasDragFill = cellElement.querySelector(".av__drag-fill");
        const blockElement = hasClosestBlock(cellElement);
        if (!blockElement) {
            return;
        }
        const viewType = blockElement.getAttribute("data-av-type") as TAVView;
        const iconElement = cellElement.querySelector(".b3-menu__avemoji");
        if (["gallery", "kanban"].includes(viewType)) {
            if (value.type === "checkbox") {
                value.checkbox = {
                    checked: value.checkbox?.checked || false,
                    content: cellElement.getAttribute("aria-label").split('<div class="ft__on-surface">')[0],
                };
            }
            cellElement.innerHTML = renderCell(value, 0, iconElement ? !iconElement.classList.contains("fn__none") : false, viewType);
            cellElement.parentElement.setAttribute("data-empty", cellValueIsEmpty(value).toString());
        } else {
            cellElement.innerHTML = renderCell(value, 0, iconElement ? !iconElement.classList.contains("fn__none") : false);
        }
        if (hasDragFill) {
            addDragFill(cellElement);
        }
        renderCellAttr(cellElement, value);
    }
};

export const removeAttrViewColAnimation = (blockElement: Element, id: string) => {
    blockElement.querySelectorAll(`.av__cell[data-col-id="${id}"]`).forEach(item => {
        item.remove();
    });
};
