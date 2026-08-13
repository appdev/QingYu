#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");

const appRoot = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(appRoot, "..");

const GUIDES = Object.freeze({
    "zh-CN": {boxID: "20210808180117-czj9bvb", rootID: "20200812220555-lj3enxa", sort: 2},
    "zh-TW": {boxID: "20211226090932-5lcq56f", rootID: "20211226115423-d5z1joq", sort: 3},
    en: {boxID: "20210808180117-6v0mkxr", rootID: "20200923234011-ieuun1p", sort: 1},
    ja: {boxID: "20240530133126-axarxgx", rootID: "20240530101000-4qitucx", sort: 4},
});

const makeIDFactory = (locale) => {
    let index = 0;
    const localeCode = {"zh-CN": "qcn0000", "zh-TW": "qtw0000", en: "qen0000", ja: "qja0000"}[locale];
    return () => `20260813${String(++index).padStart(6, "0")}-${localeCode.slice(0, 4)}${index.toString(36).padStart(3, "0")}`;
};

const textChildren = (text) => {
    const children = [];
    const expression = /\[([^\]]+)]\(([^)]+)\)/g;
    let offset = 0;
    for (const match of text.matchAll(expression)) {
        if (match.index > offset) {
            children.push({Type: "NodeText", Data: text.slice(offset, match.index)});
        }
        children.push({
            Type: "NodeTextMark",
            TextMarkType: "a",
            TextMarkAHref: match[2],
            TextMarkTextContent: match[1],
        });
        offset = match.index + match[0].length;
    }
    if (offset < text.length) {
        children.push({Type: "NodeText", Data: text.slice(offset)});
    }
    return children.length > 0 ? children : [{Type: "NodeText", Data: text}];
};

const validateGuideConfig = (source, locale) => {
    if (!GUIDES[locale] || source.locale !== locale || typeof source.productName !== "string" ||
        typeof source.title !== "string" || !Array.isArray(source.sections) || source.sections.length !== 8) {
        throw new Error(`invalid QingYu guide source for ${locale}`);
    }
    source.sections.forEach((section, index) => {
        if (typeof section.heading !== "string" || !Array.isArray(section.paragraphs) || section.paragraphs.length === 0 ||
            !Array.isArray(section.bullets) || section.bullets.length === 0) {
            throw new Error(`invalid section ${index + 1} in ${locale}`);
        }
    });
};

const buildGuideDocument = (source, identity) => {
    const nextID = makeIDFactory(source.locale);
    const makeBlock = (type, text, extra = {}) => {
        const id = nextID();
        return {
            ID: id,
            Type: type,
            ...extra,
            Properties: {id, updated: "20260813000000"},
            Children: textChildren(text),
        };
    };
    const imageParagraphID = nextID();
    const children = [{
        ID: imageParagraphID,
        Type: "NodeParagraph",
        Properties: {id: imageParagraphID, updated: "20260813000000"},
        Children: [{
            Type: "NodeImage",
            Data: "span",
            Children: [
                {Type: "NodeBang"},
                {Type: "NodeOpenBracket"},
                {Type: "NodeLinkText", Data: source.productName},
                {Type: "NodeCloseBracket"},
                {Type: "NodeOpenParen"},
                {Type: "NodeLinkDest", Data: "assets/qingyu-logo.png"},
                {Type: "NodeCloseParen"},
            ],
        }],
    }];
    for (const section of source.sections) {
        children.push(makeBlock("NodeHeading", section.heading, {HeadingLevel: 2}));
        section.paragraphs.forEach((paragraph) => children.push(makeBlock("NodeParagraph", paragraph)));
        section.bullets.forEach((bullet) => children.push(makeBlock("NodeParagraph", `• ${bullet}`)));
    }
    return {
        ID: identity.rootID,
        Spec: "2",
        Type: "NodeDocument",
        Properties: {
            icon: "1f4d4",
            id: identity.rootID,
            title: source.title,
            type: "doc",
            updated: "20260813000000",
        },
        Children: children,
    };
};

const expectedGuideFiles = (locale, identity) => {
    const source = JSON.parse(fs.readFileSync(path.join(appRoot, "guide-src", `${locale}.json`), "utf8"));
    validateGuideConfig(source, locale);
    const document = buildGuideDocument(source, identity);
    return new Map([
        [".siyuan/conf.json", `${JSON.stringify({
            name: source.title,
            sort: identity.sort,
            icon: "1f4d4",
            closed: false,
            refCreateSaveBox: "",
            refCreateSavePath: "",
            docCreateSaveBox: "",
            docCreateSavePath: "",
            sortMode: 15,
        }, null, 2)}\n`],
        [".siyuan/sort.json", `${JSON.stringify({[identity.rootID]: 0})}\n`],
        [".siyuan/assets.json", "[]\n"],
        [`${identity.rootID}.sy`, `${JSON.stringify(document, null, "\t")}\n`],
    ]);
};

const listRelativeFiles = (root) => {
    const result = [];
    const visit = (directory) => {
        for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
            const target = path.join(directory, entry.name);
            if (entry.isDirectory()) {
                visit(target);
            } else if (entry.isFile()) {
                result.push(path.relative(root, target).split(path.sep).join("/"));
            }
        }
    };
    visit(root);
    return result.sort();
};

const writeGuides = () => {
    const logo = fs.readFileSync(path.join(repositoryRoot, "logo.png"));
    for (const [locale, identity] of Object.entries(GUIDES)) {
        const target = path.join(appRoot, "guide", identity.boxID);
        if (path.dirname(target) !== path.join(appRoot, "guide") || !fs.existsSync(target)) {
            throw new Error(`refusing to replace unexpected guide target ${target}`);
        }
        const files = expectedGuideFiles(locale, identity);
        fs.rmSync(target, {recursive: true});
        for (const [relativePath, content] of files) {
            const output = path.join(target, relativePath);
            fs.mkdirSync(path.dirname(output), {recursive: true});
            fs.writeFileSync(output, content);
        }
        const logoPath = path.join(target, "assets", "qingyu-logo.png");
        fs.mkdirSync(path.dirname(logoPath), {recursive: true});
        fs.writeFileSync(logoPath, logo);
    }
};

const checkGuides = () => {
    const expectedLogo = fs.readFileSync(path.join(repositoryRoot, "logo.png"));
    const errors = [];
    for (const [locale, identity] of Object.entries(GUIDES)) {
        const target = path.join(appRoot, "guide", identity.boxID);
        const files = expectedGuideFiles(locale, identity);
        const expectedPaths = [...files.keys(), "assets/qingyu-logo.png"].sort();
        const actualPaths = fs.existsSync(target) ? listRelativeFiles(target) : [];
        if (JSON.stringify(actualPaths) !== JSON.stringify(expectedPaths)) {
            errors.push(`${locale}: generated file list differs`);
            continue;
        }
        for (const [relativePath, expected] of files) {
            if (fs.readFileSync(path.join(target, relativePath), "utf8") !== expected) {
                errors.push(`${locale}: ${relativePath} differs`);
            }
        }
        if (!fs.readFileSync(path.join(target, "assets", "qingyu-logo.png")).equals(expectedLogo)) {
            errors.push(`${locale}: logo differs from repository logo.png`);
        }
    }
    if (errors.length > 0) {
        throw new Error(errors.join("\n"));
    }
};

if (require.main === module) {
    if (process.argv.includes("--write")) {
        writeGuides();
    } else if (process.argv.includes("--check")) {
        checkGuides();
    } else {
        throw new Error("use --write or --check");
    }
}

module.exports = {GUIDES, buildGuideDocument, validateGuideConfig};
