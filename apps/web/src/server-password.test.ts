import { describe, expect, it } from "vitest";

import {
  SERVER_PASSWORD_MAX_LENGTH,
  SERVER_PASSWORD_PATTERN,
  isValidServerPassword,
} from "./server-password";

describe("server owner password", () => {
  it("accepts exactly printable non-space ASCII through 1024 characters", () => {
    const printableAscii = Array.from(
      { length: 0x7e - 0x21 + 1 },
      (_, index) => String.fromCharCode(0x21 + index),
    ).join("");
    expect(SERVER_PASSWORD_PATTERN).toBe("[!-~]+");
    expect(SERVER_PASSWORD_MAX_LENGTH).toBe(1024);
    expect(isValidServerPassword(printableAscii)).toBe(true);
    expect(isValidServerPassword("x".repeat(1024))).toBe(true);
  });

  it.each(["", " ", "a b", "\t", "\n", "\0", "\u007f", "\u00a0", "中文", "Ａ", "😀"])(
    "rejects %j",
    (candidate) => expect(isValidServerPassword(candidate)).toBe(false),
  );

  it("rejects more than 1024 characters", () => {
    expect(isValidServerPassword("x".repeat(1025))).toBe(false);
  });
});
