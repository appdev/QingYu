import { access, readFile, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";

import openapiTS, { astToString } from "openapi-typescript";

const schemaUrl = new URL(
  "../../../apps/kernel/openapi/kernel-v1.json",
  import.meta.url,
);
const outputUrl = new URL("../src/generated/kernel-v1.ts", import.meta.url);
const generated = astToString(await openapiTS(schemaUrl));
const task = process.argv
  .find((argument) => argument.startsWith("--task="))
  ?.slice("--task=".length);

async function fileExists(url) {
  try {
    await access(url);
    return true;
  } catch {
    return false;
  }
}

async function runTask(name) {
  const buildConfigUrl = new URL("../tsconfig.build.json", import.meta.url);
  const testConfigUrl = new URL("../tsconfig.test.json", import.meta.url);
  const configUrl = name === "build" ? buildConfigUrl : testConfigUrl;
  if (!(await fileExists(configUrl))) {
    return;
  }

  const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";
  const args =
    name === "test"
      ? ["exec", "vitest", "run"]
      : [
          "exec",
          "tsc",
          "-p",
          name === "build" ? "tsconfig.build.json" : "tsconfig.test.json",
          "--noEmit",
        ];
  const exitCode = await new Promise((resolve, reject) => {
    const child = spawn(pnpm, args, { stdio: "inherit" });
    child.once("error", reject);
    child.once("close", resolve);
  });
  if (exitCode !== 0) {
    process.exitCode = typeof exitCode === "number" ? exitCode : 1;
  }
}

if (process.argv.includes("--write")) {
  await writeFile(outputUrl, generated, "utf8");
} else {
  const checkedIn = await readFile(outputUrl, "utf8");
  if (checkedIn !== generated) {
    console.error("kernel-v1.ts is stale; run pnpm --filter @markra/kernel-client generate");
    process.exitCode = 1;
  } else if (task) {
    await runTask(task);
  }
}
