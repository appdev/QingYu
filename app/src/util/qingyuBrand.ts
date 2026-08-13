export const QINGYU_WEBSITE_URL = "https://apkdv.com/";
export const QINGYU_SOURCE_URL = "https://github.com/appdev/QingYu";
export const QINGYU_CONTACT_URL = "mailto:lengyue@apkdv.com";

export const getQingYuLegalURL = (kind: "privacy" | "terms", language: string) => {
    const locale = ["zh-CN", "zh-TW", "en", "ja"].includes(language) ? language : "en";
    return `/stage/legal/${kind}.${locale}.html`;
};
