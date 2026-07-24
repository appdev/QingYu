import { getAppRuntime } from "../../runtime";

export function showNativeAppAbout() {
  return getAppRuntime().dialog.showAppAbout();
}

type NativePandocSetupLabels = {
  cancelLabel: string;
  installLabel: string;
  message: string;
  setPathLabel: string;
  title: string;
};

export type NativePandocSetupAction = "cancel" | "install" | "setPath";

export function showNativePandocSetup(labels: NativePandocSetupLabels): Promise<NativePandocSetupAction> {
  return getAppRuntime().dialog.showPandocSetup(labels);
}
