import { getAppRuntime } from "../../runtime";
import type { DownloadNativeWebImageInput } from "./file";

export function downloadNativeWebImage(input: DownloadNativeWebImageInput) {
  return getAppRuntime().webResource.downloadImage(input);
}
