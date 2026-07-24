import type { AppWebResourceRuntime } from "@markra/app/runtime";
import type { WebRuntimeOptions } from "./types";

const WEB_IMAGE_MAX_BYTES = 25 * 1024 * 1024;

export function createWebResourceRuntime(options: WebRuntimeOptions): AppWebResourceRuntime {
  const fetcher = options.fetch ?? globalThis.fetch;

  return {
    async downloadImage({ src }) {
      const response = await fetcher(src);
      if (!response.ok) {
        throw new Error(`Web image download failed with HTTP ${response.status}.`);
      }

      const contentType = response.headers.get("content-type")
        ?.split(";", 1)[0]
        ?.trim()
        .toLowerCase();
      if (!contentType?.startsWith("image/")) {
        throw new Error("Downloaded web content is not an image.");
      }

      const declaredSize = Number(response.headers.get("content-length"));
      if (Number.isFinite(declaredSize) && declaredSize > WEB_IMAGE_MAX_BYTES) {
        throw new Error("Web image is too large to paste into the document.");
      }

      const blob = await response.blob();
      if (blob.size > WEB_IMAGE_MAX_BYTES) {
        throw new Error("Web image is too large to paste into the document.");
      }

      const path = new URL(src, "https://example.test").pathname;
      const fileName = path.split("/").filter(Boolean).pop() ?? "image";

      return new File([blob], fileName, {
        type: contentType
      });
    }
  };
}
