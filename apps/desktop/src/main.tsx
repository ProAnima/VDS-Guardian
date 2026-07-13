import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { applyDocumentPreferences, getInitialLocale, readTheme, resolveTheme, storageKeys } from "./shared/preferences";
import "./styles/tokens.css";
import "./styles/shell.css";
import "./styles/dashboard.css";

const root = document.getElementById("root");
const initialTheme = readTheme(localStorage.getItem(storageKeys.theme));
applyDocumentPreferences(getInitialLocale(), resolveTheme(initialTheme, matchMedia("(prefers-color-scheme: dark)").matches));

if (root) {
  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
