import { createMarkraAppViteConfig } from "@markra/scripts/vite";

export default createMarkraAppViteConfig({
  browserNodeStubUrl: new URL("./src/lib/browser-node-stub.ts", import.meta.url),
  packageJsonUrl: new URL("./package.json", import.meta.url),
  stripDebug: false,
  test: {
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: "./src/test/setup.ts"
  }
});
