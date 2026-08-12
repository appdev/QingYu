import type {SettingTabBuilder} from "../setting/builder";
import {Constants} from "../../constants";
import {fetchPost} from "../../util/fetch";
import {sendAppSetting} from "./appRuntime";

const registerAboutVersionGroup = (tab: SettingTabBuilder) => {
    const group = tab.group("version", "");

    group.slot({
        key: "version",
        keywords: [
            window.siyuan.languages.currentVer,
            window.siyuan.languages.downloadLatestVer,
            window.siyuan.languages.isMsStoreVerTip,
            window.siyuan.languages.checkUpdate,
        ],
        html: genAboutVersionHtml,
        afterMount: mountAboutVersionSlot,
    });
    /// #if !BROWSER
    if (!window.siyuan.config.system.isMicrosoftStore && window.siyuan.config.system.container === "std" && window.siyuan.config.system.os !== "linux") {
        group.switch("system.downloadInstallPkg", {
            title: window.siyuan.languages.autoDownloadUpdatePkg,
            desc: window.siyuan.languages.autoDownloadUpdatePkgTip,
            save: (value) => sendAppSetting("system.downloadInstallPkg", value),
        });
    }
    /// #endif
};

const genAboutVersionHtml = (): string => {
    if (window.siyuan.config.system.isMicrosoftStore) {
        return `<div class="fn__flex b3-label config-item config-wrap">
    <div class="fn__flex-1">
        <div class="config-name">${window.siyuan.languages.currentVer} v${Constants.SIYUAN_VERSION}<span id="isInsider"></span></div>
        <div class="b3-label__text">${window.siyuan.languages.isMsStoreVerTip}</div>
    </div>
</div>`;
    }
    return `<div class="fn__flex b3-label config-item config-wrap">
    <div class="fn__flex-1">
        <div class="config-name">${window.siyuan.languages.currentVer} v${Constants.SIYUAN_VERSION}<span id="isInsider"></span></div>
        <div class="b3-label__text">${window.siyuan.languages.downloadLatestVer}</div>
    </div>
    <div class="fn__space"></div>
    <div class="fn__flex-center fn__size200">
        <button id="checkUpdateBtn" class="b3-button b3-button--outline fn__block">
            <svg><use xlink:href="#iconRefresh"></use></svg>${window.siyuan.languages.checkUpdate}
        </button>
    </div>
</div>`;
};

const mountAboutVersionSlot = (root: HTMLElement) => {
    const isInsiderElement = root.querySelector("#isInsider");
    if (window.siyuan.config.system.isInsider && isInsiderElement) {
        isInsiderElement.innerHTML = " <span class='ft__secondary'>Insider Preview</span>";
    }
    const updateElement = root.querySelector("#checkUpdateBtn") as HTMLButtonElement | null;
    updateElement?.addEventListener("click", () => {
        const svgElement = updateElement.querySelector("svg");
        if (!svgElement || svgElement.classList.contains("fn__rotate")) {
            return;
        }
        svgElement.classList.add("fn__rotate");
        fetchPost("/api/system/checkUpdate", {showMsg: true}, () => {
            svgElement.classList.remove("fn__rotate");
        });
    });
};

const registerAboutInfoGroup = (tab: SettingTabBuilder) => {
    const group = tab.group("info", "");

    group.slot({
        key: "aboutLogo",
        keywords: [
            window.siyuan.languages.siyuanNote,
            window.siyuan.languages.slogan,
        ],
        html: () => `<div class="fn__flex b3-label config-item config-wrap">
    <div class="fn__flex-1">
        <div class="config-about__logo">
            <img src="/stage/icon.png">
            <span class="fn__space"></span>
            <span>${window.siyuan.languages.siyuanNote}</span>
            <span class="fn__space"></span>
            <span class="config-about__separator">·</span>
            <span class="fn__space"></span>
            <span class="ft__on-surface">${window.siyuan.languages.slogan}</span>
        </div>
    </div>
</div>`,
    });
};

export const registerAboutTab = (tab: SettingTabBuilder) => {
    registerAboutVersionGroup(tab);
    registerAboutInfoGroup(tab);
};
