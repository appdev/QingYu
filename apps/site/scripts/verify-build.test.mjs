import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { verifySiteBuild } from "./verify-build.mjs";

const principleLines = [
  "写作，不必先搭一套系统。",
  "所见即所得与源码，只是同一份 Markdown 的两面。",
  "文件夹盛放篇章，图片与链接保持原来的模样。",
  "同步可以抵达远方，但只跟随你当前选择的笔记目录。",
  "工具退后一步，文字便向前一步。"
];

const chineseTitle = "轻语 QingYu｜开源 Markdown 编辑器";
const primaryDescription = "轻语是一款无需账号的开源 Markdown 编辑器，支持所见即所得、源码编辑、可切换笔记目录，以及 WebDAV / S3 当前笔记目录同步。";
const coreProductCopy = "打开一份 Markdown，文字便自然铺开。所见即所得与源码模式，写的是同一份文件；无需账号，也不把笔记困在云端。";

const validHtml = `<!doctype html>
<html lang="zh-CN">
  <head>
    <meta name="description" content="${primaryDescription}">
    <meta name="theme-color" content="#f4efe4">
    <link rel="icon" href="/qingyu-logo.png" type="image/png">
    <link rel="canonical" href="/" data-site-origin>
    <meta property="og:type" content="website">
    <meta property="og:title" content="${chineseTitle}">
    <meta property="og:description" content="${primaryDescription}">
    <meta property="og:url" content="/" data-site-origin>
    <meta property="og:image" content="/og-image.png">
    <meta name="twitter:card" content="summary_large_image">
    <meta name="twitter:title" content="${chineseTitle}">
    <meta name="twitter:description" content="${primaryDescription}">
    <meta name="twitter:image" content="/og-image.png">
    <link rel="preload" href="/fonts/qingyu-wenkai-subset.woff2" as="font" type="font/woff2" crossorigin>
    <script type="application/ld+json">{"@context":"https://schema.org","@type":"SoftwareApplication","name":"轻语 QingYu"}</script>
    <title>${chineseTitle}</title>
  </head>
  <body>
    <div id="root">
      <main>
        <h1>明窗净几，字字轻语。</h1>
        <p>${coreProductCopy}</p>
        <ol>${principleLines.map((line) => `<li>${line}</li>`).join("")}</ol>
      </main>
    </div>
  </body>
</html>`;

async function createBuildFixture(html = validHtml) {
  const directory = await mkdtemp(join(tmpdir(), "qingyu-site-build-"));
  await mkdir(join(directory, "assets"), { recursive: true });
  await mkdir(join(directory, "fonts"), { recursive: true });
  await writeFile(join(directory, "index.html"), html);
  await writeFile(join(directory, "assets", "index-abc123.js"), "client");
  await writeFile(join(directory, "fonts", "qingyu-wenkai-subset.woff2"), "font");
  await writeFile(join(directory, "og-image.png"), "og");
  await writeFile(join(directory, "qingyu-logo.png"), "logo");
  return directory;
}

async function withBuildFixture(run, html = validHtml) {
  const directory = await createBuildFixture(html);
  try {
    await run(directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
}

test("returns a stable sorted asset inventory for a valid build", async () => {
  await withBuildFixture(async (directory) => {
    const result = await verifySiteBuild(directory);
    assert.deepEqual(result.assets, [
      { path: "assets/index-abc123.js", bytes: 6 },
      { path: "fonts/qingyu-wenkai-subset.woff2", bytes: 4 },
      { path: "index.html", bytes: Buffer.byteLength(validHtml) },
      { path: "og-image.png", bytes: 2 },
      { path: "qingyu-logo.png", bytes: 4 }
    ]);
  });
});

test("rejects a build missing the Chinese heading", async () => {
  await withBuildFixture(async (directory) => {
    await assert.rejects(
      verifySiteBuild(directory),
      /Chinese h1/u
    );
  }, validHtml.replace("<h1>明窗净几，字字轻语。</h1>", "<h1>QingYu</h1>"));
});

test("rejects a build missing the core Chinese product copy", async () => {
  await withBuildFixture(async (directory) => {
    await assert.rejects(
      verifySiteBuild(directory),
      /core product copy/u
    );
  }, validHtml.replace(coreProductCopy, "QingYu"));
});

test("rejects a missing or wrong Chinese HTML title", async (context) => {
  const title = `<title>${chineseTitle}</title>`;
  const cases = [
    ["missing", ""],
    ["wrong", "<title>QingYu</title>"]
  ];

  for (const [label, replacement] of cases) {
    await context.test(label, async () => {
      await withBuildFixture(async (directory) => {
        await assert.rejects(verifySiteBuild(directory), /Chinese HTML title/u);
      }, validHtml.replace(title, replacement));
    });
  }
});

test("rejects a missing or wrong primary description", async (context) => {
  const description = `<meta name="description" content="${primaryDescription}">`;
  const cases = [
    ["missing", ""],
    ["wrong", '<meta name="description" content="QingYu">']
  ];

  for (const [label, replacement] of cases) {
    await context.test(label, async () => {
      await withBuildFixture(async (directory) => {
        await assert.rejects(verifySiteBuild(directory), /primary description/u);
      }, validHtml.replace(description, replacement));
    });
  }
});

test("rejects every missing product principle", async (context) => {
  for (const line of principleLines) {
    await context.test(line, async () => {
      await withBuildFixture(async (directory) => {
        await assert.rejects(
          verifySiteBuild(directory),
          /product principle/u
        );
      }, validHtml.replace(line, ""));
    });
  }
});

test("rejects missing required SEO markup", async (context) => {
  const cases = [
    ["favicon", '<link rel="icon" href="/qingyu-logo.png" type="image/png">'],
    ["theme color", '<meta name="theme-color" content="#f4efe4">'],
    ["Open Graph", '<meta property="og:type" content="website">'],
    ["Twitter Card", '<meta name="twitter:card" content="summary_large_image">'],
    ["SoftwareApplication JSON-LD", '<script type="application/ld+json">{"@context":"https://schema.org","@type":"SoftwareApplication","name":"轻语 QingYu"}</script>']
  ];

  for (const [label, markup] of cases) {
    await context.test(label, async () => {
      await withBuildFixture(async (directory) => {
        await assert.rejects(verifySiteBuild(directory), new RegExp(label, "u"));
      }, validHtml.replace(markup, ""));
    });
  }
});

test("rejects absolute or unmarked site-origin metadata", async (context) => {
  const cases = [
    ["canonical", '<link rel="canonical" href="/" data-site-origin>', '<link rel="canonical" href="https://example.com/">'],
    ["og:url", '<meta property="og:url" content="/" data-site-origin>', '<meta property="og:url" content="https://example.com/">']
  ];

  for (const [label, valid, invalid] of cases) {
    await context.test(label, async () => {
      await withBuildFixture(async (directory) => {
        await assert.rejects(verifySiteBuild(directory), new RegExp(label, "u"));
      }, validHtml.replace(valid, invalid));
    });
  }
});

test("rejects a missing or duplicate WOFF2 preload", async (context) => {
  const preload = '<link rel="preload" href="/fonts/qingyu-wenkai-subset.woff2" as="font" type="font/woff2" crossorigin>';

  await context.test("missing", async () => {
    await withBuildFixture(async (directory) => {
      await assert.rejects(verifySiteBuild(directory), /exactly one WOFF2 preload/u);
    }, validHtml.replace(preload, ""));
  });

  await context.test("duplicate", async () => {
    await withBuildFixture(async (directory) => {
      await assert.rejects(verifySiteBuild(directory), /exactly one WOFF2 preload/u);
    }, validHtml.replace(preload, `${preload}${preload}`));
  });
});

test("rejects forbidden font formats", async (context) => {
  for (const extension of ["ttf", "otf", "woff"]) {
    await context.test(extension, async () => {
      await withBuildFixture(async (directory) => {
        await writeFile(join(directory, "fonts", `forbidden.${extension}`), "font");
        await assert.rejects(verifySiteBuild(directory), /forbidden font format/u);
      });
    });
  }
});

test("rejects multiple WOFF2 font files", async () => {
  await withBuildFixture(async (directory) => {
    await writeFile(join(directory, "fonts", "extra.woff2"), "font");
    await assert.rejects(verifySiteBuild(directory), /exactly one WOFF2 file/u);
  });
});

test("rejects forbidden application chunk families", async (context) => {
  for (const family of [
    "milkdown",
    "codemirror",
    "tauri",
    "mermaid",
    "katex",
    "code-editor-vendor",
    "markdown-source-editor-vendor",
    "diagram-vendor",
    "math-vendor"
  ]) {
    await context.test(family, async () => {
      await withBuildFixture(async (directory) => {
        await writeFile(join(directory, "assets", `${family}-abc123.js`), "chunk");
        await assert.rejects(verifySiteBuild(directory), /forbidden chunk family/u);
      });
    });
  }
});

test("rejects a missing Open Graph image asset", async () => {
  await withBuildFixture(async (directory) => {
    await rm(join(directory, "og-image.png"));
    await assert.rejects(verifySiteBuild(directory), /og-image\.png/u);
  });
});

test("rejects an Open Graph image reference without an emitted asset", async () => {
  await withBuildFixture(async (directory) => {
    await assert.rejects(verifySiteBuild(directory), /og:image.*emitted asset/u);
  }, validHtml.replace(
    '<meta property="og:image" content="/og-image.png">',
    '<meta property="og:image" content="/missing-og-image.png">'
  ));
});

test("rejects a WOFF2 preload reference without an emitted asset", async () => {
  await withBuildFixture(async (directory) => {
    await assert.rejects(verifySiteBuild(directory), /WOFF2 preload.*emitted asset/u);
  }, validHtml.replace(
    'href="/fonts/qingyu-wenkai-subset.woff2"',
    'href="/fonts/missing.woff2"'
  ));
});

test("rejects mobile store links", async (context) => {
  for (const url of [
    "https://apps.apple.com/app/qingyu/id123456789",
    "https://play.google.com/store/apps/details?id=app.markra"
  ]) {
    await context.test(url, async () => {
      await withBuildFixture(async (directory) => {
        await assert.rejects(verifySiteBuild(directory), /mobile store link/u);
      }, validHtml.replace("</body>", `<a href="${url}">Store</a></body>`));
    });
  }
});
