export type DownloadPlatform = "macos" | "windows" | "linux";

type NavigatorPlatform = {
  maxTouchPoints?: number;
  platform?: string;
  userAgentData?: { platform?: string };
};

export function detectDownloadPlatform(input: NavigatorPlatform): DownloadPlatform | null {
  const platform = input.userAgentData?.platform ?? input.platform ?? "";
  if (/mac/i.test(platform)) return (input.maxTouchPoints ?? 0) > 0 ? null : "macos";
  if (/win/i.test(platform)) return "windows";
  if (/linux/i.test(platform) && !/android/i.test(platform)) return "linux";
  return null;
}

export function orderDownloadPlatforms(preferred: DownloadPlatform | null): DownloadPlatform[] {
  const platforms: DownloadPlatform[] = ["macos", "windows", "linux"];
  return preferred
    ? [preferred, ...platforms.filter((item) => item !== preferred)]
    : platforms;
}
