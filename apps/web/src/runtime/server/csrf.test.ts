import { readServerCsrfCookie } from "./csrf";

describe("server CSRF cookie", () => {
  it("returns the one exact host cookie without decoding or normalizing its secret", () => {
    expect(readServerCsrfCookie(
      "theme=dark; __Host-qingyu_csrf=abc_DEF-123; locale=zh",
      "https://notes.example",
    ))
      .toBe("abc_DEF-123");
  });

  it("selects the non-Host cookie only for an exact HTTP browser origin", () => {
    const cookies = "qingyu_csrf=http-proof; __Host-qingyu_csrf=https-proof";

    expect(readServerCsrfCookie(cookies, "http://notes.example:3210")).toBe("http-proof");
    expect(readServerCsrfCookie(cookies, "https://notes.example")).toBe("https-proof");
    expect(readServerCsrfCookie("qingyu_csrf=http-proof", "https://notes.example")).toBeNull();
    expect(readServerCsrfCookie("__Host-qingyu_csrf=https-proof", "http://notes.example:3210"))
      .toBeNull();
  });

  it("fails closed for non-HTTP browser origins", () => {
    for (const origin of ["file:///tmp/index.html", "tauri://localhost", "not-a-url"]) {
      expect(readServerCsrfCookie("qingyu_csrf=proof", origin)).toBeNull();
    }
  });

  it("fails closed for missing, duplicate, empty, encoded, or unsafe values", () => {
    for (const cookie of [
      "",
      "other=value",
      "__Host-qingyu_csrf=",
      "__Host-qingyu_csrf=first; __Host-qingyu_csrf=second",
      "__Host-qingyu_csrf=abc%5Fdef",
      "__Host-qingyu_csrf=abc def",
      "__Host-qingyu_csrf=abc=def",
      "__Host-QINGYU-csrf=abc",
    ]) {
      expect(readServerCsrfCookie(cookie, "https://notes.example")).toBeNull();
    }
  });
});
