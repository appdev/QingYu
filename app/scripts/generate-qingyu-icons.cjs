#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const sharp = require("sharp");

const appRoot = path.resolve(__dirname, "..");
const repositoryRoot = path.resolve(appRoot, "..");
const sizes = [16, 32, 48, 64, 128, 256, 512];
const shared512 = [
    "electron/icon.png",
    "src/assets/icon.png",
    "stage/icon.png",
    "stage/icon-large.png",
    "stage/images/icon.png",
];

const buildPNGs = async () => {
    const source = fs.readFileSync(path.join(repositoryRoot, "logo.png"));
    const outputs = new Map();
    for (const size of sizes) {
        outputs.set(`src/assets/icon/${size}x${size}.png`, await sharp(source).resize(size, size).png().toBuffer());
    }
    const desktop = outputs.get("src/assets/icon/512x512.png");
    shared512.forEach((target) => outputs.set(target, desktop));
    return outputs;
};

const writePNGs = async () => {
    for (const [relativePath, content] of await buildPNGs()) {
        const target = path.join(appRoot, relativePath);
        fs.mkdirSync(path.dirname(target), {recursive: true});
        fs.writeFileSync(target, content);
    }
};

const checkPNGs = async () => {
    const errors = [];
    for (const [relativePath, expected] of await buildPNGs()) {
        const target = path.join(appRoot, relativePath);
        if (!fs.existsSync(target) || !fs.readFileSync(target).equals(expected)) {
            errors.push(relativePath);
        }
    }
    if (errors.length > 0) {
        throw new Error(`QingYu icons differ from logo.png: ${errors.join(", ")}`);
    }
};

if (require.main === module) {
    const action = process.argv.includes("--write") ? writePNGs : process.argv.includes("--check") ? checkPNGs : null;
    if (!action) {
        throw new Error("use --write or --check");
    }
    action().catch((error) => {
        process.stderr.write(`${error.stack || error}\n`);
        process.exitCode = 1;
    });
}

module.exports = {buildPNGs};
