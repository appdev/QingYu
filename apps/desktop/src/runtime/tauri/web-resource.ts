import { invokeNative } from "./invoke";
import type { DownloadNativeWebImageInput } from "@markra/app/runtime";

type WebImageDownloadResponse = {
  bytes: number[];
  fileName: string;
  mimeType: string;
};

export async function downloadNativeWebImage({ src }: DownloadNativeWebImageInput): Promise<File> {
  const downloadedImage = await invokeNative<WebImageDownloadResponse>("download_web_image", {
    request: { url: src }
  });

  return new File([new Uint8Array(downloadedImage.bytes)], downloadedImage.fileName, {
    type: downloadedImage.mimeType
  });
}
