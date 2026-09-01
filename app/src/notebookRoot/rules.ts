import type {NotebookRootView} from "./types";

export const NOTEBOOK_ROOT_DOCUMENT_MIME = "application/qingyu-notebook-root-document";

export const notebookRootCardRatio = (view: NotebookRootView, ratio: number) => view === "large" ? 1.25 : ratio;

export const notebookRootCardLayout = (view: NotebookRootView) => view === "list" ? "list" : "paper";

export const notebookRootPreviewCanvasOptions = (backgroundColor?: string) => ({
    width: 640,
    height: 960,
    canvasWidth: 640,
    canvasHeight: 960,
    pixelRatio: 1,
    ...(backgroundColor ? {backgroundColor} : {}),
} as const);

export const notebookRootPreviewCaptureStyle = (backgroundColor: string, foregroundColor: string) =>
    "position:relative;width:640px;height:960px;overflow:hidden;pointer-events:none;" +
    `box-sizing:border-box;padding:32px;background:${backgroundColor};color:${foregroundColor};`;

export const notebookRootPreviewCaptureRootStyle = () =>
    "position:fixed;left:-10000px;top:0;width:640px;height:960px;overflow:hidden;pointer-events:none;";

export const notebookRootNeedsMarkdownIdentity = (kind: string, identityState: string | undefined, identityConflict: boolean) =>
    kind === "markdown" && (identityState !== "valid" || identityConflict);

export const formatNotebookRootUpdated = (updatedSeconds: number, nowMilliseconds: number, locale: string) => {
    if (!Number.isFinite(updatedSeconds) || updatedSeconds <= 0) {
        return "";
    }
    const elapsedSeconds = updatedSeconds - Math.floor(nowMilliseconds / 1000);
    const ranges: Array<{limit: number, divisor: number, unit: Intl.RelativeTimeFormatUnit}> = [
        {limit: 60, divisor: 1, unit: "second"},
        {limit: 60 * 60, divisor: 60, unit: "minute"},
        {limit: 24 * 60 * 60, divisor: 60 * 60, unit: "hour"},
        {limit: 30 * 24 * 60 * 60, divisor: 24 * 60 * 60, unit: "day"},
        {limit: 365 * 24 * 60 * 60, divisor: 30 * 24 * 60 * 60, unit: "month"},
        {limit: Number.POSITIVE_INFINITY, divisor: 365 * 24 * 60 * 60, unit: "year"},
    ];
    const range = ranges.find((item) => Math.abs(elapsedSeconds) < item.limit) ?? ranges[ranges.length - 1];
    return new Intl.RelativeTimeFormat(locale, {numeric: "auto"})
        .format(Math.round(elapsedSeconds / range.divisor), range.unit);
};

export const classifyNotebookRootDrop = (
    source: {notebook: string},
    target: {notebook: string, root: boolean},
    effectiveSortMode: number,
) => {
    if (!target.root) return "move" as const;
    if (source.notebook !== target.notebook) return "move" as const;
    return effectiveSortMode === 6 ? "reorder" as const : "spring-back" as const;
};

export const isNotebookRootMoveTarget = (sourceNotebook: string, targetNotebook: string) =>
    Boolean(sourceNotebook && targetNotebook && sourceNotebook !== targetNotebook);
