const previewThemeProperties = [
    "--b3-theme-background",
    "--b3-theme-on-background",
    "--b3-theme-surface",
    "--b3-theme-on-surface",
    "--b3-theme-primary",
    "--b3-font-family",
];

export const documentCardPreviewThemeSignature = () => {
    const root = document.documentElement;
    const appearance = window.siyuan.config.appearance;
    const styles = window.getComputedStyle(root);
    const attributes = Array.from(root.attributes)
        .filter((attribute) => attribute.name === "class" || attribute.name.includes("theme"))
        .map((attribute) => [attribute.name, attribute.value])
        .sort(([left], [right]) => left.localeCompare(right));
    return JSON.stringify({
        mode: appearance.mode,
        theme: appearance.mode === 1 ? appearance.themeDark : appearance.themeLight,
        themeVer: appearance.themeVer,
        themeStyle: document.getElementById("themeStyle")?.getAttribute("href") || "",
        defaultThemeStyle: document.getElementById("themeDefaultStyle")?.getAttribute("href") || "",
        attributes,
        properties: previewThemeProperties.map((property) => [property, styles.getPropertyValue(property).trim()]),
    });
};

export const documentCardPreviewAppearanceKey = async () => {
    const signature = new TextEncoder().encode(documentCardPreviewThemeSignature());
    const digest = await crypto.subtle.digest("SHA-256", signature);
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
};
