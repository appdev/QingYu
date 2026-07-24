import fs from "node:fs";

const marker = "<!-- markra-macos-unsigned-notice -->";
const notesPath = process.env.RELEASE_NOTES_PATH || "release-notes.md";

if (!fs.existsSync(notesPath)) {
  throw new Error(`Release notes file not found: ${notesPath}`);
}

const notes = fs.readFileSync(notesPath, "utf8");

if (notes.includes(marker)) {
  process.exit(0);
}

const notice = `${marker}
## macOS Notice

QingYu's macOS build is currently unsigned. If macOS blocks the first launch, Control-click \`QingYu.app\`, choose \`Open\`, then confirm \`Open\`.

If macOS says the app is damaged, drag \`QingYu.app\` to \`/Applications\`, then run \`QingYu-macOS-Open-Anyway.command\` inside the macOS DMG. The helper only removes the quarantine flag from \`/Applications/QingYu.app\`.

`;

fs.writeFileSync(notesPath, `${notice}${notes}`);
