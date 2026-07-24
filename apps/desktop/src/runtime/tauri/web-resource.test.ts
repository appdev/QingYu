import { invoke } from "@tauri-apps/api/core";
import { downloadNativeWebImage } from "./web-resource";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

const mockedInvoke = vi.mocked(invoke);

describe("native web image runtime", () => {
  beforeEach(() => {
    mockedInvoke.mockReset();
  });

  it("downloads web images without application proxy data", async () => {
    mockedInvoke.mockResolvedValue({
      bytes: [1, 2, 3],
      fileName: "kitten.png",
      mimeType: "image/png"
    });

    const image = await downloadNativeWebImage({ src: "https://images.example.com/kitten.png" });

    expect(image).toBeInstanceOf(File);
    expect(image.name).toBe("kitten.png");
    expect(image.type).toBe("image/png");

    expect(mockedInvoke).toHaveBeenCalledWith("download_web_image", {
      request: {
        url: "https://images.example.com/kitten.png"
      }
    });
  });
});
