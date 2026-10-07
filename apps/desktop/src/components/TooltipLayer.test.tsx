import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDrawerFocus } from "../shared/useDrawerFocus";
import { TooltipLayer } from "./TooltipLayer";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("tooltip layer", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers();
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount()); container.remove(); vi.useRealTimers();
  });

  const bubble = () => document.querySelector<HTMLElement>("[role='tooltip']");

  it("shows the hint on keyboard focus, not only on hover, and hides it on Escape", async () => {
    await act(async () => root.render(<><TooltipLayer /><span tabIndex={0} role="img" aria-label="Why" data-tip="Because">?</span></>));
    const hint = container.querySelector<HTMLElement>("[data-tip]");
    await act(async () => { hint?.focus(); vi.advanceTimersByTime(500); });
    expect(bubble()?.textContent).toBe("Because");
    await act(async () => { document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
    expect(bubble()).toBeNull();
  });

  it("does not pop the focus hint up when the element was just clicked", async () => {
    await act(async () => root.render(<><TooltipLayer /><button type="button" data-tip="Backups">B</button></>));
    const target = container.querySelector<HTMLElement>("[data-tip]");
    await act(async () => { document.dispatchEvent(new Event("pointerdown")); target?.focus(); vi.advanceTimersByTime(500); });
    expect(bubble()).toBeNull();
  });

  it("finds the hint from an inner icon and keeps the bubble inside the window", async () => {
    await act(async () => root.render(<><TooltipLayer /><button type="button" data-tip="Refresh"><svg data-testid="icon" /></button></>));
    const icon = container.querySelector("svg");
    await act(async () => { icon?.dispatchEvent(new Event("pointerover", { bubbles: true })); vi.advanceTimersByTime(500); });
    const shown = bubble();
    expect(shown?.textContent).toBe("Refresh");
    expect(Number.parseFloat(shown?.style.left ?? "-1")).toBeGreaterThanOrEqual(8);
    expect(Number.parseFloat(shown?.style.top ?? "-1")).toBeGreaterThanOrEqual(8);
  });
});

describe("drawer focus", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => { container = document.createElement("div"); document.body.append(container); root = createRoot(container); });
  afterEach(async () => { await act(async () => root.unmount()); container.remove(); });

  function Drawer({ onClose }: { onClose?: () => void }) {
    const ref = useDrawerFocus<HTMLElement>(onClose);
    return <aside ref={ref} tabIndex={-1}><form><input aria-label="first" /></form></aside>;
  }

  function Host({ closable }: { closable: boolean }) {
    const [open, setOpen] = useState(false);
    return (
      <>
        <button type="button" onClick={() => setOpen(true)}>open</button>
        {open && <Drawer onClose={closable ? () => setOpen(false) : undefined} />}
      </>
    );
  }

  it("moves focus into the drawer, closes on Escape and returns focus to the opener", async () => {
    await act(async () => root.render(<Host closable />));
    const opener = container.querySelector("button");
    opener?.focus();
    await act(async () => opener?.click());
    expect(document.activeElement?.getAttribute("aria-label")).toBe("first");
    await act(async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
    expect(container.querySelector("aside")).toBeNull();
    expect(document.activeElement).toBe(opener);
  });

  it("ignores Escape when the drawer cannot be dismissed", async () => {
    await act(async () => root.render(<Host closable={false} />));
    await act(async () => container.querySelector("button")?.click());
    await act(async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" })); });
    expect(container.querySelector("aside")).not.toBeNull();
  });
});
