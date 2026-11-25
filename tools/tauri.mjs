import cli from "@tauri-apps/cli";
import { readFileSync } from "node:fs";
import { createConnection, createServer } from "node:net";
import { prepareDebPackaging } from "./deb-package.mjs";

const args = process.argv.slice(2);
const separator = args.indexOf("--");
const options = args.slice(0, separator === -1 ? args.length : separator);
const cargoArgs = separator === -1 ? [] : args.slice(separator + 1);
const ios = options[0] === "ios";
const androidDev = options[0] === "android" && options[1] === "dev";
const command = options[ios ? 1 : 0];
const debug =
  (command === "dev" && !options.includes("--release")) ||
  (ios && command === "run" && !options.includes("--release") && !options.includes("-r")) ||
  (["build", "bundle"].includes(command) && (options.includes("--debug") || options.includes("-d")));

async function availableDevPort(port) {
  try {
    // On macOS a wildcard listener can coexist with a loopback-only listener on the same port.
    const inUse = await new Promise(resolve => {
      const connection = createConnection({ host: "localhost", port, autoSelectFamily: true });
      connection.once("connect", () => { connection.destroy(); resolve(true); });
      connection.once("error", () => resolve(false));
    });
    if (inUse && port < 65535) return availableDevPort(port + 1);
    if (inUse) throw new Error("No available development port.");
    await new Promise((resolve, reject) => {
      const server = createServer();
      server.once("error", reject);
      server.listen(port, "0.0.0.0", () => server.close(resolve));
    });
    return port;
  } catch (error) {
    if (error.code === "EADDRINUSE" && port < 65535) return availableDevPort(port + 1);
    throw error;
  }
}

try {
  const config = JSON.parse(readFileSync(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const identifier = debug ? config.identifier + ".debug" : config.identifier;
  // Android Gradle sets its suffix separately from the namespace used by JNI.
  const override = debug ? { identifier } : {};
  if ((command === "dev" || androidDev) &&
    !options.some(option => /^(?:-h|--help|-V|--version|-c.*|--config(?:=.*)?)$/.test(option))) {
    const androidConfig = androidDev
      ? JSON.parse(readFileSync(new URL("../src-tauri/tauri.android.conf.json", import.meta.url), "utf8"))
      : undefined;
    const devUrl = new URL(androidConfig?.build?.devUrl ?? config.build.devUrl);
    const defaultPort = Number(devUrl.port);
    const port = await availableDevPort(defaultPort);
    if (port !== defaultPort) {
      devUrl.port = String(port);
      // shortcut: Vite keeps --strictPort if another process binds during handoff, revisit if concurrent starts need retry.
      override.build = { devUrl: devUrl.href, beforeDevCommand: `${config.build.beforeDevCommand} -- --port ${port}` };
      console.log(`Dev port ${defaultPort} is in use. Starting on port ${port}.`);
    }
  }
  if (Object.keys(override).length) args.splice(ios || androidDev ? 2 : 1, 0, "--config", JSON.stringify(override));

  const linuxBundle = process.platform === "linux" && !ios && ["build", "bundle"].includes(command) &&
    !options.some(option => ["--no-bundle", "-h", "--help", "-V", "--version"].includes(option));
  const finishDeb = linuxBundle ? prepareDebPackaging(options, cargoArgs, debug) : undefined;
  await cli.run(args, "npm run tauri");
  finishDeb?.(identifier);
} catch (error) {
  cli.logError(error.message);
  process.exitCode = 1;
}
