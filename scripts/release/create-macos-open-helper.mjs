import fs from "node:fs";
import path from "node:path";

const outputPath = process.env.OUTPUT_PATH || "release-assets/QingYu-macOS-Open-Anyway.command";

const script = `#!/bin/bash
set -euo pipefail

APP_PATH="/Applications/QingYu.app"

echo "QingYu macOS open helper"
echo
echo "This helper only removes the macOS quarantine flag from:"
echo "  $APP_PATH"
echo
echo "Use it only if macOS says QingYu is damaged or cannot be opened after you"
echo "dragged QingYu.app into Applications."
echo

if [[ ! -d "$APP_PATH" ]]; then
  echo "QingYu.app was not found in /Applications."
  echo "Drag QingYu.app into Applications first, then run this helper again."
  echo
  read -r -p "Press Return to close this window..." _
  exit 1
fi

read -r -p "Continue? [y/N] " answer
case "$answer" in
  [yY]|[yY][eE][sS]) ;;
  *)
    echo "Cancelled."
    read -r -p "Press Return to close this window..." _
    exit 0
    ;;
esac

echo
echo "Removing quarantine flag..."

if ! xattr -dr com.apple.quarantine "$APP_PATH"; then
  echo
  echo "Could not repair QingYu.app automatically."
  echo "You can try this command manually:"
  echo "  sudo xattr -dr com.apple.quarantine /Applications/QingYu.app"
  echo
  read -r -p "Press Return to close this window..." _
  exit 1
fi

echo
echo "Done. Try opening QingYu from Applications again."
echo
read -r -p "Press Return to close this window..." _
`;

fs.mkdirSync(path.dirname(outputPath), { recursive: true });
fs.writeFileSync(outputPath, script, { mode: 0o755 });
fs.chmodSync(outputPath, 0o755);
