import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { SiteApp } from "./SiteApp";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing QingYu site root.");

const app = <StrictMode><SiteApp initialLocale="zh-CN" /></StrictMode>;
if (root.hasChildNodes()) {
  hydrateRoot(root, app);
} else {
  createRoot(root).render(app);
}
