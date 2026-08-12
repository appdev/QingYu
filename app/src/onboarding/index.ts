import {Constants} from "../constants";
import {fetchPost, fetchSyncPost} from "../util/fetch";
import {openDataMigration} from "../menus/dataMigration";
import {mountHelp} from "../util/mount";
import {syncGuide} from "../sync/syncGuide";
import {setNoteBook} from "../util/pathName";
import type {App} from "../index";
/// #if MOBILE
import {openMobileFileById} from "../mobile/editor";
/// #else
import {openFileById} from "../editor/util";
/// #endif

export const ensureOnboarding = async () => {
    const onboarding = window.siyuan.config.onboarding;
    if (!onboarding?.newUser || onboarding.dismissed || window.siyuan.config.readonly || window.siyuan.isPublish) {
        return;
    }
    try {
        const response = await fetchSyncPost("/api/system/ensureOnboarding", {});
        if (response.code === 0) {
            window.siyuan.config.onboarding = response.data;
        }
    } catch (error) {
        console.warn("ensure onboarding failed", error);
    }
};

const shouldShowOnboarding = () => {
    return window.siyuan.config.onboarding?.newUser &&
        window.siyuan.config.onboarding.state === "completed" &&
        window.siyuan.config.onboarding.documentID &&
        !window.siyuan.config.onboarding.dismissed;
};

let pendingSyncHandler: (() => void) | undefined;
let mobileKeyboardHandler: EventListener | undefined;

const dismissOnboarding = () => {
    if (pendingSyncHandler) {
        window.removeEventListener("siyuan-sync-success", pendingSyncHandler);
        pendingSyncHandler = undefined;
    }
    if (mobileKeyboardHandler) {
        window.removeEventListener("siyuan-mobile-keyboard-change", mobileKeyboardHandler);
        mobileKeyboardHandler = undefined;
    }
    const onboardingElement = document.querySelector(".onboarding");
    onboardingElement?.parentElement?.classList.remove("onboarding-container");
    onboardingElement?.remove();
    window.siyuan.config.onboarding.dismissed = true;
    fetchPost("/api/system/dismissOnboarding", {});
};

const syncAndDismissOnSuccess = (app: App) => {
    if (pendingSyncHandler) {
        window.removeEventListener("siyuan-sync-success", pendingSyncHandler);
    }
    pendingSyncHandler = () => {
        pendingSyncHandler = undefined;
        dismissOnboarding();
    };
    window.addEventListener("siyuan-sync-success", pendingSyncHandler, {once: true});
    syncGuide(app);
};

const renderOnboarding = (app: App) => {
    if (!shouldShowOnboarding() || document.querySelector(".onboarding")) {
        return;
    }
    const element = document.createElement("section");
    element.className = "onboarding";
    element.innerHTML = `<button class="onboarding__close" data-type="close" aria-label="${window.siyuan.languages.close}">
    <svg><use xlink:href="#iconCloseRound"></use></svg>
</button>
<div class="onboarding__title">&#x1F389; ${window.siyuan.languages.onboardingWelcome}</div>
<div class="onboarding__desc">${window.siyuan.languages.onboardingDescription}</div>
<button class="b3-button b3-button--outline fn__block" data-type="import">
    <svg><use xlink:href="#iconDownload"></use></svg>${window.siyuan.languages.importExistingData}
</button>
<button class="b3-button b3-button--outline fn__block" data-type="sync">
    <svg><use xlink:href="#iconCloud"></use></svg>${window.siyuan.languages.sync}
</button>
<button class="b3-button b3-button--outline fn__block" data-type="guide">
    <svg><use xlink:href="#iconHelp"></use></svg>${window.siyuan.languages.userGuide}
</button>`;
    element.addEventListener("click", (event) => {
        const target = (event.target as HTMLElement).closest("[data-type]") as HTMLElement;
        if (!target) {
            return;
        }
        switch (target.dataset.type) {
            case "close":
                dismissOnboarding();
                break;
            case "import":
                openDataMigration({
                    mode: "onboarding",
                    notebookID: window.siyuan.config.onboarding.notebookID,
                    onContentImportComplete: dismissOnboarding,
                });
                break;
            case "sync":
                syncAndDismissOnSuccess(app);
                break;
            case "guide":
                mountHelp();
                dismissOnboarding();
                break;
        }
    });
    let containerElement = document.body;
    /// #if !MOBILE
    const editorContainerElement = document.querySelector(".layout__center") as HTMLElement;
    if (editorContainerElement) {
        containerElement = editorContainerElement;
        containerElement.classList.add("onboarding-container");
        element.classList.add("onboarding--editor");
    }
    /// #endif
    containerElement.append(element);
    /// #if MOBILE
    mobileKeyboardHandler = (event: Event) => {
        element.classList.toggle("onboarding--keyboard", (event as CustomEvent<boolean>).detail);
    };
    window.addEventListener("siyuan-mobile-keyboard-change", mobileKeyboardHandler);
    /// #endif
};

/// #if !MOBILE
export const openDesktopOnboarding = (app: App) => {
    if (!shouldShowOnboarding()) {
        return;
    }
    window.setTimeout(() => {
        openFileById({
            app,
            id: window.siyuan.config.onboarding.documentID,
            action: [Constants.CB_GET_FOCUSFIRST],
        });
        renderOnboarding(app);
    });
};
/// #endif

/// #if MOBILE
export const openMobileOnboarding = (app: App) => {
    if (!shouldShowOnboarding()) {
        return false;
    }
    renderOnboarding(app);
    openMobileFileById(app, window.siyuan.config.onboarding.documentID, [Constants.CB_GET_CONTEXT]);
    return true;
};
/// #endif

export const activateOnboarding = async (app: App, onboarding: Config.IConf["onboarding"]) => {
    window.siyuan.config.onboarding = onboarding;
    await ensureOnboarding();
    setNoteBook(() => {
        /// #if MOBILE
        openMobileOnboarding(app);
        /// #else
        openDesktopOnboarding(app);
        /// #endif
    });
};
