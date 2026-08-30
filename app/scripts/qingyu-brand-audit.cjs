#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const {spawnSync} = require("node:child_process");
const {APPROVED_PRODUCT_URLS, AUDIT_SOURCE_EXCLUDES, SKIPPED_DIRECTORIES, TEXT_EXTENSIONS} = require("./qingyu-brand-policy.cjs");

const normalize = (value) => value.split(path.sep).join("/");
const isAuditedTextFile = (relativePath) => {
    const normalized = normalize(relativePath);
    return normalized === "Dockerfile" || TEXT_EXTENSIONS.has(path.extname(normalized).toLowerCase());
};

const shouldSkipDirectory = (relativePath) => {
    const normalized = normalize(relativePath);
    return [...SKIPPED_DIRECTORIES].some((entry) => normalized === entry || normalized.endsWith(`/${entry}`));
};

const listTextFiles = (root) => {
    const gitFiles = spawnSync("git", ["-C", root, "ls-files", "-z", "--cached", "--others", "--exclude-standard"], {encoding: "utf8"});
    if (gitFiles.status === 0 && gitFiles.stdout) {
        return gitFiles.stdout.split("\0").filter(Boolean).filter((relativePath) => {
            const normalized = normalize(relativePath);
            return isAuditedTextFile(normalized) &&
                !AUDIT_SOURCE_EXCLUDES.some((excluded) => normalized === excluded || normalized.startsWith(excluded));
        }).map((relativePath) => ({
            absolutePath: path.join(root, relativePath),
            relativePath: normalize(relativePath),
        })).filter(({absolutePath}) => fs.existsSync(absolutePath));
    }
    const files = [];
    const visit = (directory) => {
        for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
            const absolutePath = path.join(directory, entry.name);
            const relativePath = path.relative(root, absolutePath);
            if (entry.isDirectory()) {
                if (!shouldSkipDirectory(relativePath)) {
                    visit(absolutePath);
                }
                continue;
            }
            if (entry.isFile() && isAuditedTextFile(relativePath)) {
                const normalized = normalize(relativePath);
                if (!AUDIT_SOURCE_EXCLUDES.some((excluded) => normalized === excluded || normalized.startsWith(excluded))) {
                    files.push({absolutePath, relativePath: normalized});
                }
            }
        }
    };
    visit(root);
    return files;
};

const isExactCompatibilityLine = (file, line) => {
    if (/\.go$/.test(file) && /^\s*(?:import\s+)?["`]github\.com\/siyuan-note\/(?:siyuan|[\w-]+)(?:\/[^"`]*)?["`]\s*$/.test(line)) {
        return true;
    }
    if (/app\/src\/.*\.(?:ts|tsx)$/.test(file) && /(?:window\.siyuan|["`]web\+siyuan:|["`]siyuan:)/.test(line)) {
        return true;
    }
    if (/^(?:LICENSE|NOTICE\.md)$/.test(file) && /(?:AGPL|official|官方|上游|copyright|Copyright|commit\/|issues\/|pull\/)/.test(line)) {
        return true;
    }
    return false;
};

const addViolation = (violations, rule, file, lineNumber, line) => {
    violations.push({
        rule,
        file,
        line: lineNumber,
        excerpt: line.trim().slice(0, 180),
    });
};

const auditRepository = (root) => {
    const violations = [];
    for (const {absolutePath, relativePath} of listTextFiles(root)) {
        const lines = fs.readFileSync(absolutePath, "utf8").split(/\r?\n/);
        lines.forEach((line, index) => {
            const lineNumber = index + 1;
            const productSurface = /^(?:README(?:\.[^/]+)?\.md|NOTICE\.md|docs\/legal\/|app\/(?:appearance\/langs|electron|guide|guide-src)\/)/.test(relativePath);
            const dockerRuntimeSurface = relativePath === "Dockerfile" || relativePath === "kernel/entrypoint.sh";
            const accessAuthRuntimeSurface = productSurface || dockerRuntimeSurface || relativePath === "kernel/util/working.go";
            if (dockerRuntimeSurface && /\/opt\/siyuan|\/home\/siyuan|\/siyuan\/workspace|SIYUAN_WORKSPACE_PATH|\/kernel\/kernel\b|:-siyuan}/.test(line)) {
                addViolation(violations, "upstream-docker-runtime-identity", relativePath, lineNumber, line);
            }
            if (accessAuthRuntimeSurface && /SIYUAN_ACCESS_AUTH_CODE(?:_BYPASS)?/.test(line)) {
                addViolation(violations, "upstream-access-auth-environment", relativePath, lineNumber, line);
            }
            if (/(?:release\.b3log\.org|release\.liuyun\.io|github\.com\/siyuan-note\/siyuan\/releases)/i.test(line)) {
                addViolation(violations, "upstream-update-service", relativePath, lineNumber, line);
            }
            if (productSurface && /(?:b3log\.org\/siyuan|ld246\.com\/article|liuyun\.io\/article)/i.test(line)) {
                addViolation(violations, "upstream-product-service", relativePath, lineNumber, line);
            }
            if (/siyuan-[\w.${}-]+\.(?:exe|dmg|deb|AppImage|rpm)/i.test(line)) {
                addViolation(violations, "upstream-package-name", relativePath, lineNumber, line);
            }
            if (/iconSiYuan/.test(line) && relativePath !== "app/appearance/icons/litheness/icon.js") {
                addViolation(violations, "upstream-logo-consumer", relativePath, lineNumber, line);
            }
            if (/(?:SiYuan official download|<[^>]+>\s*(?:SiYuan|思源笔记?)\s*<|title\s*=\s*["'](?:SiYuan|思源笔记?))/i.test(line) &&
                !isExactCompatibilityLine(relativePath, line)) {
                addViolation(violations, "user-visible-upstream-name", relativePath, lineNumber, line);
            }
        });
    }
    return {violations};
};

const runCLI = () => {
    const args = process.argv.slice(2);
    const rootIndex = args.indexOf("--root");
    const root = rootIndex >= 0 ? path.resolve(args[rootIndex + 1]) : path.resolve(__dirname, "../..");
    const result = auditRepository(root);
    for (const violation of result.violations) {
        process.stderr.write(`${violation.rule} ${violation.file}:${violation.line} ${violation.excerpt}\n`);
    }
    if (result.violations.length > 0) {
        process.exitCode = 1;
    } else {
        process.stdout.write(`QingYu brand audit passed (${APPROVED_PRODUCT_URLS.size} approved product URLs).\n`);
    }
};

if (require.main === module) {
    runCLI();
}

module.exports = {auditRepository};
