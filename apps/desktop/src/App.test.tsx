import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("view switching", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    // jsdom has no matchMedia; the theme preference only needs a "prefers light" answer here.
    window.matchMedia = vi.fn().mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() });
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount()); container.remove();
  });

  const nav = (index: number) => container.querySelectorAll<HTMLButtonElement>(".nav-button")[index] as HTMLButtonElement;
  const hosts = () => [...container.querySelectorAll<HTMLElement>(".view-host")];

  it("mounts a view on its first visit and keeps earlier views alive but hidden", async () => {
    await act(async () => root.render(<App />));
    expect(hosts()).toHaveLength(1);
    await act(async () => nav(1).click());
    await act(async () => nav(3).click());
    expect(hosts()).toHaveLength(3);
    expect(hosts().filter((host) => !host.hidden)).toHaveLength(1);
    await act(async () => nav(1).click());
    expect(hosts()).toHaveLength(3);
    expect(nav(1).getAttribute("aria-current")).toBe("page");
  });
});
