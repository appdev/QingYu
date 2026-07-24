import { invokeNative } from "./invoke";

export * from "./logs/shared";

export async function openNativeLogFolder() {
  await invokeNative("open_log_folder");
}
