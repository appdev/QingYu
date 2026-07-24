import { renderToString } from "react-dom/server";
import { SiteApp } from "./SiteApp";

export function renderSite(): string {
  return renderToString(<SiteApp initialLocale="zh-CN" />);
}
