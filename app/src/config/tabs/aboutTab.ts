import type {SettingTabBuilder} from "../setting/builder";
import {Constants} from "../../constants";
import {getQingYuLegalURL, QINGYU_SOURCE_URL, QINGYU_WEBSITE_URL} from "../../util/qingyuBrand";

const getLegalLabels = () => {
    if (window.siyuan.config.lang === "zh-CN") {
        return {privacy: "隐私政策", terms: "用户协议", source: "源代码", website: "官方网站"};
    }
    if (window.siyuan.config.lang === "zh-TW") {
        return {privacy: "隱私政策", terms: "使用者協議", source: "原始碼", website: "官方網站"};
    }
    if (window.siyuan.config.lang === "ja") {
        return {privacy: "プライバシーポリシー", terms: "利用規約", source: "ソースコード", website: "公式サイト"};
    }
    return {privacy: "Privacy Policy", terms: "User Agreement", source: "Source code", website: "Website"};
};

const registerAboutVersionGroup = (tab: SettingTabBuilder) => {
    const group = tab.group("version", "");

    group.slot({
        key: "version",
        keywords: [
            window.siyuan.languages.currentVer,
            window.siyuan.languages.isMsStoreVerTip,
        ],
        html: genAboutVersionHtml,
        afterMount: mountAboutVersionSlot,
    });
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
    </div>
</div>`;
};

const mountAboutVersionSlot = (root: HTMLElement) => {
    const isInsiderElement = root.querySelector("#isInsider");
    if (window.siyuan.config.system.isInsider && isInsiderElement) {
        isInsiderElement.innerHTML = " <span class='ft__secondary'>Insider Preview</span>";
    }
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
    group.slot({
        key: "legal",
        keywords: Object.values(getLegalLabels()),
        html: () => {
            const labels = getLegalLabels();
            return `<div class="fn__flex b3-label config-item config-wrap">
    <div class="fn__flex-1">
        <a href="${getQingYuLegalURL("privacy", window.siyuan.config.lang)}" target="_blank">${labels.privacy}</a>
        <span class="fn__space"></span>
        <a href="${getQingYuLegalURL("terms", window.siyuan.config.lang)}" target="_blank">${labels.terms}</a>
        <span class="fn__space"></span>
        <a href="${QINGYU_SOURCE_URL}" target="_blank">${labels.source}</a>
        <span class="fn__space"></span>
        <a href="${QINGYU_WEBSITE_URL}" target="_blank">${labels.website}</a>
    </div>
</div>`;
        },
    });
};

export const registerAboutTab = (tab: SettingTabBuilder) => {
    registerAboutVersionGroup(tab);
    registerAboutInfoGroup(tab);
};
