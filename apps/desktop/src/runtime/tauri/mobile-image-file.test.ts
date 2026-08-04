import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { vi } from "vitest";

import {
  createAndroidMobileLocalImagePicker,
  createMobileLocalImagePicker,
  createPlatformMobileLocalImagePicker,
  saveMobileClipboardImage
} from "./file/mobile";
import { mobileImageFileFromBytes } from "./mobile-image-file";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn()
}));
vi.mock("@tauri-apps/plugin-fs", () => ({
  readFile: vi.fn()
}));
vi.mock("@tauri-apps/plugin-os", () => ({
  platform: vi.fn(() => "ios")
}));

const signatures = {
  avif: new Uint8Array([
    0x00, 0x00, 0x00, 0x18,
    0x66, 0x74, 0x79, 0x70,
    0x61, 0x76, 0x69, 0x66,
    0x00, 0x00, 0x00, 0x00,
    0x61, 0x76, 0x69, 0x66,
    0x6d, 0x69, 0x66, 0x31
  ]),
  bmp: new Uint8Array([0x42, 0x4d, 0x1a, 0x00, 0x00, 0x00]),
  gif: new TextEncoder().encode("GIF89a"),
  jpeg: new Uint8Array([0xff, 0xd8, 0xff, 0xdb]),
  png: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  svg: new TextEncoder().encode("\uFEFF  <?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"),
  webp: new Uint8Array([
    0x52, 0x49, 0x46, 0x46,
    0x04, 0x00, 0x00, 0x00,
    0x57, 0x45, 0x42, 0x50
  ])
} as const;

describe("mobile image files", () => {
  it.each([
    ["png", "image/png", "png"],
    ["jpeg", "image/jpeg", "jpg"],
    ["gif", "image/gif", "gif"],
    ["webp", "image/webp", "webp"],
    ["bmp", "image/bmp", "bmp"],
    ["avif", "image/avif", "avif"],
    ["svg", "image/svg+xml", "svg"]
  ] as const)("recognizes %s bytes authoritatively", (signature, mimeType, extension) => {
    const file = mobileImageFileFromBytes({
      bytes: signatures[signature],
      uri: "content://media/picked-image.bin"
    });

    expect(file).toMatchObject({
      name: `picked-image.${extension}`,
      type: mimeType
    });
  });

  it.each([
    ["file:///private/var/mobile/Local%20Diagram.PNG", "Local Diagram.png"],
    ["content://media/images/Encoded%20Photo.jpeg", "Encoded Photo.png"],
    ["content://provider/item/42?displayName=Camera%20Shot.JPG", "Camera Shot.png"]
  ])("recovers a safe name from %s", (uri, expectedName) => {
    expect(mobileImageFileFromBytes({ bytes: signatures.png, uri }).name).toBe(expectedName);
  });

  it.each([
    "content://media/42",
    "content://media/%2E%2E%2Fescape.png",
    "not a URI"
  ])("falls back when the selected URI has no safe image name", (uri) => {
    expect(mobileImageFileFromBytes({ bytes: signatures.webp, uri }).name).toBe("picked-image.webp");
  });

  it("replaces a disagreeing URI extension with the detected extension", () => {
    expect(mobileImageFileFromBytes({
      bytes: signatures.jpeg,
      uri: "content://media/not-really-a-document.pdf"
    })).toMatchObject({ name: "not-really-a-document.jpg", type: "image/jpeg" });
  });

  it.each([
    ["empty", new Uint8Array()],
    ["PDF", new TextEncoder().encode("%PDF-1.7")],
    ["plain text", new TextEncoder().encode("ordinary text")]
  ])("rejects %s data", (_label, bytes) => {
    expect(() => mobileImageFileFromBytes({ bytes, uri: "content://media/file.bin" }))
      .toThrow(/supported image/i);
  });

  it("rejects AVIF image sequences outside the still-image contract", () => {
    const sequence = new Uint8Array(signatures.avif);
    sequence.set(new TextEncoder().encode("avis"), 8);
    sequence.set(new TextEncoder().encode("avis"), 16);

    expect(() => mobileImageFileFromBytes({ bytes: sequence, uri: "content://media/sequence.avif" }))
      .toThrow(/supported image/i);
  });
});

describe("mobile image picker", () => {
  const mockedInvoke = vi.mocked(invoke);
  const mockedOpen = vi.mocked(open);
  const mockedReadFile = vi.mocked(readFile);

  beforeEach(() => {
    mockedInvoke.mockReset();
    mockedOpen.mockReset();
    mockedReadFile.mockReset();
  });

  it("uses the scoped system image picker and reads returned URIs directly", async () => {
    const uris = [
      "content://media/images/42",
      "file:///private/var/mobile/Containers/Data/photo.jpeg"
    ];
    const open = vi.fn().mockResolvedValue(uris);
    const readFile = vi.fn()
      .mockResolvedValueOnce(signatures.png)
      .mockResolvedValueOnce(signatures.jpeg);
    const pickImages = createMobileLocalImagePicker({ open, readFile });

    const files = await pickImages({ title: "Import images" });

    expect(open).toHaveBeenCalledWith({
      fileAccessMode: "scoped",
      filters: [{
        extensions: [
          "image/avif",
          "image/bmp",
          "image/gif",
          "image/jpeg",
          "image/png",
          "image/svg+xml",
          "image/webp"
        ],
        name: "Images"
      }],
      multiple: true,
      pickerMode: "image",
      title: "Import images"
    });
    expect(readFile).toHaveBeenNthCalledWith(1, uris[0]);
    expect(readFile).toHaveBeenNthCalledWith(2, uris[1]);
    expect(mockedInvoke).not.toHaveBeenCalled();
    expect(files.map(({ type }) => type)).toEqual(["image/png", "image/jpeg"]);
  });

  it("wakes the mobile event loop until the Android picker result is delivered", async () => {
    vi.useFakeTimers();
    const uri = "content://media/images/42?displayName=blocked-activity-result.png";
    let resolveOpen: (selection: string | string[] | null) => unknown = () => undefined;
    const open = vi.fn(() => new Promise<string | string[] | null>((resolve) => {
      resolveOpen = resolve;
    }));
    const readFile = vi.fn().mockResolvedValue(signatures.png);
    const wakeEventLoop = vi.fn(() => {
      resolveOpen(uri);
      return Promise.reject(new Error("the wake response may trail the picker result"));
    });
    const dependencies = { open, readFile, wakeEventLoop };
    const pickImages = createMobileLocalImagePicker(dependencies);
    const pendingSelection = pickImages();

    try {
      await vi.advanceTimersByTimeAsync(1_000);

      expect(wakeEventLoop).toHaveBeenCalled();
      const files = await pendingSelection;
      expect(files.map(({ name, type }) => ({ name, type }))).toEqual([{
        name: "blocked-activity-result.png",
        type: "image/png"
      }]);

      const settledWakeCount = wakeEventLoop.mock.calls.length;
      await vi.advanceTimersByTimeAsync(1_000);
      expect(wakeEventLoop).toHaveBeenCalledTimes(settledWakeCount);
    } finally {
      resolveOpen(null);
      await pendingSelection.catch(() => []);
      vi.useRealTimers();
    }
  });

  it("uses the dedicated Android picker command so dialog callback loss cannot strand the selection", async () => {
    const uri = "content://media/images/42?displayName=dedicated-picker.png";
    const pickUris = vi.fn().mockResolvedValue([uri]);
    const readFile = vi.fn().mockResolvedValue(signatures.png);
    const pickImages = createAndroidMobileLocalImagePicker({ pickUris, readFile });

    const files = await pickImages({ title: "Import images" });

    expect(pickUris).toHaveBeenCalledWith({ title: "Import images" });
    expect(readFile).toHaveBeenCalledWith(uri);
    expect(files.map(({ name, type }) => ({ name, type }))).toEqual([{
      name: "dedicated-picker.png",
      type: "image/png"
    }]);
  });

  it("routes Android platform image imports away from the shared dialog ActivityResult callback", async () => {
    const uri = "content://media/images/42?displayName=android-platform-route.png";
    mockedInvoke.mockResolvedValue({ uris: [uri] });
    mockedReadFile.mockResolvedValue(signatures.png);

    const files = await createPlatformMobileLocalImagePicker(() => "android")({ title: "Import images" });

    expect(mockedInvoke).toHaveBeenCalledWith("pick_mobile_images", { title: "Import images" });
    expect(mockedOpen).not.toHaveBeenCalled();
    expect(mockedReadFile).toHaveBeenCalledWith(uri);
    expect(files.map(({ name, type }) => ({ name, type }))).toEqual([{
      name: "android-platform-route.png",
      type: "image/png"
    }]);
  });

  it("keeps non-Android platform image imports on the dialog picker path", async () => {
    const uri = "content://media/images/42?displayName=ios-dialog-route.png";
    mockedOpen.mockResolvedValue(uri);
    mockedReadFile.mockResolvedValue(signatures.png);

    const files = await createPlatformMobileLocalImagePicker(() => "ios")({ title: "Import images" });

    expect(mockedOpen).toHaveBeenCalledWith(expect.objectContaining({
      fileAccessMode: "scoped",
      multiple: true,
      pickerMode: "image",
      title: "Import images"
    }));
    expect(mockedInvoke).not.toHaveBeenCalledWith("pick_mobile_images", expect.anything());
    expect(mockedReadFile).toHaveBeenCalledWith(uri);
    expect(files.map(({ name, type }) => ({ name, type }))).toEqual([{
      name: "ios-dialog-route.png",
      type: "image/png"
    }]);
  });

  it("completes repeated Android picker selections without sharing the previous callback slot", async () => {
    const uris = [
      "content://media/images/1?displayName=first-repeat.png",
      "content://media/images/2?displayName=second-repeat.webp"
    ];
    const pickUris = vi.fn()
      .mockResolvedValueOnce([uris[0]])
      .mockResolvedValueOnce([uris[1]]);
    const readFile = vi.fn()
      .mockResolvedValueOnce(signatures.png)
      .mockResolvedValueOnce(signatures.webp);
    const pickImages = createAndroidMobileLocalImagePicker({ pickUris, readFile });

    await expect(pickImages()).resolves.toMatchObject([{ name: "first-repeat.png", type: "image/png" }]);
    await expect(pickImages()).resolves.toMatchObject([{ name: "second-repeat.webp", type: "image/webp" }]);

    expect(pickUris).toHaveBeenCalledTimes(2);
    expect(readFile).toHaveBeenNthCalledWith(1, uris[0]);
    expect(readFile).toHaveBeenNthCalledWith(2, uris[1]);
  });

  it("models why Tauri's shared ActivityResult slot can leave the original dialog promise pending", () => {
    let callback: ((value: string) => unknown) | null = null;
    const first = vi.fn();
    const second = vi.fn();
    const launch = (next: (value: string) => unknown) => {
      callback = next;
    };
    const deliver = (value: string) => {
      const current = callback;
      if (current) current(value);
    };

    launch(first);
    launch(second);
    deliver("picked-uri");

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith("picked-uri");
  });

  it("keeps Android picker cancellation distinct from selected unreadable content", async () => {
    const pickUris = vi.fn().mockResolvedValue([]);
    const readFile = vi.fn();

    await expect(createAndroidMobileLocalImagePicker({ pickUris, readFile })()).resolves.toEqual([]);
    expect(readFile).not.toHaveBeenCalled();
  });

  it("reports unreadable Android picker content without treating it as cancellation", async () => {
    const pickUris = vi.fn().mockResolvedValue(["content://media/images/unreadable"]);
    const readFile = vi.fn().mockRejectedValue(new Error("permission denied"));

    await expect(createAndroidMobileLocalImagePicker({ pickUris, readFile })())
      .rejects.toThrow(/read selected image/i);
    expect(readFile).toHaveBeenCalledWith("content://media/images/unreadable");
  });

  it("returns an empty list when the user cancels", async () => {
    const open = vi.fn().mockResolvedValue(null);
    const readFile = vi.fn();

    await expect(createMobileLocalImagePicker({ open, readFile })()).resolves.toEqual([]);
    expect(readFile).not.toHaveBeenCalled();
    expect(mockedInvoke).not.toHaveBeenCalled();
  });

  it("reports selected image read failures actionably", async () => {
    const open = vi.fn().mockResolvedValue("content://media/images/unavailable");
    const readFile = vi.fn().mockRejectedValue(new Error("permission denied"));

    await expect(createMobileLocalImagePicker({ open, readFile })())
      .rejects.toThrow(/read selected image/i);
  });

  it("reports unsupported selected data actionably", async () => {
    const open = vi.fn().mockResolvedValue("content://media/documents/42");
    const readFile = vi.fn().mockResolvedValue(new TextEncoder().encode("not an image"));

    await expect(createMobileLocalImagePicker({ open, readFile })())
      .rejects.toThrow(/supported image/i);
  });
});

describe("mobile image persistence", () => {
  const mockedInvoke = vi.mocked(invoke);

  beforeEach(() => {
    mockedInvoke.mockReset();
  });

  it("saves validated bytes through the primary-workspace image command", async () => {
    mockedInvoke.mockResolvedValue({ relativePath: "../assets/Camera Shot.png" });
    const image = new File([new Uint8Array(signatures.png)], "Camera Shot.png", { type: "image/png" });

    await expect(saveMobileClipboardImage({
      documentPath: "/mobile/workspace/notes/note.md",
      fileName: "Camera Shot.png",
      folder: "ignored-on-mobile",
      image,
      projectRootPath: "/mobile/workspace"
    })).resolves.toEqual({
      alt: "Camera Shot",
      src: "../assets/Camera%20Shot.png"
    });

    expect(mockedInvoke).toHaveBeenCalledWith("save_clipboard_image", {
      bytes: Array.from(signatures.png),
      documentPath: "/mobile/workspace/notes/note.md",
      fileName: "Camera Shot.png",
      folder: "assets",
      mimeType: "image/png",
      projectRootPath: "/mobile/workspace"
    });
  });

  it("rejects image persistence without the primary workspace root", async () => {
    const image = new File([new Uint8Array(signatures.png)], "Camera Shot.png", { type: "image/png" });

    await expect(saveMobileClipboardImage({
      documentPath: "/mobile/workspace/note.md",
      fileName: "Camera Shot.png",
      folder: "assets",
      image
    })).rejects.toThrow(/primary workspace/i);
    expect(mockedInvoke).not.toHaveBeenCalled();
  });
});
