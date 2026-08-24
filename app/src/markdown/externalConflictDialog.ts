export type ExternalMarkdownConflictChoice = "reload" | "overwrite" | "cancel";

interface ConflictCallbacks {
    reload(): Promise<void> | void;
    overwrite(revision: string): Promise<void> | void;
    cancel(): Promise<void> | void;
}

export const handleExternalMarkdownConflictChoice = async (
    choice: ExternalMarkdownConflictChoice,
    revision: string,
    callbacks: ConflictCallbacks,
) => {
    if (choice === "reload") return callbacks.reload();
    if (choice === "overwrite") return callbacks.overwrite(revision);
    return callbacks.cancel();
};

export const openExternalMarkdownConflictDialog = async (
    revision: string,
    callbacks: ConflictCallbacks,
) => {
    const {Dialog} = await import("../dialog");
    let settled = false;
    const dialog = new Dialog({
        title: window.siyuan.languages.conflict,
        content: `<div class="b3-dialog__content">${window.siyuan.languages.externalMarkdownConflictTip}</div>
<div class="b3-dialog__action">
    <button class="b3-button b3-button--cancel" data-choice="cancel">${window.siyuan.languages.cancel}</button>
    <div class="fn__space"></div>
    <button class="b3-button b3-button--cancel" data-choice="reload">${window.siyuan.languages.refresh}</button>
    <button class="b3-button b3-button--text" data-choice="overwrite">${window.siyuan.languages.replace}</button>
</div>`,
        width: "560px",
        destroyCallback: () => {
            if (!settled) void handleExternalMarkdownConflictChoice("cancel", revision, callbacks);
        },
    });
    const choose = (choice: ExternalMarkdownConflictChoice) => {
        if (settled) return;
        settled = true;
        dialog.destroy();
        void handleExternalMarkdownConflictChoice(choice, revision, callbacks);
    };
    dialog.element.querySelectorAll<HTMLButtonElement>("[data-choice]").forEach((button) => {
        button.addEventListener("click", () => choose(button.dataset.choice as ExternalMarkdownConflictChoice));
    });
    dialog.element.addEventListener("keydown", (event) => {
        if (event.key === "Escape") choose("cancel");
        if (event.key === "Enter") event.preventDefault();
    });
    return dialog;
};
