export interface SiyuanImageResizeOptions {
    centerResize: boolean;
    initialClientX: number;
    initialWidth: number;
    maxRight: number;
    minWidth: number;
    onPreview(width: number): void;
    onCommit(width: number): void;
    onCancel(): void;
}

type ResizePointerEvent = Pick<PointerEvent, "clientX">;

export interface SiyuanImageResizeEventTarget {
    addEventListener(type: "pointermove", listener: (event: ResizePointerEvent) => void): void;
    addEventListener(type: "pointerup" | "pointercancel", listener: () => void, options?: {once?: boolean}): void;
    removeEventListener(type: "pointermove", listener: (event: ResizePointerEvent) => void): void;
    removeEventListener(type: "pointerup" | "pointercancel", listener: () => void): void;
}

export const calculateSiyuanImageWidth = (
    initialWidth: number,
    deltaX: number,
    centerResize: boolean,
    maxWidth: number,
    minWidth = 17,
) => Math.min(maxWidth, Math.max(minWidth, Math.round(initialWidth + deltaX * (centerResize ? 2 : 1))));

export const startSiyuanImageResize = (
    options: SiyuanImageResizeOptions,
    eventTarget: SiyuanImageResizeEventTarget = window,
) => {
    let active = true;
    let width = options.initialWidth;
    const maxWidth = Math.max(options.minWidth, options.maxRight);
    const move = (event: ResizePointerEvent) => {
        if (!active) {
            return;
        }
        width = calculateSiyuanImageWidth(
            options.initialWidth,
            event.clientX - options.initialClientX,
            options.centerResize,
            maxWidth,
            options.minWidth,
        );
        options.onPreview(width);
    };
    const cleanup = () => {
        eventTarget.removeEventListener("pointermove", move);
        eventTarget.removeEventListener("pointerup", commit);
        eventTarget.removeEventListener("pointercancel", cancel);
    };
    const commit = () => {
        if (!active) {
            return;
        }
        active = false;
        cleanup();
        options.onCommit(width);
    };
    const cancel = () => {
        if (!active) {
            return;
        }
        active = false;
        cleanup();
        options.onCancel();
    };
    eventTarget.addEventListener("pointermove", move);
    eventTarget.addEventListener("pointerup", commit, {once: true});
    eventTarget.addEventListener("pointercancel", cancel, {once: true});
    return cancel;
};
