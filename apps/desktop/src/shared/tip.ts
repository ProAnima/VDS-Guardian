/**
 * Attributes for an icon-first control: the localized label is both the
 * accessible name and the visual tooltip (rendered by `styles/tooltip.css`),
 * so no caption text needs to sit next to the icon.
 */
export function tip(label: string): { "aria-label": string; "data-tip": string } {
  return { "aria-label": label, "data-tip": label };
}
