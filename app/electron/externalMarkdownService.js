const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");

const REGISTRY_VERSION = 2;
const MARKDOWN_EXTENSIONS = new Set([".md", ".markdown"]);
const WINDOWS_RESERVED_NAMES = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i;
const IMAGE_MIME_BY_EXTENSION = new Map([
    [".avif", "image/avif"], [".gif", "image/gif"], [".jpg", "image/jpeg"], [".jpeg", "image/jpeg"],
    [".png", "image/png"], [".svg", "image/svg+xml"], [".webp", "image/webp"],
]);

const createError = (code) => Object.assign(new Error(code), {code});
const isMarkdownFilePath = (filePath) => MARKDOWN_EXTENSIONS.has(path.extname(filePath).toLowerCase());
const pathKey = (filePath) => process.platform === "win32" ? filePath.toLowerCase() : filePath;
const hasControlCharacters = (value) => Array.from(value).some((character) => {
    const codePoint = character.codePointAt(0);
    return codePoint < 32 || codePoint === 127;
});
const replaceControlCharacters = (value, replacement) => Array.from(value, (character) =>
    hasControlCharacters(character) ? replacement : character).join("");
const isInside = (root, target) => {
    const relative = path.relative(root, target);
    return relative === "" || (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative));
};
const decodeRelativeTarget = (target) => {
    if (typeof target !== "string" || hasControlCharacters(target)) throw createError("INVALID_PATH");
    const pathPart = target.split(/[?#]/, 1)[0];
    let decoded;
    try {
        decoded = decodeURIComponent(pathPart).split(/[?#]/, 1)[0];
    } catch {
        throw createError("INVALID_PATH");
    }
    if (hasControlCharacters(decoded)) throw createError("INVALID_PATH");
    if (/%(?:2e|2f|5c)/i.test(decoded) || path.isAbsolute(decoded) || /^[a-z]:[\\/]/i.test(decoded) || decoded.startsWith("\\")) {
        throw createError("PATH_OUTSIDE_SCOPE");
    }
    return decoded;
};
const publicDescriptor = (record) => ({
    capabilityId: record.capabilityId,
    name: path.basename(record.realPath),
    displayPath: record.realPath,
});
const fileIdentity = (stat) => {
    if (stat.ino === undefined || stat.dev === undefined) return undefined;
    const inode = stat.ino.toString();
    const device = stat.dev.toString();
    return inode === "0" && device === "0" ? undefined : `${device}:${inode}`;
};
const assertFileIdentity = (record, stat) => {
    if (record.identity && fileIdentity(stat) !== record.identity) throw createError("FILE_IDENTITY_CHANGED");
};

const decodeUtf8 = (bytes) => {
    try {
        return new TextDecoder("utf-8", {fatal: true}).decode(bytes);
    } catch {
        throw createError("INVALID_UTF8");
    }
};

const detectLineEnding = (content) => {
    const counts = {"\r\n": 0, "\n": 0, "\r": 0};
    const first = {"\r\n": Number.POSITIVE_INFINITY, "\n": Number.POSITIVE_INFINITY, "\r": Number.POSITIVE_INFINITY};
    for (let index = 0; index < content.length; index++) {
        if (content[index] === "\r" && content[index + 1] === "\n") {
            counts["\r\n"]++;
            first["\r\n"] = Math.min(first["\r\n"], index);
            index++;
        } else if (content[index] === "\n") {
            counts["\n"]++;
            first["\n"] = Math.min(first["\n"], index);
        } else if (content[index] === "\r") {
            counts["\r"]++;
            first["\r"] = Math.min(first["\r"], index);
        }
    }
    if (counts["\r\n"] + counts["\n"] + counts["\r"] === 0) return "\n";
    return ["\r\n", "\n", "\r"].sort((left, right) => counts[right] - counts[left] || first[left] - first[right])[0];
};

const detectMarkdownFormat = (bytes) => {
    const utf8Bom = bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf;
    const content = decodeUtf8(utf8Bom ? bytes.subarray(3) : bytes);
    return {content, utf8Bom, lineEnding: detectLineEnding(content)};
};

const encodeMarkdownContent = (content, {utf8Bom, lineEnding}) => {
    const normalized = content.replace(/\r\n|\r/g, "\n");
    const body = Buffer.from(lineEnding === "\n" ? normalized : normalized.replace(/\n/g, lineEnding), "utf8");
    return utf8Bom ? Buffer.concat([Buffer.from([0xef, 0xbb, 0xbf]), body]) : body;
};

const computeRevision = async (filePath, bytes) => {
    const stat = await fs.stat(filePath, {bigint: true});
    return crypto.createHash("sha256")
        .update(`${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:`)
        .update(bytes)
        .digest("hex");
};

const hasImageSignature = (mimeType, bytes) => {
    const buffer = Buffer.from(bytes);
    if (mimeType === "image/png") return buffer.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    if (mimeType === "image/jpeg") return buffer[0] === 0xff && buffer[1] === 0xd8 && buffer[2] === 0xff;
    if (mimeType === "image/gif") return buffer.subarray(0, 6).toString("ascii") === "GIF87a" || buffer.subarray(0, 6).toString("ascii") === "GIF89a";
    if (mimeType === "image/webp") return buffer.subarray(0, 4).toString("ascii") === "RIFF" && buffer.subarray(8, 12).toString("ascii") === "WEBP";
    if (mimeType === "image/avif") return buffer.subarray(4, 12).toString("ascii").includes("ftypavif");
    if (mimeType === "image/svg+xml") return /^\s*(?:<\?xml[^>]*>\s*)?<svg[\s>]/iu.test(buffer.toString("utf8"));
    return false;
};

const safeAssetName = (name) => {
    const extension = path.extname(name).toLowerCase();
    const stem = replaceControlCharacters(path.basename(name, path.extname(name)).normalize("NFC"), "-")
        .replace(/[<>:"/\\|?*]+/g, "-")
        .replace(/\s+/g, "-")
        .replace(/^-+|-+$/g, "") || "image";
    return `${stem}${extension}`;
};

class ExternalMarkdownService {
    static async create(options) {
        const service = new ExternalMarkdownService(options);
        await service.loadRegistry();
        return service;
    }

    constructor({registryPath, randomUUID = crypto.randomUUID, pruneDelayMs = 5000, beforeReplace}) {
        this.registryPath = registryPath;
        this.randomUUID = randomUUID;
        this.pruneDelayMs = pruneDelayMs;
        this.beforeReplace = beforeReplace;
        this.capabilities = new Map();
        this.workspaceLayoutReferences = new Map();
        this.appearanceReferences = new Set();
        this.runtimeOwners = new Map();
        this.resourceTokens = new Map();
        this.pruneTimers = new Map();
    }

    async loadRegistry() {
        let registry;
        try {
            registry = JSON.parse(await fs.readFile(this.registryPath, "utf8"));
        } catch (error) {
            if (error.code !== "ENOENT") return;
            return;
        }
        if (![1, REGISTRY_VERSION].includes(registry?.version) || !registry.capabilities ||
            !registry.workspaceLayoutReferences) return;
        for (const [capabilityId, record] of Object.entries(registry.capabilities)) {
            if (record && typeof record.realPath === "string") {
                this.capabilities.set(capabilityId, {...record, capabilityId});
            }
        }
        for (const [workspaceKey, ids] of Object.entries(registry.workspaceLayoutReferences)) {
            if (Array.isArray(ids)) this.workspaceLayoutReferences.set(workspaceKey, new Set(ids));
        }
        if (Array.isArray(registry.appearanceReferences)) {
            registry.appearanceReferences.forEach((capabilityId) => {
                if (this.capabilities.has(capabilityId)) this.appearanceReferences.add(capabilityId);
            });
        }
    }

    async grantFromSystem(filePath) {
        if (!isMarkdownFilePath(filePath)) throw createError("UNSUPPORTED_TYPE");
        let realPath;
        try {
            realPath = await fs.realpath(path.resolve(filePath));
        } catch {
            throw createError("FILE_NOT_FOUND");
        }
        const stat = await fs.lstat(realPath);
        if (!stat.isFile()) throw createError("NOT_A_FILE");
        detectMarkdownFormat(await fs.readFile(realPath));
        for (const record of this.capabilities.values()) {
            if (pathKey(record.realPath) === pathKey(realPath) && (!record.identity || record.identity === fileIdentity(stat))) {
                return publicDescriptor(record);
            }
        }
        const capabilityId = this.randomUUID();
        const record = {capabilityId, realPath, identity: fileIdentity(stat)};
        this.capabilities.set(capabilityId, record);
        await this.persist();
        return publicDescriptor(record);
    }

    async grantRelativeMarkdown(parentCapabilityId, target) {
        const parent = this.capabilities.get(parentCapabilityId);
        if (!parent) throw createError("UNKNOWN_CAPABILITY");
        const root = path.dirname(parent.realPath);
        const candidate = path.resolve(root, decodeRelativeTarget(target));
        if (!isInside(root, candidate)) throw createError("PATH_OUTSIDE_SCOPE");
        let realPath;
        try {
            realPath = await fs.realpath(candidate);
        } catch {
            throw createError("FILE_NOT_FOUND");
        }
        if (!isInside(root, realPath)) throw createError("PATH_OUTSIDE_SCOPE");
        return this.grantFromSystem(realPath);
    }

    async resolveRelativeFile(parentCapabilityId, target) {
        const parent = this.capabilities.get(parentCapabilityId);
        if (!parent) throw createError("UNKNOWN_CAPABILITY");
        const root = path.dirname(parent.realPath);
        const candidate = path.resolve(root, decodeRelativeTarget(target));
        if (!isInside(root, candidate)) throw createError("PATH_OUTSIDE_SCOPE");
        const realPath = await fs.realpath(candidate).catch(() => { throw createError("FILE_NOT_FOUND"); });
        if (!isInside(root, realPath)) throw createError("PATH_OUTSIDE_SCOPE");
        if (!(await fs.lstat(realPath)).isFile()) throw createError("NOT_A_FILE");
        return realPath;
    }

    getDescriptor(capabilityId) {
        const record = this.capabilities.get(capabilityId);
        return record ? publicDescriptor(record) : undefined;
    }

    async retainCapability(capabilityId, webContentsId) {
        if (!this.capabilities.has(capabilityId)) throw createError("UNKNOWN_CAPABILITY");
        const owner = this.runtimeOwners.get(capabilityId);
        if (owner !== undefined && owner !== webContentsId) throw createError("CAPABILITY_IN_USE");
        const timer = this.pruneTimers.get(capabilityId);
        if (timer) clearTimeout(timer);
        this.pruneTimers.delete(capabilityId);
        this.runtimeOwners.set(capabilityId, webContentsId);
        if (!this.resourceTokens.has(capabilityId)) this.resourceTokens.set(capabilityId, this.randomUUID());
    }

    findCapabilityOwner(capabilityId) {
        return this.runtimeOwners.get(capabilityId);
    }

    getResourceToken(capabilityId, webContentsId) {
        if (this.runtimeOwners.get(capabilityId) !== webContentsId) throw createError("UNAUTHORIZED");
        return this.resourceTokens.get(capabilityId);
    }

    verifyResourceToken(capabilityId, token) {
        return this.runtimeOwners.has(capabilityId) && typeof token === "string" &&
            this.resourceTokens.get(capabilityId) === token;
    }

    async releaseWindowCapabilities(webContentsId) {
        for (const [capabilityId, owner] of this.runtimeOwners) {
            if (owner !== webContentsId) continue;
            this.runtimeOwners.delete(capabilityId);
            this.resourceTokens.delete(capabilityId);
            this.schedulePrune(capabilityId);
        }
    }

    async releaseCapability(capabilityId, webContentsId) {
        if (this.runtimeOwners.get(capabilityId) !== webContentsId) return;
        this.runtimeOwners.delete(capabilityId);
        this.resourceTokens.delete(capabilityId);
        this.schedulePrune(capabilityId);
    }

    async setWorkspaceLayoutReferences(workspaceKey, capabilityIds) {
        const references = new Set(capabilityIds.filter((capabilityId) => this.capabilities.has(capabilityId)));
        this.workspaceLayoutReferences.set(workspaceKey, references);
        for (const capabilityId of this.capabilities.keys()) this.schedulePrune(capabilityId);
        await this.persist();
    }

    async setAppearanceReference(capabilityId, retained) {
        if (!this.capabilities.has(capabilityId)) throw createError("UNKNOWN_CAPABILITY");
        if (retained) {
            this.appearanceReferences.add(capabilityId);
            const timer = this.pruneTimers.get(capabilityId);
            if (timer) clearTimeout(timer);
            this.pruneTimers.delete(capabilityId);
        } else {
            this.appearanceReferences.delete(capabilityId);
            this.schedulePrune(capabilityId);
        }
        await this.persist();
    }

    schedulePrune(capabilityId) {
        if (this.runtimeOwners.has(capabilityId) || this.appearanceReferences.has(capabilityId) ||
            [...this.workspaceLayoutReferences.values()].some((ids) => ids.has(capabilityId))) {
            return;
        }
        if (this.pruneTimers.has(capabilityId)) return;
        const timer = setTimeout(() => {
            this.pruneTimers.delete(capabilityId);
            if (this.runtimeOwners.has(capabilityId) || this.appearanceReferences.has(capabilityId) ||
                [...this.workspaceLayoutReferences.values()].some((ids) => ids.has(capabilityId))) {
                return;
            }
            this.capabilities.delete(capabilityId);
            void this.persist();
        }, this.pruneDelayMs);
        timer.unref?.();
        this.pruneTimers.set(capabilityId, timer);
    }

    async read(capabilityId) {
        const record = this.capabilities.get(capabilityId);
        if (!record) throw createError("UNKNOWN_CAPABILITY");
        const bytes = await fs.readFile(record.realPath);
        const stat = await fs.stat(record.realPath);
        assertFileIdentity(record, stat);
        const format = detectMarkdownFormat(bytes);
        record.format = {utf8Bom: format.utf8Bom, lineEnding: format.lineEnding};
        return {
            ...publicDescriptor(record),
            ...format,
            revision: await computeRevision(record.realPath, bytes),
            mtime: stat.mtimeMs,
        };
    }

    async save(capabilityId, {content, revision, overwriteRevision}) {
        const record = this.capabilities.get(capabilityId);
        if (!record) return {status: "error", code: "UNKNOWN_CAPABILITY"};
        try {
            const currentBytes = await fs.readFile(record.realPath);
            const stat = await fs.stat(record.realPath);
            assertFileIdentity(record, stat);
            const currentRevision = await computeRevision(record.realPath, currentBytes);
            const authorizedRevision = overwriteRevision || revision;
            if (currentRevision !== authorizedRevision) return {status: "conflict", revision: currentRevision};
            const format = record.format || detectMarkdownFormat(currentBytes);
            const bytes = encodeMarkdownContent(content, format);
            const temporaryPath = path.join(
                path.dirname(record.realPath),
                `.${path.basename(record.realPath)}.${this.randomUUID()}.tmp`,
            );
            let handle;
            try {
                handle = await fs.open(temporaryPath, "wx", stat.mode & 0o777);
                await handle.writeFile(bytes);
                await handle.sync();
                await handle.close();
                handle = undefined;
                await fs.chmod(temporaryPath, stat.mode & 0o777);
                await this.beforeReplace?.({capabilityId, filePath: record.realPath, temporaryPath});
                const replacementBytes = await fs.readFile(record.realPath);
                assertFileIdentity(record, await fs.stat(record.realPath));
                const replacementRevision = await computeRevision(record.realPath, replacementBytes);
                if (replacementRevision !== authorizedRevision) {
                    await fs.rm(temporaryPath, {force: true});
                    return {status: "conflict", revision: replacementRevision};
                }
                await fs.rename(temporaryPath, record.realPath);
                record.identity = fileIdentity(await fs.stat(record.realPath));
                await this.persist();
            } catch (error) {
                await fs.rm(temporaryPath, {force: true}).catch(() => undefined);
                throw error;
            } finally {
                await handle?.close().catch(() => undefined);
            }
            return {status: "ok", document: await this.read(capabilityId)};
        } catch (error) {
            return {status: "error", code: error.code || "WRITE_FAILED"};
        }
    }

    async rename(capabilityId, {name, revision}) {
        const record = this.capabilities.get(capabilityId);
        if (!record) return {status: "error", code: "UNKNOWN_CAPABILITY"};
        if (typeof name !== "string" || name !== path.basename(name) || !isMarkdownFilePath(name) ||
            WINDOWS_RESERVED_NAMES.test(name) || hasControlCharacters(name) || /[<>:"/\\|?*]/.test(name)) {
            return {status: "error", code: "INVALID_NAME"};
        }
        try {
            const currentBytes = await fs.readFile(record.realPath);
            assertFileIdentity(record, await fs.stat(record.realPath));
            const currentRevision = await computeRevision(record.realPath, currentBytes);
            if (currentRevision !== revision) return {status: "conflict", revision: currentRevision};
            const previousPath = record.realPath;
            const nextPath = path.join(path.dirname(previousPath), name);
            if (pathKey(previousPath) === pathKey(nextPath)) return {status: "ok", document: await this.read(capabilityId)};
            try {
                await fs.link(previousPath, nextPath);
            } catch (error) {
                if (error.code === "EEXIST") return {status: "error", code: "TARGET_EXISTS"};
                throw error;
            }
            try {
                await fs.unlink(previousPath);
                record.realPath = await fs.realpath(nextPath);
                await this.persist();
            } catch (error) {
                await fs.rm(nextPath, {force: true}).catch(() => undefined);
                record.realPath = previousPath;
                throw error;
            }
            return {status: "ok", document: await this.read(capabilityId)};
        } catch (error) {
            return {status: "error", code: error.code || "RENAME_FAILED"};
        }
    }

    async saveAssets(capabilityId, assets) {
        const record = this.capabilities.get(capabilityId);
        if (!record) throw createError("UNKNOWN_CAPABILITY");
        const root = path.dirname(record.realPath);
        const assetsDirectory = path.join(root, "assets");
        try {
            const stat = await fs.lstat(assetsDirectory);
            if (!stat.isDirectory() || stat.isSymbolicLink()) throw createError("UNSAFE_ASSETS_DIRECTORY");
        } catch (error) {
            if (error.code !== "ENOENT") throw error;
            await fs.mkdir(assetsDirectory, {mode: 0o755});
        }
        const assetsRealPath = await fs.realpath(assetsDirectory);
        if (!isInside(root, assetsRealPath)) throw createError("UNSAFE_ASSETS_DIRECTORY");
        const saved = [];
        for (const asset of assets) {
            const extensionMime = IMAGE_MIME_BY_EXTENSION.get(path.extname(asset.name).toLowerCase());
            if (!extensionMime || extensionMime !== asset.mimeType || !hasImageSignature(asset.mimeType, asset.bytes)) {
                throw createError("INVALID_ASSET");
            }
            const parsed = path.parse(safeAssetName(asset.name));
            let index = 0;
            while (true) {
                const name = index === 0 ? `${parsed.name}${parsed.ext}` : `${parsed.name}-${index}${parsed.ext}`;
                const target = path.join(assetsDirectory, name);
                try {
                    await fs.writeFile(target, Buffer.from(asset.bytes), {flag: "wx", mode: 0o644});
                    saved.push({name: asset.name, markdownDestination: `assets/${name}`});
                    break;
                } catch (error) {
                    if (error.code !== "EEXIST") throw error;
                    index++;
                }
            }
        }
        return saved;
    }

    async resolveResource(capabilityId, relativePath) {
        const record = this.capabilities.get(capabilityId);
        if (!record) throw createError("UNKNOWN_CAPABILITY");
        const root = path.dirname(record.realPath);
        const candidate = path.resolve(root, decodeRelativeTarget(relativePath));
        if (!isInside(root, candidate)) throw createError("PATH_OUTSIDE_SCOPE");
        const realPath = await fs.realpath(candidate).catch(() => { throw createError("FILE_NOT_FOUND"); });
        if (!isInside(root, realPath)) throw createError("PATH_OUTSIDE_SCOPE");
        const stat = await fs.lstat(realPath);
        if (!stat.isFile()) throw createError("NOT_A_FILE");
        const mimeType = IMAGE_MIME_BY_EXTENSION.get(path.extname(realPath).toLowerCase());
        if (!mimeType) throw createError("UNSUPPORTED_RESOURCE");
        return {path: realPath, mimeType};
    }

    async persist() {
        await fs.mkdir(path.dirname(this.registryPath), {recursive: true});
        const registry = {
            version: REGISTRY_VERSION,
            capabilities: Object.fromEntries(this.capabilities),
            workspaceLayoutReferences: Object.fromEntries(
                [...this.workspaceLayoutReferences].map(([key, value]) => [key, [...value]]),
            ),
            appearanceReferences: [...this.appearanceReferences],
        };
        const temporaryPath = `${this.registryPath}.${this.randomUUID()}.tmp`;
        await fs.writeFile(temporaryPath, `${JSON.stringify(registry, null, 2)}\n`, {flag: "wx"});
        await fs.rename(temporaryPath, this.registryPath);
    }
}

module.exports = {ExternalMarkdownService, detectMarkdownFormat, encodeMarkdownContent, fileIdentity, isMarkdownFilePath};
