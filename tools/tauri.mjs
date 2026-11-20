import cli from "@tauri-apps/cli";
import { readFileSync } from "node:fs";
import { prepareDebPackaging } from "./deb-package.mjs";

const args = process.argv.slice(2);
const separator = args.indexOf("--");
const options = args.slice(0, separator === -1 ? args.length : separator);
const cargoArgs = separator === -1 ? [] : args.slice(separator + 1);
const ios = options[0] === "ios";
const command = options[ios ? 1 : 0];
const debug =
  (command === "dev" && !options.includes("--release")) ||
  (ios && command === "run" && !options.includes("--release") && !options.includes("-r")) ||
  (["build", "bundle"].includes(command) && (options.includes("--debug") || options.includes("-d")));

try {
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const identifier = debug ? config.identifier + ".debug" : config.identifier;
  // Android Gradle sets its suffix separately from the namespace used by JNI.
  if (debug) args.splice(ios ? 2 : 1, 0, "--config", JSON.stringify({ identifier }));

  const linuxBundle = process.platform === "linux" && !ios && ["build", "bundle"].includes(command) &&
    !options.some(option => ["--no-bundle", "-h", "--help", "-V", "--version"].includes(option));
  const finishDeb = linuxBundle ? prepareDebPackaging(options, cargoArgs, debug) : undefined;
  await cli.run(args, "npm run tauri");
  finishDeb?.(identifier);
} catch (error) {
  cli.logError(error.message);
  process.exitCode = 1;
}
