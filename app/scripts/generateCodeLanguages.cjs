const fs = require("node:fs");
const path = require("node:path");

const appDir = path.resolve(__dirname, "..");
const highlightPath = path.join(appDir, "stage/protyle/js/highlight.js/highlight.min.js");
const thirdLanguagesPath = path.join(appDir, "stage/protyle/js/highlight.js/third-languages.js");
const outputPath = path.join(appDir, "src/protyle/codeLanguages.generated.ts");
const highlight = require(highlightPath);
const thirdLanguageSource = fs.readFileSync(thirdLanguagesPath, "utf8");
const thirdLanguages = Array.from(
    thirdLanguageSource.matchAll(/registerLanguage\((["'])([^"']+)\1/g),
    (match) => match[2],
);
const languages = [...new Set([...highlight.listLanguages(), ...thirdLanguages])].sort();
const lines = [];

for (let index = 0; index < languages.length; index += 8) {
    lines.push(`    ${languages.slice(index, index + 8).map((language) => JSON.stringify(language)).join(", ")},`);
}

fs.writeFileSync(outputPath, [
    "// 由 scripts/generateCodeLanguages.cjs 根据内置 Highlight.js 资源生成，请勿手动编辑。",
    "export const BUNDLED_CODE_LANGUAGES = [",
    ...lines,
    "] as const;",
    "",
].join("\n"));
console.log(`Generated ${languages.length} code languages in ${path.relative(appDir, outputPath)}`);
