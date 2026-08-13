import * as assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {readFile} from "node:fs/promises";
import {join} from "node:path";
import test from "node:test";
import sharp from "sharp";

const repositoryRoot = join(__dirname, "../../..");
const readRepositoryFile = (path: string) => readFile(join(repositoryRoot, path));
const hash = (content: Buffer) => createHash("sha256").update(content).digest("hex");

test("QingYu desktop branding uses the product icon everywhere", async () => {
    const [sourceIcon, packagedIcon, desktopIcon, electronIcon, stagedIcon, stagedLargeIcon, linuxIcon] =
        await Promise.all([
            readRepositoryFile("macos-icon.icns"),
            readRepositoryFile("app/src/assets/icon.icns"),
            readRepositoryFile("app/src/assets/icon.png"),
            readRepositoryFile("app/electron/icon.png"),
            readRepositoryFile("app/stage/icon.png"),
            readRepositoryFile("app/stage/icon-large.png"),
            readRepositoryFile("app/src/assets/icon/512x512.png"),
        ]);

    assert.equal(hash(packagedIcon), hash(sourceIcon));
    assert.equal(hash(electronIcon), hash(desktopIcon));
    assert.equal(hash(stagedIcon), hash(desktopIcon));
    assert.equal(hash(stagedLargeIcon), hash(desktopIcon));
    assert.equal(hash(linuxIcon), hash(desktopIcon));

    const rootLogo = await readRepositoryFile("logo.png");
    const expectedDesktopIcon = await sharp(rootLogo).resize(512, 512).png().toBuffer();
    assert.equal(hash(desktopIcon), hash(expectedDesktopIcon));

    for (const size of [16, 32, 48, 64, 128, 256, 512]) {
        const actual = await readRepositoryFile(`app/src/assets/icon/${size}x${size}.png`);
        const expected = await sharp(rootLogo).resize(size, size).png().toBuffer();
        assert.equal(hash(actual), hash(expected), `${size}x${size}`);
    }
});

test("QingYu startup surfaces do not fall back to SiYuan branding", async () => {
    const [mainSource, bootSource, loadingSource, stagedLoadingSource, appTemplate, desktopTemplate, mobileTemplate] =
        await Promise.all([
            readRepositoryFile("app/electron/main.js"),
            readRepositoryFile("app/electron/boot.html"),
            readRepositoryFile("app/src/assets/loading-pure.svg"),
            readRepositoryFile("app/stage/loading-pure.svg"),
            readRepositoryFile("app/src/assets/template/app/index.tpl"),
            readRepositoryFile("app/src/assets/template/desktop/index.tpl"),
            readRepositoryFile("app/src/assets/template/mobile/index.tpl"),
        ]).then((files) => files.map((file) => file.toString()));

    assert.match(mainSource, /app\.dock\.setIcon\(path\.join\(appDir, "stage", "icon-large\.png"\)\)/);
    assert.match(bootSource, /\.\.\/stage\/icon-large\.png/);
    assert.doesNotMatch(bootSource, /#d23f31|#3b3e43/);
    assert.equal(stagedLoadingSource, loadingSource);
    assert.doesNotMatch(loadingSource, /#d23f31|#3b3e43/);
    for (const template of [appTemplate, desktopTemplate, mobileTemplate]) {
        assert.match(template, /\.\.\/\.\.\/icon\.png/);
        assert.doesNotMatch(template, /icon\.svg/);
    }
});

test("QingYu runtime identity is isolated from SiYuan", async () => {
    const [mainSource, windowSource, bootSource, packageSource, manifestSource] = await Promise.all([
        readRepositoryFile("app/electron/main.js"),
        readRepositoryFile("app/electron/window.js"),
        readRepositoryFile("app/electron/boot.html"),
        readRepositoryFile("app/package.json"),
        readRepositoryFile("app/stage/manifest.webmanifest"),
    ]).then((files) => files.map((file) => file.toString()));

    assert.match(mainSource, /\.config", "qingyu"/);
    assert.match(mainSource, /QingYu-Electron/);
    assert.match(mainSource, /com\.apkdv\.qingyu/);
    assert.match(mainSource, /轻语 · 明窗净几，字字轻语/);
    assert.doesNotMatch(mainSource, /Refactor your thinking/);
    assert.match(mainSource, /let kernelPort = 9806/);
    assert.match(mainSource, /QingYu-Kernel/);
    assert.match(mainSource, /setAsDefaultProtocolClient\("qingyu"/);
    assert.doesNotMatch(mainSource, /\.config", "siyuan"/);
    assert.doesNotMatch(mainSource, /setAsDefaultProtocolClient\("siyuan"/);
    assert.match(windowSource, /Application Support", "QingYu"/);
    assert.match(bootSource, /getSearch\('port'\) \|\| '9806'/);

    const packageJSON = JSON.parse(packageSource) as {name: string; desktopName: string};
    assert.equal(packageJSON.name, "QingYu");
    assert.equal(packageJSON.desktopName, "com.apkdv.qingyu");
    const manifestJSON = JSON.parse(manifestSource) as {
        name: string;
        short_name: string;
        related_applications?: unknown[];
        protocol_handlers: Array<{protocol: string}>;
    };
    assert.equal(manifestJSON.name, "QingYu");
    assert.equal(manifestJSON.short_name, "qingyu");
    assert.deepEqual(manifestJSON.related_applications ?? [], []);
    assert.deepEqual(manifestJSON.protocol_handlers, [{protocol: "web+qingyu", url: "/?url=%s"}]);
});

test("QingYu block links are generated with the new protocol and accept legacy links", async () => {
    const pathNameSource = (await readRepositoryFile("app/src/util/pathName.ts")).toString();
    assert.match(pathNameSource, /"qingyu:"/);
    assert.match(pathNameSource, /"web\+qingyu:"/);
    assert.match(pathNameSource, /"siyuan:"/);
    assert.match(pathNameSource, /"web\+siyuan:"/);

    const producerPaths = [
        "app/src/protyle/render/av/action.ts",
        "app/src/protyle/toolbar/util.ts",
        "app/src/boot/globalEvent/searchKeydown.ts",
    ];
    const producerSource = (await Promise.all(producerPaths.map(readRepositoryFile)))
        .map((file) => file.toString()).join("\n");
    assert.match(producerSource, /qingyu:\/\/blocks\//);
    assert.doesNotMatch(producerSource, /siyuan:\/\/blocks\//);
});

test("QingYu packaging launches the renamed kernel", async () => {
    const builderPaths = [
        "app/electron-builder.yml",
        "app/electron-builder-arm64.yml",
        "app/electron-builder-darwin.yml",
        "app/electron-builder-darwin-arm64.yml",
        "app/electron-builder-linux.yml",
        "app/electron-builder-linux-arm64.yml",
    ];
    const builderSources = await Promise.all(builderPaths.map(async (path) => ({
        path,
        source: (await readRepositoryFile(path)).toString(),
    })));
    for (const {path, source} of builderSources) {
        assert.match(source, /productName: "QingYu"/, path);
        assert.match(source, /appId: "com\.apkdv\.qingyu"/, path);
        assert.match(source, /artifactName: "qingyu-\$\{version}-\$\{os}/, path);
        assert.doesNotMatch(source, /SiYuan-Kernel/, path);
    }

    const buildPaths = [
        "scripts/darwin-build.sh",
        "scripts/linux-build.sh",
        "scripts/win-build.bat",
        ".github/workflows/cd.yml",
        "app/nsis/installer.nsh",
    ];
    const buildSource = (await Promise.all(buildPaths.map(readRepositoryFile)))
        .map((file) => file.toString()).join("\n");
    assert.match(buildSource, /QingYu-Kernel/);
    assert.doesNotMatch(buildSource, /SiYuan-Kernel/);
});

test("QingYu application updater cannot use SiYuan release services", async () => {
    const updaterSource = (await readRepositoryFile("kernel/model/updater.go")).toString();
    assert.doesNotMatch(updaterSource, /util\.GetRhyResult/);
    assert.doesNotMatch(updaterSource, /"siyuan-" \+ ver/);
    assert.doesNotMatch(updaterSource, /github\.com\/siyuan-note\/siyuan\/releases/);
    assert.doesNotMatch(updaterSource, /release\.(?:b3log|liuyun)\.(?:org|io)/);
});

test("QingYu brand voice is consistent across application and README surfaces", async () => {
    const slogans = {
        ar: "نوافذ مضيئة، مكتب هادئ، وكلمات تُقال برفق",
        de: "Helle Fenster, ein stiller Schreibtisch, sanft gesetzte Worte.",
        en: "Clear windows, a quiet desk, words softly spoken.",
        es: "Ventanas luminosas, un escritorio sereno, palabras dichas en voz baja.",
        fr: "Fenêtres claires, bureau paisible, mots murmurés.",
        he: "חלונות מוארים, שולחן שקט, מילים הנאמרות ברכות.",
        hi: "उजली खिड़कियाँ, शांत मेज़, हर शब्द धीमे से कहा गया।",
        id: "Jendela terang, meja tenang, kata-kata yang terucap lembut.",
        it: "Finestre luminose, una scrivania quieta, parole sussurrate.",
        ja: "明るい窓、静かな机、言葉はそっと。",
        ko: "밝은 창, 고요한 책상, 나직이 놓이는 말.",
        nl: "Heldere ramen, een rustige schrijftafel, zacht gesproken woorden.",
        pl: "Jasne okna, spokojne biurko, słowa wypowiadane cicho.",
        "pt-BR": "Janelas claras, uma mesa serena, palavras ditas suavemente.",
        ru: "Светлые окна, тихий стол, слова, сказанные вполголоса.",
        sk: "Svetlé okná, pokojný stôl, slová vyslovené potichu.",
        th: "หน้าต่างสว่าง โต๊ะสงบ ถ้อยคำแผ่วเบา",
        tr: "Aydınlık pencereler, sakin bir masa, usulca söylenen sözler.",
        uk: "Світлі вікна, тихий стіл, слова, сказані пошепки.",
        "zh-CN": "明窗净几，字字轻语",
        "zh-TW": "明窗淨几，字字輕語",
    } as const;

    const electronWindowSource = (await readRepositoryFile("app/electron/window.js")).toString();
    for (const [locale, slogan] of Object.entries(slogans)) {
        const languageSource = (await readRepositoryFile(`app/appearance/langs/${locale}.json`)).toString();
        const language = JSON.parse(languageSource) as {
            slogan: string;
        };
        assert.equal(language.slogan, slogan, locale);
        assert.ok(electronWindowSource.includes(`slogan: ${JSON.stringify(slogan)}`), locale);
        assert.doesNotMatch(languageSource, /Yunnan Liandi Technology|云南链滴科技|雲南鏈滴科技/, locale);
        assert.doesNotMatch(languageSource, /"about1"|"accountSupport1"|"accountSupport2"/, locale);
    }

    const [aboutSource, readmeEn, readmeZhCN, readmeJa, readmeTr, manifestSource] = await Promise.all([
        readRepositoryFile("app/src/config/tabs/aboutTab.ts"),
        readRepositoryFile("README.md"),
        readRepositoryFile("README.zh-CN.md"),
        readRepositoryFile("README.ja.md"),
        readRepositoryFile("README.tr.md"),
        readRepositoryFile("app/stage/manifest.webmanifest"),
    ]).then((files) => files.map((file) => file.toString()));

    assert.match(aboutSource, /config-about__separator/);
    assert.doesNotMatch(aboutSource, /会泽百家 至公天下/);
    assert.doesNotMatch(aboutSource, /languages\.about1|languages\.accountSupport|sponsorBtn|getCloudURL|SIYUAN_IMAGE_SPONSOR/);
    assert.match(readmeEn, /<em>QingYu · Sunlit windows, an uncluttered desk, words in a gentle voice\.<\/em>/);
    assert.match(readmeZhCN, /<em>轻语 · 明窗净几，字字轻语<\/em>/);
    assert.match(readmeJa, /<em>QingYu · 光さす窓辺、整えた机。言葉はそっと息づく。<\/em>/);
    assert.match(readmeTr, /<em>QingYu · Gün ışığı alan bir pencere, derli toplu bir masa; sözcükler usulca dile gelir\.<\/em>/);

    const productReadmes = [readmeEn, readmeZhCN, readmeJa, readmeTr];
    for (const readme of productReadmes) {
        assert.match(readme, /src="logo\.png" width="128"/);
        assert.match(readme, /https:\/\/github\.com\/siyuan-note\/siyuan/);
        assert.match(readme, /AGPL-3\.0/);
        assert.doesNotMatch(readme, /b3logfile\.com|hub\.docker\.com\/r\/b3log\/siyuan/);
        assert.doesNotMatch(readme, /apps\.apple\.com|play\.google\.com|b3log\.org\/siyuan\/.*pricing/);
        assert.doesNotMatch(readme, /OpenAI|Architecture and Ecosystem|架构和生态|アーキテクチャとエコシステム|Mimari ve Ekosistem/);
    }
    assert.match(readmeZhCN, /不是思源笔记官方发行版/);
    assert.match(readmeEn, /not an official SiYuan release/);
    assert.match(readmeJa, /SiYuanの公式ディストリビューションではありません/);
    assert.match(readmeTr, /resmî bir SiYuan dağıtımı değildir/);

    const packageMetadata = JSON.parse((await readRepositoryFile("app/package.json")).toString()) as {
        homepage?: string;
        author?: {name?: string, email?: string};
    };
    assert.equal(packageMetadata.homepage, "https://apkdv.com/");
    assert.equal(packageMetadata.author?.name, "appdev");
    assert.equal(packageMetadata.author?.email, "lengyue@apkdv.com");

    const manifest = JSON.parse(manifestSource) as {description: string};
    assert.match(manifest.description, /privacy-first space for writing and thinking/);

    const packageMetadataPaths = [
        "app/package.json",
        "app/appx/AppxManifest.xml",
        "app/appx/AppxManifest-arm64.xml",
        "app/electron-builder-linux.yml",
        "app/electron-builder-linux-arm64.yml",
    ];
    for (const path of packageMetadataPaths) {
        const source = (await readRepositoryFile(path)).toString();
        assert.match(source, /QingYu · Clear windows, a quiet desk, words softly spoken\./, path);
        assert.doesNotMatch(source, /Refactor your thinking/, path);
        assert.doesNotMatch(source, /Yunnan Liandi Technology|云南链滴科技|雲南鏈滴科技/, path);
        if (path.includes("AppxManifest")) {
            assert.match(source, /<PublisherDisplayName>QingYu<\/PublisherDisplayName>/, path);
        }
    }
});
