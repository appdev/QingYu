import { describe, expect, it } from "vitest";
import { createEditorResourceRequest } from "./clipboard-images";

describe("editor resource requests", () => {
  it.each(["clipboard", "drop", "import"] as const)("keeps the %s file origin explicit", (origin) => {
    const file = new File([new Uint8Array([1])], "resource.png", { type: "image/png" });

    expect(createEditorResourceRequest(origin, [file])).toEqual({ files: [file], origin });
  });

  it("keeps remote URLs explicit without pretending they are local files", () => {
    expect(createEditorResourceRequest("remote", ["https://example.test/image.png"]))
      .toEqual({ origin: "remote", urls: ["https://example.test/image.png"] });
  });
});
