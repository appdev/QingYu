const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const {GUIDES, buildGuideDocument, validateGuideConfig} = require("./generate-qingyu-guide.cjs");

test("guide identities preserve the four mounted notebook and root IDs", () => {
    assert.deepEqual(Object.values(GUIDES).map(({boxID, rootID}) => [boxID, rootID]), [
        ["20210808180117-czj9bvb", "20200812220555-lj3enxa"],
        ["20211226090932-5lcq56f", "20211226115423-d5z1joq"],
        ["20210808180117-6v0mkxr", "20200923234011-ieuun1p"],
        ["20240530133126-axarxgx", "20240530101000-4qitucx"],
    ]);
});

test("generated guides are valid Spec 2 documents with approved identity", () => {
    for (const [locale, identity] of Object.entries(GUIDES)) {
        const source = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "guide-src", `${locale}.json`), "utf8"));
        validateGuideConfig(source, locale);
        const document = buildGuideDocument(source, identity);
        assert.equal(document.ID, identity.rootID);
        assert.equal(document.Spec, "2");
        assert.equal(document.Type, "NodeDocument");
        assert.equal(document.Properties.id, identity.rootID);
        assert.equal(document.Properties.type, "doc");
        assert.ok(document.Children.length >= 16);
        const serialized = JSON.stringify(document);
        assert.match(serialized, /AGPL-3\.0/);
        assert.match(serialized, /lengyue@apkdv\.com/);
        assert.doesNotMatch(serialized, /b3log\.org\/siyuan|siyuan-note\/siyuan\/releases|会员|會員|membership|メンバーシップ/);
    }
});

test("guide validation rejects an unknown locale and incomplete sections", () => {
    assert.throws(() => validateGuideConfig({locale: "fr", sections: []}, "fr"));
});
