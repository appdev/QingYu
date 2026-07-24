import { describe, expect, it } from "vitest";
import { isSafeEditorResourceUrl } from "./resource-urls";

describe("isSafeEditorResourceUrl", () => {
  it.each([
    "file:///Users/example/notes/reference.pdf",
    "FILE:///C:/Notes/reference.pdf",
    "https://cdn.example.test/image.png",
    "http://localhost:3000/image.png",
    "blob:https://app.example.test/asset-id",
    "assets/reference.pdf",
    "../assets/reference.pdf",
    "assets/Reference%20Doc.pdf"
  ])("allows supported editor resource URL %s", (url) => {
    expect(isSafeEditorResourceUrl(url)).toBe(true);
  });

  it.each([
    "javascript:alert(1)",
    "JaVaScRiPt:alert(1)",
    "javascript&colon;alert%281%29",
    "javascript&#58;alert%281%29",
    String.raw`javascript\:alert%281%29`,
    "javascript%3Aalert%281%29",
    "java%73cript%3Aalert%281%29",
    "data:text/html,<script>alert(1)</script>",
    "data&colon;text/html,test",
    "vbscript:msgbox(1)",
    "custom-protocol://resource"
  ])("rejects unsupported editor resource URL %s", (url) => {
    expect(isSafeEditorResourceUrl(url)).toBe(false);
  });
});
