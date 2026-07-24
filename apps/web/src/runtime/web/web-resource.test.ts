import { FakeIndexedDbFactory } from "../../test/web-runtime-fakes";
import { createWebRuntime } from "..";

describe("web image runtime", () => {
  it("uses fetch for ordinary web image downloads", async () => {
    const fetch = vi.fn(async () => new Response(new Uint8Array([1, 2, 3]), {
      headers: { "content-type": "image/png" }
    }));
    const runtime = createWebRuntime({
      fetch,
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    const image = await runtime.webResource.downloadImage({
      src: "https://example.test/image.png"
    });

    expect(image).toBeInstanceOf(File);
    expect(image.name).toBe("image.png");
    expect(image.type).toBe("image/png");
    await expect(image.arrayBuffer()).resolves.toEqual(new Uint8Array([1, 2, 3]).buffer);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("rejects unsuccessful web image responses", async () => {
    const fetch = vi.fn(async () => new Response("missing", {
      status: 404,
      headers: { "content-type": "image/png" }
    }));
    const runtime = createWebRuntime({
      fetch,
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    await expect(runtime.webResource.downloadImage({
      src: "https://example.test/missing.png"
    })).rejects.toThrow("Web image download failed with HTTP 404.");
  });

  it("rejects web responses whose content type is not an image", async () => {
    const fetch = vi.fn(async () => new Response("not an image", {
      headers: { "content-type": "text/plain; charset=utf-8" }
    }));
    const runtime = createWebRuntime({
      fetch,
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    await expect(runtime.webResource.downloadImage({
      src: "https://example.test/readme.txt"
    })).rejects.toThrow("Downloaded web content is not an image.");
  });

  it("rejects web images whose declared size exceeds 25 MiB", async () => {
    const fetch = vi.fn(async () => new Response(new Uint8Array([1]), {
      headers: {
        "content-length": String(25 * 1024 * 1024 + 1),
        "content-type": "image/png"
      }
    }));
    const runtime = createWebRuntime({
      fetch,
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    await expect(runtime.webResource.downloadImage({
      src: "https://example.test/oversized.png"
    })).rejects.toThrow("Web image is too large to paste into the document.");
  });

  it("rejects streamed web images whose actual size exceeds 25 MiB", async () => {
    const chunk = new Uint8Array(13 * 1024 * 1024);
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(chunk);
        controller.enqueue(chunk);
        controller.close();
      }
    });
    const fetch = vi.fn(async () => new Response(body, {
      headers: { "content-type": "image/webp" }
    }));
    const runtime = createWebRuntime({
      fetch,
      indexedDB: new FakeIndexedDbFactory().indexedDB
    });

    await expect(runtime.webResource.downloadImage({
      src: "https://example.test/oversized.webp"
    })).rejects.toThrow("Web image is too large to paste into the document.");
  });
});
