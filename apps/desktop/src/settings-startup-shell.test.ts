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
  it("uses the WenKai palettes when startup theme parameters are absent", () => {
    const lightDom = renderDesktopIndex("?settings=1&startupAppearanceMode=light");
    const lightShell = lightDom.querySelector(".settings-startup-shell") as Element;
    expect(lightDom.documentElement.dataset.theme).toBe("wenkai-paper-light");
    expect(lightDom.documentElement.style.backgroundColor).toBe("rgb(255, 255, 255)");
    expect(window.getComputedStyle(lightShell).getPropertyValue("--settings-startup-sidebar")).toBe("#ffffff");
    expect(window.getComputedStyle(lightShell).getPropertyValue("--settings-startup-border")).toBe("#ededed");

    const darkDom = renderDesktopIndex("?settings=1&startupAppearanceMode=dark");
    const darkShell = darkDom.querySelector(".settings-startup-shell") as Element;
    expect(darkDom.documentElement.dataset.theme).toBe("wenkai-paper-dark");
    expect(darkDom.documentElement.style.backgroundColor).toBe("rgb(35, 40, 45)");
    expect(window.getComputedStyle(darkShell).getPropertyValue("--settings-startup-sidebar")).toBe("#23282d");
    expect(window.getComputedStyle(darkShell).getPropertyValue("--settings-startup-border")).toBe("#33373c");
  });

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
