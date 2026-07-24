import { Menu, X } from "lucide-react";
import { useEffect, useRef, useState, type ReactElement } from "react";
import type { SiteCopy, SiteLocale } from "../content";
import { siteLinks } from "../links";

type SiteHeaderProps = {
  copy: SiteCopy;
  locale: SiteLocale;
  onLocaleChange: (locale: SiteLocale) => unknown;
};

const navigationItems = [
  { key: "product", href: "#product" },
  { key: "features", href: "#features" },
  { key: "sync", href: "#sync" },
  { key: "mobile", href: "#mobile" },
  { key: "manifesto", href: "#manifesto" },
  { key: "download", href: "#download" }
] as const satisfies ReadonlyArray<{ key: keyof SiteCopy["nav"]; href: `#${string}` }>;

export function SiteHeader({ copy, locale, onLocaleChange }: SiteHeaderProps): ReactElement {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuButtonRef = useRef<HTMLButtonElement>(null);
  const nextLocale: SiteLocale = locale === "zh-CN" ? "en" : "zh-CN";

  useEffect(() => {
    if (!menuOpen) return;

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setMenuOpen(false);
      menuButtonRef.current?.focus();
    };

    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [menuOpen]);

  return (
    <header className="site-header">
      <a className="site-brand" href="#product" aria-label={copy.accessibility.brandHome}>
        <img src="/qingyu-logo.webp" alt="" />
        <span aria-hidden="true">{copy.brand.name}</span>
      </a>

      <div className="site-header__actions">
        <button type="button" lang={nextLocale} onClick={() => onLocaleChange(nextLocale)}>
          {copy.languageLabel}
        </button>
        <a className="site-header__web" href={siteLinks.webEditor}>{copy.hero.web}</a>
        <a className="site-header__download" href={siteLinks.releases}>{copy.hero.download}</a>
        <button
          className="site-header__menu"
          ref={menuButtonRef}
          type="button"
          aria-label={copy.accessibility.navigationMenu}
          aria-controls="site-compact-navigation"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen((open) => !open)}
        >
          {menuOpen ? <X aria-hidden="true" /> : <Menu aria-hidden="true" />}
        </button>
      </div>

      <nav
        className="site-compact-navigation"
        id="site-compact-navigation"
        aria-label={copy.accessibility.compactNavigation}
        hidden={!menuOpen}
      >
        {navigationItems.map(({ key, href }) => (
          <a key={key} href={href} onClick={() => setMenuOpen(false)}>{copy.nav[key]}</a>
        ))}
      </nav>
    </header>
  );
}
