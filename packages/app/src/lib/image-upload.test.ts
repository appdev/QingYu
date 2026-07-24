import { defaultEditorPreferences } from "./settings/app-settings";
import { createImageUploadFileName, saveLocalEditorImage } from "./image-upload";

describe("save editor image", () => {
  it("creates safe image file names from the configured pattern", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "My Diagram!.png", { type: "image/png" });

    expect(await createImageUploadFileName(image, "{name}-{timestamp}-{random}", {
      random: () => "abc123",
      timestamp: () => "1700000000000"
    })).toBe("My-Diagram-1700000000000-abc123.png");
  });

  it("uses the image content md5 in configured file naming patterns", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Diagram.png", { type: "image/png" });

    expect(await createImageUploadFileName(image, "{name}-{md5}")).toBe(
      "Diagram-5289df737df57326fcdd22597afb1fac.png"
    );
  });

  it("keeps dots that are part of the configured file naming pattern", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "My Diagram.png", { type: "image/png" });

    expect(await createImageUploadFileName(image, "{name}.{timestamp}", {
      timestamp: () => "1700000000000"
    })).toBe("My-Diagram.1700000000000.png");
  });

  it("keeps SVG image extensions when creating local file names", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Logo.svg", { type: "image/svg+xml" });

    expect(await createImageUploadFileName(image, "{name}-{timestamp}", {
      timestamp: () => "1700000000000"
    })).toBe("Logo-1700000000000.svg");
  });

  it("uses the local clipboard image folder and refreshes the file tree", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Screenshot.png", { type: "image/png" });
    const saveLocalImage = vi.fn().mockResolvedValue({ alt: "Screenshot", src: "assets/pasted-image.png" });

    await expect(saveLocalEditorImage({
      documentPath: "/mock-files/note.md",
      image,
      preferences: defaultEditorPreferences,
      saveLocalImage
    })).resolves.toEqual({
      image: { alt: "Screenshot", src: "assets/pasted-image.png" },
      refreshTree: true,
      status: "saved"
    });

    expect(saveLocalImage).toHaveBeenCalledWith({
      documentPath: "/mock-files/note.md",
      fileName: expect.stringMatching(/^pasted-image-\d+\.png$/u),
      folder: "assets",
      image
    });
  });

  it("stores dropped images in the fixed primary workspace assets directory", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Local Diagram.png", { type: "image/png" });
    const saveLocalImage = vi.fn().mockResolvedValue({ alt: "Local Diagram", src: "../assets/diagram.png" });

    await expect(saveLocalEditorImage({
      context: { mode: "primary-workspace", primaryRootPath: "/mock-vault" },
      documentPath: "/mock-vault/notes/day.md",
      image,
      origin: "drop",
      preferences: defaultEditorPreferences,
      saveLocalImage
    })).resolves.toMatchObject({ status: "saved", refreshTree: true });

    expect(saveLocalImage).toHaveBeenCalledWith({
      documentPath: "/mock-vault/notes/day.md",
      fileName: expect.stringMatching(/^pasted-image-\d+\.png$/u),
      folder: "assets",
      image,
      projectRootPath: "/mock-vault"
    });
  });

  it("stores imported images in the fixed primary workspace assets directory", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Camera.png", { type: "image/png" });
    const saveLocalImage = vi.fn().mockResolvedValue({ alt: "Camera", src: "../assets/camera.png" });

    await expect(saveLocalEditorImage({
      context: { mode: "primary-workspace", primaryRootPath: "/mobile/workspace" },
      documentPath: "/mobile/workspace/notes/day.md",
      image,
      origin: "import",
      preferences: defaultEditorPreferences,
      saveLocalImage
    })).resolves.toMatchObject({ status: "saved", refreshTree: true });

    expect(saveLocalImage).toHaveBeenCalledWith({
      documentPath: "/mobile/workspace/notes/day.md",
      fileName: expect.stringMatching(/^pasted-image-\d+\.png$/u),
      folder: "assets",
      image,
      projectRootPath: "/mobile/workspace"
    });
  });

  it("references standalone dropped images without copying", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Local Diagram.png", { type: "image/png" });
    const saveLocalImage = vi.fn().mockResolvedValue({ alt: "Local Diagram", src: "file:///mock/Local%20Diagram.png" });

    await expect(saveLocalEditorImage({
      context: { mode: "standalone" },
      documentPath: null,
      image,
      origin: "drop",
      preferences: defaultEditorPreferences,
      saveLocalImage
    })).resolves.toEqual({
      image: { alt: "Local Diagram", src: "file:///mock/Local%20Diagram.png" },
      refreshTree: false,
      status: "saved"
    });

    expect(saveLocalImage).toHaveBeenCalledWith({
      copyToStorage: false,
      documentPath: null,
      fileName: expect.stringMatching(/^pasted-image-\d+\.png$/u),
      folder: "assets",
      image
    });
  });

  it("references an existing standalone imported image without copying", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Existing.png", { type: "image/png" });
    Object.defineProperty(image, "path", { value: "/External/Existing.png" });
    const saveLocalImage = vi.fn().mockResolvedValue({
      alt: "Existing",
      src: "file:///External/Existing.png"
    });

    await expect(saveLocalEditorImage({
      context: { mode: "standalone" },
      documentPath: "/External/note.md",
      image,
      origin: "import",
      preferences: defaultEditorPreferences,
      saveLocalImage
    })).resolves.toMatchObject({ refreshTree: false, status: "saved" });

    expect(saveLocalImage).toHaveBeenCalledWith(expect.objectContaining({
      copyToStorage: false,
      documentPath: "/External/note.md",
      image
    }));
  });

  it("requires standalone clipboard bytes to have a saved document", async () => {
    const image = new File([new Uint8Array([1, 2, 3])], "Clipboard.png", { type: "image/png" });
    const saveLocalImage = vi.fn();

    await expect(saveLocalEditorImage({
      context: { mode: "standalone" },
      documentPath: null,
      image,
      origin: "clipboard",
      preferences: defaultEditorPreferences,
      saveLocalImage
    })).resolves.toEqual({
      reason: "requires-saved-document",
      status: "skipped"
    });
    expect(saveLocalImage).not.toHaveBeenCalled();
  });
});
