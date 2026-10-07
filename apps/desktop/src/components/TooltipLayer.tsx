import { useEffect, useLayoutEffect, useRef, useState } from "react";

const SHOW_DELAY_MS = 380;
const MARGIN = 8;
const GAP = 6;

interface Anchor { text: string; rect: DOMRect; side: string | null }

/**
 * One floating tooltip for every element with `data-tip` (see shared/tip.ts). It is rendered at
 * the document level with fixed positioning, so it is never clipped by a scrolling container,
 * works for any element (including icons), appears on keyboard focus as well as on hover, and is
 * kept inside the window.
 */
export function TooltipLayer() {
  const [anchor, setAnchor] = useState<Anchor>();
  useTooltipTriggers(setAnchor);
  const bubble = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{ left: number; top: number }>();
  useLayoutEffect(() => {
    if (!anchor || !bubble.current) { setPosition(undefined); return; }
    setPosition(place(anchor, bubble.current.getBoundingClientRect()));
  }, [anchor]);
  if (!anchor) return null;
  return (
    <div ref={bubble} className="tooltip" role="tooltip" style={position ? { left: position.left, top: position.top } : { visibility: "hidden" }}>
      {anchor.text}
    </div>
  );
}

function useTooltipTriggers(setAnchor: (anchor: Anchor | undefined) => void) {
  useEffect(() => {
    let current: Element | null = null;
    let timer: number | undefined;
    let pointerAt = 0;
    const hide = () => { window.clearTimeout(timer); current = null; setAnchor(undefined); };
    const press = () => { pointerAt = Date.now(); hide(); };
    const enter = (event: Event) => {
      // Focus that follows a click is not keyboard navigation; the hover already explained it.
      if (event.type === "focusin" && Date.now() - pointerAt < 600) return;
      const target = event.target instanceof Element ? event.target.closest("[data-tip]") : null;
      if (target === current) return;
      hide();
      if (!target) return;
      current = target;
      timer = window.setTimeout(() => {
        const text = target.getAttribute("data-tip");
        if (text && target.isConnected) setAnchor({ text, rect: target.getBoundingClientRect(), side: target.getAttribute("data-tip-side") });
      }, SHOW_DELAY_MS);
    };
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape") hide(); };
    document.addEventListener("pointerover", enter);
    document.addEventListener("focusin", enter);
    document.addEventListener("pointerdown", press);
    document.addEventListener("scroll", hide, true);
    document.addEventListener("keydown", onKey);
    return () => {
      hide();
      document.removeEventListener("pointerover", enter);
      document.removeEventListener("focusin", enter);
      document.removeEventListener("pointerdown", press);
      document.removeEventListener("scroll", hide, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [setAnchor]);
}

/** Below the anchor by default, beside it for `end` (rail icons); always inside the window. */
function place({ rect, side }: Anchor, size: DOMRect): { left: number; top: number } {
  const rtl = document.documentElement.dir === "rtl";
  let left: number;
  let top = rect.bottom + GAP;
  if (side === "end") {
    left = rtl ? rect.left - size.width - GAP : rect.right + GAP;
    top = rect.top + rect.height / 2 - size.height / 2;
  } else if (side === "start") {
    left = rtl ? rect.right - size.width : rect.left;
  } else if (side === "stop") {
    left = rtl ? rect.left : rect.right - size.width;
  } else {
    left = rect.left + rect.width / 2 - size.width / 2;
  }
  if (top + size.height > window.innerHeight - MARGIN) top = rect.top - size.height - GAP;
  return {
    left: Math.min(Math.max(MARGIN, left), window.innerWidth - size.width - MARGIN),
    top: Math.min(Math.max(MARGIN, top), window.innerHeight - size.height - MARGIN),
  };
}
