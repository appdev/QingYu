import { upsertMarkdownFrontmatterTitle } from "@markra/markdown";
import { markdownDocumentTitleFromFileName } from "@markra/shared";

export function markdownDocumentSourceForCreatedFile(fileName: string, source: string) {
  const patched = upsertMarkdownFrontmatterTitle(
    source,
    markdownDocumentTitleFromFileName(fileName)
  );
  if (!patched.ok) {
    throw new Error("Cannot create a document from malformed Front Matter.");
  }

  return patched.source;
}
