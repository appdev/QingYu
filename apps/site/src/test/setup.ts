import * as matchers from "@testing-library/jest-dom/matchers";
import { afterEach, expect, vi } from "vitest";

expect.extend(matchers);

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.lang = "zh-CN";
  vi.unstubAllGlobals();
});
