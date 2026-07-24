import { spawnSync } from "node:child_process";

import { bumppFilePaths } from "../../bump.config.mjs";

const commandArgs = [
  "node_modules/bumpp/bin/bumpp.mjs",
  ...process.argv.slice(2),
  ...bumppFilePaths,
  "--configFilePath",
  "bump.config.mjs",
];

const result = spawnSync("node", commandArgs, {
  stdio: "inherit",
});

if (typeof result.status === "number") {
  process.exit(result.status);
}

process.exit(1);
