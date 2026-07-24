import { invokeNative } from "./invoke";
import { message } from "@tauri-apps/plugin-dialog";

export function showNativeAppAbout() {
  return invokeNative("show_native_app_about");
}

type NativePandocSetupLabels = {
  cancelLabel: string;
  installLabel: string;
  message: string;
  setPathLabel: string;
  title: string;
};

export type NativePandocSetupAction = "cancel" | "install" | "setPath";

export async function showNativePandocSetup(labels: NativePandocSetupLabels): Promise<NativePandocSetupAction> {
  const result = await message(labels.message, {
    buttons: {
      cancel: labels.cancelLabel,
      no: labels.setPathLabel,
      yes: labels.installLabel
    },
    kind: "warning",
    title: labels.title
  });

  if (result === labels.installLabel) return "install";
  if (result === labels.setPathLabel) return "setPath";

  return "cancel";
}
