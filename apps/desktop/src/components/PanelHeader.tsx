import type { ReactNode } from "react";
import { CircleHelp, type LucideIcon } from "lucide-react";

interface PanelHeaderProps {
  id: string;
  icon: LucideIcon;
  title: string;
  /** Longer explanation, shown only as a tooltip so the panel stays one line tall. */
  hint?: string;
  children?: ReactNode;
}

export function PanelHeader({ id, icon: Icon, title, hint, children }: PanelHeaderProps) {
  return (
    <header className="panel-header">
      <Icon size={16} aria-hidden="true" />
      <h2 id={id}>{title}</h2>
      {hint && <span className="field__hint" data-tip={hint} data-tip-side="start" tabIndex={0} role="img" aria-label={hint}><CircleHelp size={13} aria-hidden="true" /></span>}
      <span className="panel-header__end">{children}</span>
    </header>
  );
}
