const {createHash} = require("node:crypto");

const maximumCacheBytes = 8 * 1024 * 1024;
const cache = new Map();
let cacheBytes = 0;

// 仅处理普通轮廓字体；彩色、可变和未知格式保留完整文件。
function canSubset(buffer) {
    if (buffer.length < 256 * 1024) return false;
    const signature = buffer.toString("ascii", 0, 4);
    const woff = signature === "wOFF";
    if (!woff && buffer.readUInt32BE(0) !== 0x00010000) return false;
    const count = buffer.readUInt16BE(woff ? 12 : 4);
    const offset = woff ? 44 : 12;
    const stride = woff ? 20 : 16;
    if (count > 256 || offset + count * stride > buffer.length) return false;
    const tags = new Set();
    for (let index = 0; index < count; index++) {
        tags.add(buffer.toString("ascii", offset + index * stride, offset + index * stride + 4));
    }
    return tags.has("glyf") && !["COLR", "CPAL", "CBDT", "CBLC", "sbix", "SVG ", "fvar"].some((tag) => tags.has(tag));
}

function cacheSubset(key, value) {
    if (value.length > maximumCacheBytes) return;
    while (cache.size >= 128 || cacheBytes + value.length > maximumCacheBytes) {
        const oldest = cache.keys().next().value;
        cacheBytes -= cache.get(oldest).length;
        cache.delete(oldest);
    }
    cache.set(key, value);
    cacheBytes += value.length;
}

async function subsetCSS(css, text) {
    if (typeof css !== "string" || css.length > 64 * 1024 * 1024 || typeof text !== "string" || text.length > 16384) {
        throw new Error("Preview font input exceeds budget");
    }
    // 未覆盖的复杂文字保持原始字体，避免改变组合字形和连接行为。
    if (!/^[\p{Script=Latin}\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Hangul}\p{Script=Common}\s]*$/u.test(text)) {
        return css;
    }
    const matches = Array.from(css.matchAll(/data:[^\s"'()]*?;base64,([A-Za-z0-9+/=]+)/g));
    let result = "";
    let offset = 0;
    for (const match of matches) {
        let replacement = match[0];
        const original = Buffer.from(match[1], "base64");
        if (canSubset(original)) {
            const key = createHash("sha256").update(original).update("\0").update(text).digest("hex");
            let subset = cache.get(key);
            if (subset) {
                cache.delete(key);
                cache.set(key, subset);
            } else {
                try {
                    subset = await require("subset-font")(original, text);
                    if (subset.length < original.length) cacheSubset(key, subset);
                } catch {
                    // 单个字体失败不影响其他字体和截图。
                }
            }
            if (subset && subset.length < original.length) {
                replacement = match[0].slice(0, match[0].indexOf(",") + 1) + subset.toString("base64");
            }
        }
        result += css.slice(offset, match.index) + replacement;
        offset = match.index + match[0].length;
    }
    return result + css.slice(offset);
}

module.exports = {canSubset, subsetCSS};
