import { readServerCsrfCookie } from "./csrf";

describe("server CSRF cookie", () => {
  it("returns the one exact host cookie without decoding or normalizing its secret", () => {
    expect(readServerCsrfCookie("theme=dark; __Host-qingyu_csrf=abc_DEF-123; locale=zh"))
      .toBe("abc_DEF-123");
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
      expect(readServerCsrfCookie(cookie)).toBeNull();
    }
  });
});
