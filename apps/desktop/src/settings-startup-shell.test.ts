import { readFileSync } from "node:fs";
import { resolve } from "node:path";

function renderDesktopIndex(search = "") {
  const html = readFileSync(resolve(process.cwd(), "index.html"), "utf8");
  const parsed = new DOMParser().parseFromString(html, "text/html");
  const startupScript = parsed.querySelector("head script:not([type])")?.textContent;

  if (!startupScript) throw new Error("Desktop startup script is missing");

  window.history.replaceState({}, "", `/${search}`);
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-theme-appearance");
  document.documentElement.removeAttribute("data-window");
  document.head.querySelectorAll("style").forEach((style) => style.remove());
  parsed.head.querySelectorAll("style").forEach((style) => {
    document.head.append(style.cloneNode(true));
  });
  document.body.innerHTML = parsed.body.innerHTML;
  window.eval(startupScript);

  return document;
}

describe("settings startup shell", () => {
  it("shows a static shimmer shell before the settings module starts", () => {
    const dom = renderDesktopIndex("?settings=1&startupAppearanceMode=dark");
    const shell = dom.querySelector(".settings-startup-shell");

    expect(shell).not.toBeNull();
    expect(shell?.getAttribute("aria-busy")).toBe("true");
    expect(shell?.querySelectorAll(".settings-startup-shimmer").length).toBeGreaterThan(0);
    expect(window.getComputedStyle(shell as Element).display).not.toBe("none");
  });

  it("keeps the static settings shell hidden on the workspace route", () => {
    const dom = renderDesktopIndex();
    const shell = dom.querySelector(".settings-startup-shell");

    expect(shell).not.toBeNull();
    expect(window.getComputedStyle(shell as Element).display).toBe("none");
  });
});
