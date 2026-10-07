import { useEffect, useRef } from "react";
import { useLatest } from "./useLatest";

const FOCUSABLE = "input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [tabindex]:not([tabindex='-1'])";

/**
 * Keyboard behaviour shared by the side drawers: focus moves into the drawer when it opens
 * (the first field, or the drawer itself), Escape closes it when closing is allowed, and focus
 * returns to whatever opened it once it closes.
 */
export function useDrawerFocus<T extends HTMLElement>(onClose: (() => void) | undefined) {
  const ref = useRef<T>(null);
  const close = useLatest(onClose);
  useEffect(() => {
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const drawer = ref.current;
    const first = drawer?.querySelector<HTMLElement>(`form ${FOCUSABLE}, .drawer__content ${FOCUSABLE}`);
    (first ?? drawer)?.focus({ preventScroll: true });
    return () => { if (opener?.isConnected) opener.focus({ preventScroll: true }); };
  }, []);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !close.current || event.defaultPrevented) return;
      event.preventDefault(); close.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);
  return ref;
}
