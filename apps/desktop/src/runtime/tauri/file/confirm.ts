import { confirm } from "@tauri-apps/plugin-dialog";

type ConfirmationLabels = {
  cancelLabel: string;
  message: string;
  okLabel: string;
};

function confirmFileAction(fileName: string, labels: ConfirmationLabels) {
  return confirm(labels.message, {
    cancelLabel: labels.cancelLabel,
    kind: "warning",
    okLabel: labels.okLabel,
    title: fileName
  });
}

export function confirmNativeMarkdownFileDelete(fileName: string, labels: ConfirmationLabels) {
  return confirmFileAction(fileName, labels);
}

export function confirmNativeWorkspaceResourceTrash(labels: ConfirmationLabels) {
  return confirm(labels.message, {
    cancelLabel: labels.cancelLabel,
    kind: "warning",
    okLabel: labels.okLabel,
    title: "QingYu"
  });
}

export function confirmNativeUnsavedMarkdownDocumentDiscard(fileName: string, labels: ConfirmationLabels) {
  return confirmFileAction(fileName, labels);
}
