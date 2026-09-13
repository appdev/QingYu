const fs = require("node:fs/promises");
const {constants} = require("node:fs");
const crypto = require("node:crypto");
const path = require("node:path");

const suffix = ".annotations.json";
const fail = (code = "INVALID_ANNOTATIONS") => Object.assign(new Error(code), {code});
const identity = (stat) => `${stat.dev}:${stat.ino}`;
const hash = (text) => crypto.createHash("sha256").update(text.replace(/^\uFEFF/u, "").replace(/\r\n?/gu, "\n")).digest("hex");

const validate = (data, content) => {
    if (!data || data.schemaVersion !== 1 || typeof data.documentId !== "string" || !/^[\w-]{1,128}$/u.test(data.documentId) ||
        !Number.isSafeInteger(data.revision) || data.revision < 0 || data.revision >= Number.MAX_SAFE_INTEGER ||
        typeof data.contentHash !== "string" || !/^(?:[a-f\d]{64})?$/u.test(data.contentHash) ||
        !Array.isArray(data.records) || data.records.length > 10000) throw fail();
    const text = content?.replace(/^\uFEFF/u, "").replace(/\r\n?/gu, "\n");
    const ids = new Set();
    for (const record of data.records) {
        if (!record || typeof record.id !== "string" || !/^[\w-]{1,128}$/u.test(record.id) || ids.has(record.id) ||
            !Number.isSafeInteger(record.from) || !Number.isSafeInteger(record.to) || record.from < 0 ||
            record.to < record.from || !["attached", "orphaned"].includes(record.status) ||
            typeof record.quote !== "string" || !record.quote || typeof record.note !== "string" ||
            record.note.length > 65536 || typeof record.prefix !== "string" || Array.from(record.prefix).length > 64 ||
            typeof record.suffix !== "string" || Array.from(record.suffix).length > 64 ||
            !Number.isSafeInteger(record.createdAt) || record.createdAt < 0 ||
            !Number.isSafeInteger(record.updatedAt) || record.updatedAt < 0 ||
            (text !== undefined && record.status === "attached" &&
                (record.from >= record.to || record.to > text.length || text.slice(record.from, record.to) !== record.quote))) throw fail();
        ids.add(record.id);
    }
    if (Buffer.byteLength(JSON.stringify(data)) > 16 * 1024 * 1024) throw fail();
    return data;
};

const readFile = async (file) => {
    let handle;
    try {
        handle = await fs.open(file, constants.O_RDONLY | (constants.O_NOFOLLOW || 0));
        const stat = await handle.stat();
        const linkStat = await fs.lstat(file);
        if (!stat.isFile() || linkStat.isSymbolicLink() || identity(stat) !== identity(linkStat) ||
            stat.size > 16 * 1024 * 1024) throw fail();
        const bytes = await handle.readFile();
        return {identity: identity(stat), bytes, digest: crypto.createHash("sha256").update(bytes).digest("hex")};
    } catch (error) {
        if (error.code === "ENOENT") return null;
        throw error;
    } finally {
        await handle?.close();
    }
};

const read = async (documentPath) => {
    const file = await readFile(documentPath + suffix);
    if (!file) return null;
    return {...file, data: {...validate(JSON.parse(file.bytes.toString("utf8"))), etag: file.digest}};
};

const syncParent = async (file) => {
    if (process.platform === "win32") return;
    const handle = await fs.open(path.dirname(file), "r");
    try { await handle.sync(); } finally { await handle.close(); }
};

const stage = async (documentPath, data, content) => {
    validate(data, content);
    const old = await read(documentPath);
    if ((!old && data.revision !== 0) || (old && (old.data.revision !== data.revision ||
        old.data.documentId !== data.documentId || old.digest !== data.etag))) throw fail("ANNOTATION_CONFLICT");
    const next = {...data, etag: undefined, revision: data.revision + 1, contentHash: hash(content)};
    const target = documentPath + suffix;
    const staging = `${target}.pending-${crypto.randomUUID()}`;
    const handle = await fs.open(staging, "wx", 0o600);
    try { await handle.writeFile(JSON.stringify(next)); await handle.sync(); } finally { await handle.close(); }
    const file = await readFile(staging);
    await syncParent(staging);
    return {target, staging, identity: file.identity, digest: file.digest,
        oldIdentity: old?.identity, oldDigest: old?.digest, backup: `${staging}.old`};
};

const matches = (file, id, digest) => file && file.identity === id && file.digest === digest;
const commit = async (documentPath, tx) => {
    const expected = documentPath + suffix;
    if (tx.target !== expected || !tx.staging.startsWith(`${expected}.pending-`) ||
        path.dirname(tx.staging) !== path.dirname(expected) || tx.backup !== `${tx.staging}.old`) throw fail();
    const current = await readFile(tx.target);
    if (!matches(current, tx.identity, tx.digest)) {
        const staged = await readFile(tx.staging);
        if (!matches(staged, tx.identity, tx.digest)) throw fail("ANNOTATION_CONFLICT");
        if (current) {
            if (!matches(current, tx.oldIdentity, tx.oldDigest)) throw fail("ANNOTATION_CONFLICT");
            // 先隔离旧文件，再以不覆盖方式安装；失败时原始内容留在恢复文件中。
            const backup = await readFile(tx.backup);
            if (!backup) await fs.link(tx.target, tx.backup);
            else if (!matches(backup, tx.oldIdentity, tx.oldDigest)) throw fail("ANNOTATION_CONFLICT");
            if (!matches(await readFile(tx.target), tx.oldIdentity, tx.oldDigest)) throw fail("ANNOTATION_CONFLICT");
            await fs.unlink(tx.target);
            await syncParent(tx.target);
        } else if (tx.oldIdentity && !matches(await readFile(tx.backup), tx.oldIdentity, tx.oldDigest)) {
            throw fail("ANNOTATION_CONFLICT");
        }
        await fs.link(tx.staging, tx.target);
        await syncParent(tx.target);
    }
};

const cleanup = async (tx) => {
    for (const [file, id, digest] of [[tx.staging, tx.identity, tx.digest], [tx.backup, tx.oldIdentity, tx.oldDigest]]) {
        const current = await readFile(file);
        if (current) {
            if (!matches(current, id, digest)) throw fail("ANNOTATION_CONFLICT");
            await fs.unlink(file);
        }
    }
};

module.exports = {read, readFile, stage, commit, cleanup, hash, validate, suffix, syncParent};
