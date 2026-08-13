const APPROVED_PRODUCT_URLS = new Set([
    "https://apkdv.com/",
    "https://github.com/appdev/QingYu",
    "https://github.com/appdev",
    "mailto:lengyue@apkdv.com",
]);

const SKIPPED_DIRECTORIES = new Set([
    ".git",
    "node_modules",
    "stage/build",
]);

const TEXT_EXTENSIONS = new Set([
    ".cjs", ".css", ".go", ".html", ".js", ".json", ".md", ".sh", ".sy", ".ts", ".tsx", ".txt", ".xml", ".yml", ".yaml",
]);

const AUDIT_SOURCE_EXCLUDES = [
    "app/changelogs/",
    "app/scripts/qingyu-brand-audit.cjs",
    "app/scripts/qingyu-brand-audit.test.cjs",
    "app/scripts/qingyu-brand-policy.cjs",
    "docs/superpowers/",
];

module.exports = {
    APPROVED_PRODUCT_URLS,
    AUDIT_SOURCE_EXCLUDES,
    SKIPPED_DIRECTORIES,
    TEXT_EXTENSIONS,
};
