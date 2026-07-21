import { describe, expect, it } from "vitest";
import { isLocalAttachmentHref } from "./markdown-link-target";

describe("isLocalAttachmentHref", () => {
  it("keeps Markra's existing local attachment routing contract", () => {
    expect(isLocalAttachmentHref("assets/synthetic.pdf")).toBe(true);
    expect(isLocalAttachmentHref("../images/synthetic.png#preview")).toBe(true);
    expect(isLocalAttachmentHref("file:///synthetic/attachment.pdf")).toBe(true);

    expect(isLocalAttachmentHref("./notes/synthetic.md")).toBe(false);
    expect(isLocalAttachmentHref("file:///synthetic/note.markdown#section")).toBe(false);
    expect(isLocalAttachmentHref("https://example.test/attachment.pdf")).toBe(false);
    expect(isLocalAttachmentHref("javascript:alert(1)")).toBe(false);
    expect(isLocalAttachmentHref("mailto:author@example.test")).toBe(false);
    expect(isLocalAttachmentHref("#section")).toBe(false);
  });
});
