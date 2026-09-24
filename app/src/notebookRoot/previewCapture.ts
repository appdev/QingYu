export const documentCardPreviewFormat = (): "png" | "webp" => {
    try {
        return typeof window.require === "function" && window.require("electron").ipcRenderer ? "png" : "webp";
    } catch {
        return "webp";
    }
};

export const captureDocumentCardPreview = async (host: HTMLElement): Promise<Blob> => {
    const clone = host.cloneNode(true) as HTMLElement;
    // 将文档级图标定义带入截图，保留 use 的尺寸和继承样式。
    for (const use of Array.from(clone.querySelectorAll("use"))) {
        const href = use.getAttribute("href") || use.getAttribute("xlink:href");
        if (!href?.startsWith("#") || clone.querySelector(`#${CSS.escape(href.slice(1))}`)) continue;
        const definition = document.getElementById(href.slice(1));
        if (definition) {
            const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
            defs.append(definition.cloneNode(true));
            use.closest("svg").prepend(defs);
        }
    }
    const canvases = clone.querySelectorAll("canvas");
    host.querySelectorAll("canvas").forEach((canvas, index) => {
        const image = document.createElement("img");
        for (const attribute of Array.from(canvas.attributes)) image.setAttribute(attribute.name, attribute.value);
        image.src = canvas.toDataURL();
        image.style.width = getComputedStyle(canvas).width;
        image.style.height = getComputedStyle(canvas).height;
        canvases[index].replaceWith(image);
    });
    const attributes = (element: Element) => Array.from(element.attributes, attribute => [attribute.name, attribute.value]);
    const styles = Array.from(document.querySelectorAll<HTMLStyleElement | HTMLLinkElement>("style, link[rel=stylesheet]"))
        .filter(element => !(element as HTMLLinkElement).disabled)
        .map(element => element instanceof HTMLLinkElement ? {href: element.href, media: element.media} : {
            css: element.sheet ? Array.from(element.sheet.cssRules, rule => rule.cssText).join("\n") : element.textContent,
            media: element.media,
        });
    const {ipcRenderer} = window.require("electron") as typeof import("electron");
    const bytes = await ipcRenderer.invoke("siyuan-preview-capture", {
        html: clone.outerHTML, styles, root: attributes(document.documentElement), body: attributes(document.body),
        base: document.baseURI,
    });
    if (!(bytes instanceof Uint8Array) || bytes.length < 8 ||
        ![137, 80, 78, 71, 13, 10, 26, 10].every((value, index) => bytes[index] === value)) {
        throw new Error("Invalid card preview PNG");
    }
    return new Blob([bytes], {type: "image/png"});
};
