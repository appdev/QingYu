import type {
    MarkdownControlHandle,
    MarkdownPopoverRequest,
} from "./markra-core/adapter";

export interface SiyuanMarkdownPopoverRequest extends MarkdownPopoverRequest {
    position(anchor: HTMLElement, popover: HTMLElement): void;
}

export const mountSiyuanMarkdownPopover = (
    request: SiyuanMarkdownPopoverRequest,
): MarkdownControlHandle => {
    let destroyed = false;
    const element = request.ownerDocument.createElement("div");
    element.className = request.kind === "search"
        ? "b3-menu markra-search-popover"
        : `protyle-util markra-${request.kind}-popover`;
    element.dataset.appearanceState = "ready";
    element.setAttribute("role", request.kind === "search" ? "dialog" : "tooltip");
    element.tabIndex = -1;
    element.append(request.content);
    (request.anchor.closest(".markdown-editor") ?? request.ownerDocument.body).append(element);
    request.position(request.anchor, element);

    const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === "Escape") destroy();
    };
    const onPointerDown = (event: Event) => {
        if (!(event.target instanceof Node)) return;
        if (!element.contains(event.target) && !request.anchor.contains(event.target)) destroy();
    };
    const destroy = () => {
        if (destroyed) return;
        destroyed = true;
        request.ownerDocument.removeEventListener("keydown", onKeyDown);
        request.ownerDocument.removeEventListener("pointerdown", onPointerDown, true);
        element.remove();
        if (request.restoreFocus && request.anchor.isConnected) request.anchor.focus();
    };
    request.ownerDocument.addEventListener("keydown", onKeyDown);
    request.ownerDocument.addEventListener("pointerdown", onPointerDown, true);

    return {
        destroy,
        element,
        focus() {
            (element.querySelector<HTMLElement>(
                "input, button, select, textarea, [tabindex]:not([tabindex='-1'])",
            ) ?? element).focus();
        },
    };
};
