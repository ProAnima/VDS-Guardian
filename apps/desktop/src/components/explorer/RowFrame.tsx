import type { CSSProperties, KeyboardEvent, ReactNode } from "react";
import { ChevronRight, LoaderCircle, type LucideIcon } from "lucide-react";
import { tip } from "../../shared/tip";
import type { EntryTone } from "./file-icons";

export interface RowCheck { checked: boolean; indeterminate?: boolean; disabled: boolean; label: string; onChange: () => void }

export interface RowFrameProps {
  depth: number;
  icon: LucideIcon;
  tone: EntryTone | "docker" | "section";
  name: string;
  /** Collapse/expand: `undefined` means the row has no children. */
  open?: boolean;
  loading?: boolean;
  openLabel?: string;
  onToggleOpen?: () => void;
  /** Section rows: reload with R or F5 while the row has focus. */
  onReload?: () => void;
  check?: RowCheck;
  /** Reason or detail, shown as a localized tooltip instead of inline text. */
  hint?: string;
  size?: string;
  date?: string;
  dateTitle?: string;
  state?: "active" | "stopped";
  badge?: string;
  detail?: string;
  selected?: boolean;
  dimmed?: boolean;
  children?: ReactNode;
}

export function RowFrame(props: RowFrameProps) {
  const { depth, icon: Icon, tone, name, open, hint, check, selected, dimmed } = props;
  return (
    <div
      className="tree-row" role="treeitem" tabIndex={-1} data-row="" data-tone={tone}
      aria-level={depth + 1} aria-expanded={open} aria-selected={selected ?? false}
      aria-description={hint} data-tip={hint} data-tip-side="start"
      data-selected={selected || undefined} data-dimmed={dimmed || undefined}
      style={{ "--depth": depth } as CSSProperties}
      onKeyDown={(event) => rowKeys(event, props)}
    >
      <Chevron {...props} />
      {check ? <CheckBox check={check} /> : <span className="tree-row__gap" />}
      <span className="tree-row__icon" data-tone={tone}><Icon size={16} aria-hidden="true" /></span>
      <span className="tree-row__name">{name}{props.detail && <code>{props.detail}</code>}{props.state && <i className="tree-row__state" data-state={props.state} aria-hidden="true" />}{props.badge && <b>{props.badge}</b>}</span>
      <span className="tree-row__meta">{props.size}</span>
      <time className="tree-row__meta" title={props.dateTitle}>{props.date}</time>
      {props.children}
    </div>
  );
}

function Chevron({ open, loading, openLabel, onToggleOpen }: RowFrameProps) {
  if (open === undefined) return <span />;
  return (
    <button className="tree-row__chevron" type="button" tabIndex={-1} data-open={open || undefined} onClick={onToggleOpen} {...tip(openLabel ?? "")}>
      {loading ? <LoaderCircle className="spin" size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
    </button>
  );
}

function CheckBox({ check }: { check: RowCheck }) {
  return (
    <input
      type="checkbox" tabIndex={-1} aria-label={check.label} checked={check.checked} disabled={check.disabled}
      ref={(node) => { if (node) node.indeterminate = Boolean(check.indeterminate) && !check.checked; }}
      onChange={check.onChange}
    />
  );
}

function rowKeys(event: KeyboardEvent<HTMLDivElement>, props: RowFrameProps) {
  if (event.target !== event.currentTarget) return;
  const rtl = document.documentElement.dir === "rtl";
  const expand = rtl ? "ArrowLeft" : "ArrowRight";
  const collapse = rtl ? "ArrowRight" : "ArrowLeft";
  if (event.key === expand && props.open === false) { event.preventDefault(); props.onToggleOpen?.(); }
  else if (event.key === collapse && props.open) { event.preventDefault(); props.onToggleOpen?.(); }
  else if ((event.key === "F5" || event.key === "r" || event.key === "R") && props.onReload) { event.preventDefault(); props.onReload(); }
  else if (event.key === " " && props.check && !props.check.disabled) { event.preventDefault(); props.check.onChange(); }
  else if (event.key === "Enter") { event.preventDefault(); if (props.onToggleOpen) props.onToggleOpen(); else if (props.check && !props.check.disabled) props.check.onChange(); }
}
