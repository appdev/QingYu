import type {MarkdownScrollAnchor} from "./documentScroll";

export const DEFAULT_MARKDOWN_TYPEWRITER_MODE = false;

export interface MarkdownEditorSessionState {
    mode: "source" | "visual";
    selection: {anchor: number; head: number};
    scroll: MarkdownScrollAnchor | null;
    typewriterMode: boolean;
    typewriterModeConfigured?: boolean;
}

const clampPosition = (value: unknown, documentLength: number) => {
    const number = Number(value);
    return Number.isFinite(number) ? Math.max(0, Math.min(documentLength, Math.trunc(number))) : 0;
};

export const normalizeMarkdownEditorSessionState = (
    value: unknown,
    documentLength: number,
): MarkdownEditorSessionState => {
    const source = value && typeof value === "object" ? value as Record<string, unknown> : {};
    const rawSelection = source.selection && typeof source.selection === "object"
        ? source.selection as Record<string, unknown>
        : source;
    const rawScroll = source.scroll && typeof source.scroll === "object"
        ? source.scroll as Record<string, unknown>
        : null;
    const viewportOffset = Number(rawScroll?.viewportOffset);
    const typewriterModeConfigured = source.typewriterModeConfigured === true;
    return {
        mode: source.mode === "source" ? "source" : "visual",
        selection: {
            anchor: clampPosition(rawSelection.anchor, documentLength),
            head: clampPosition(rawSelection.head, documentLength),
        },
        scroll: rawScroll ? {
            position: clampPosition(rawScroll.position, documentLength),
            viewportOffset: Number.isFinite(viewportOffset) ? viewportOffset : 0,
        } : null,
        typewriterMode: typewriterModeConfigured
            ? Boolean(source.typewriterMode)
            : DEFAULT_MARKDOWN_TYPEWRITER_MODE,
        typewriterModeConfigured,
    };
};

export const serializeMarkdownEditorSessionState = (
    session: MarkdownEditorSessionState,
): MarkdownEditorSessionState => ({
    ...session,
    mode: "visual",
});

export const restoreMarkdownEditorSession = (
    session: MarkdownEditorSessionState,
    actions: {
        configure(): void;
        cue(position: number): void;
        restoreScroll(anchor: MarkdownScrollAnchor): void;
    },
) => {
    actions.configure();
    if (!session.scroll) return;
    actions.restoreScroll(session.scroll);
    if (session.scroll.position > 0) actions.cue(session.scroll.position);
};
