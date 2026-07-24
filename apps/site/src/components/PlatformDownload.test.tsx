import { render, screen, waitFor, within } from "@testing-library/react";
import { renderToString } from "react-dom/server";
import { siteContent } from "../content";
import { siteLinks } from "../links";
import { PlatformDownload } from "./PlatformDownload";

const platformCases = [
  ["MacIntel", 0, "macos"],
  ["MacIntel", 5, null],
  ["Win32", 0, "windows"],
  ["Linux x86_64", 0, "linux"],
  ["iPhone", 5, null]
] as const;

describe("PlatformDownload", () => {
  it.each(platformCases)(
    "keeps every desktop card when navigator platform is %s with %i touch points and prefers %s",
    async (navigatorPlatform, maxTouchPoints, preferredPlatform) => {
      vi.stubGlobal("navigator", { platform: navigatorPlatform, maxTouchPoints });
      render(<PlatformDownload copy={siteContent.en.downloads} />);

      await waitFor(() => {
        const preferredCards = document.querySelectorAll('[data-preferred="true"]');
        expect(preferredCards).toHaveLength(preferredPlatform ? 1 : 0);
      });

      const cards = Array.from(document.querySelectorAll<HTMLElement>("[data-platform]"));
      expect(cards.map((card) => card.dataset.platform).sort()).toEqual([
        "linux",
        "macos",
        "windows"
      ]);

      for (const card of cards) {
        expect(within(card).queryByRole("img")).toBeNull();
        expect(card.querySelector("img")).toBeNull();
        expect(within(card).getByRole("link", { name: siteContent.en.downloads.release }))
          .toHaveAttribute("href", siteLinks.releases);
      }

      expect(document.querySelector("#download img")).toBeNull();

      if (preferredPlatform) {
        expect(cards[0]).toHaveAttribute("data-platform", preferredPlatform);
        expect(cards[0]).toHaveAttribute("data-preferred", "true");
      } else {
        expect(cards.map((card) => card.dataset.platform)).toEqual(["macos", "windows", "linux"]);
      }

      expect(screen.getByRole("heading", { name: siteContent.en.downloads.webLabel }))
        .toBeInTheDocument();
      expect(screen.getByRole("link", { name: siteContent.en.downloads.webAction }))
        .toHaveAttribute("href", siteLinks.webEditor);
    }
  );

  it("keeps server output in stable macOS, Windows, Linux order", () => {
    const markup = renderToString(<PlatformDownload copy={siteContent.en.downloads} />);

    expect(markup.indexOf("macOS")).toBeLessThan(markup.indexOf("Windows"));
    expect(markup.indexOf("Windows")).toBeLessThan(markup.indexOf("Linux"));
    expect(markup).not.toContain('data-preferred="true"');
  });
});
