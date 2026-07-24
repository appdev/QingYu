import { useEffect, useState, type ReactElement } from "react";
import { FeatureGrid } from "./components/FeatureGrid";
import { FullManifesto } from "./components/FullManifesto";
import { Hero } from "./components/Hero";
import { ManifestoIntro } from "./components/ManifestoIntro";
import { MobilePreview } from "./components/MobilePreview";
import { OpenSourceSection } from "./components/OpenSourceSection";
import { Personalization } from "./components/Personalization";
import { PlatformDownload } from "./components/PlatformDownload";
import { SiteFooter } from "./components/SiteFooter";
import { SiteHeader } from "./components/SiteHeader";
import { SyncStory } from "./components/SyncStory";
import { siteContent, type SiteLocale } from "./content";
import { readStoredLocale, writeStoredLocale } from "./lib/locale";

function getSiteStorage(): Storage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

export function SiteApp({ initialLocale }: { initialLocale: SiteLocale }): ReactElement {
  const [locale, setLocale] = useState(initialLocale);

  useEffect(() => {
    const storage = getSiteStorage();
    setLocale(storage ? readStoredLocale(storage) : "zh-CN");
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale;
    document.title = locale === "zh-CN"
      ? "轻语 QingYu｜开源 Markdown 编辑器"
      : "QingYu — Open-source Markdown editor";
  }, [locale]);

  const changeLocale = (nextLocale: SiteLocale) => {
    setLocale(nextLocale);
    const storage = getSiteStorage();
    if (storage) writeStoredLocale(storage, nextLocale);
  };

  const copy = siteContent[locale];

  return (
    <>
      <SiteHeader copy={copy} locale={locale} onLocaleChange={changeLocale} />
      <main>
        <Hero copy={copy} />
        <ManifestoIntro copy={copy.personality} />
        <FeatureGrid copy={copy} />
        <Personalization copy={copy} />
        <SyncStory copy={copy} />
        <MobilePreview copy={copy} />
        <PlatformDownload copy={copy.downloads} />
        <FullManifesto copy={copy.manifesto} />
        <OpenSourceSection copy={copy.openSource} />
      </main>
      <SiteFooter copy={copy} />
    </>
  );
}
