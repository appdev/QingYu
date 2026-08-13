const fs = require("node:fs");
const path = require("node:path");

const lutePath = path.join(__dirname, "../stage/protyle/js/lute/lute.min.js");
let cachedLute;

const getLute = () => {
    if (cachedLute?.New) {
        return cachedLute;
    }
    if (!global.Lute && !global.window?.Lute) {
        require(lutePath);
    }
    const Lute = global.Lute || global.window?.Lute;
    if (!Lute?.New) {
        throw new Error("The bundled Lute runtime is unavailable");
    }
    cachedLute = Lute;
    return cachedLute;
};

const markdownToBlockDOM = (markdown) => {
    const lute = getLute().New();
    lute.SetProtyleWYSIWYG(true);
    return lute.Md2BlockDOM(markdown);
};

const installLute = async (webContents) => {
    const source = fs.readFileSync(lutePath, "utf8");
    await webContents.executeJavaScript(`${source}\nvoid 0;`);
};

module.exports = {installLute, markdownToBlockDOM};
