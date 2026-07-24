import { fireEvent, render, screen } from "@testing-library/react";
import { renderToString } from "react-dom/server";
import { SiteApp } from "./SiteApp";

describe("locale-aware site shell", () => {
  it("renders the approved Chinese product promise", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    expect(screen.getByRole("heading", {
      level: 1,
      name: "明窗净几，字字轻语。"
    })).toBeInTheDocument();
  });

  it("switches to English and persists the choice", () => {
    render(<SiteApp initialLocale="zh-CN" />);
    fireEvent.click(screen.getByRole("button", { name: "English" }));

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("A clear desk");
    expect(window.localStorage.getItem("qingyu.site.locale")).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    expect(document.title).toBe("QingYu — Open-source Markdown editor");
  });

  it("restores a stored locale after the initial render", () => {
    window.localStorage.setItem("qingyu.site.locale", "en");

    render(<SiteApp initialLocale="zh-CN" />);

    expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("A clear desk");
    expect(document.documentElement.lang).toBe("en");
  });

  it("keeps server markup tied to the provided locale", () => {
    window.localStorage.setItem("qingyu.site.locale", "en");

    const markup = renderToString(<SiteApp initialLocale="zh-CN" />);

    expect(markup).toContain("明窗净几，字字轻语。");
    expect(markup).not.toContain("A clear desk");
  });

  it("renders and switches locale in memory when localStorage acquisition throws", () => {
    const localStorageDescriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new DOMException("Storage access denied", "SecurityError");
      }
    });

    try {
      render(<SiteApp initialLocale="zh-CN" />);
      expect(screen.getByRole("heading", {
        level: 1,
        name: "明窗净几，字字轻语。"
      })).toBeInTheDocument();

      fireEvent.click(screen.getByRole("button", { name: "English" }));

      expect(screen.getByRole("heading", { level: 1 })).toHaveTextContent("A clear desk");
      expect(document.documentElement.lang).toBe("en");
    } finally {
      if (localStorageDescriptor) {
        Object.defineProperty(window, "localStorage", localStorageDescriptor);
      }
    }
  });

  it("exposes all approved single-page anchors", () => {
    render(<SiteApp initialLocale="zh-CN" />);

    for (const href of ["#product", "#features", "#sync", "#mobile", "#manifesto", "#download"]) {
      expect(document.querySelector(`a[href="${href}"]`)).not.toBeNull();
    }
  });
});
