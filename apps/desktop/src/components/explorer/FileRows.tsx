import type { CSSProperties, KeyboardEvent } from "react";
import { CircleAlert, LoaderCircle, RefreshCw } from "lucide-react";
import type { Translate } from "../../i18n";
import type { RemoteBrowseEntry } from "../../shared/commands";
import { tip } from "../../shared/tip";
import { visualForEntry } from "./file-icons";
import { formatDate, formatDateTime, formatSize } from "./format";
import { RowFrame } from "./RowFrame";
import { coveringAncestor } from "./selection";
import type { TreeRow } from "./tree-rows";

export interface FileRowContext {
  t: Translate;
  paths: string[];
  onTogglePath: (path: string) => void;
  onToggleDirectory: (path: string) => void;
  onMore: (path: string) => void;
  onReload: (path: string) => void;
}

type Of<K extends TreeRow["type"]> = Extract<TreeRow, { type: K }>;

export function EntryRow({ row, context }: { row: Of<"entry">; context: FileRowContext }) {
  const { entry, depth, open, loading } = row;
  const { t, paths } = context;
  const covered = coveringAncestor(paths, entry.absolutePath);
  const disabled = !entry.selectable || Boolean(covered);
  const selected = paths.includes(entry.absolutePath);
  const visual = visualForEntry(entry, open);
  const isDirectory = entry.kind === "directory";
  return (
    <RowFrame
      depth={depth} icon={visual.Icon} tone={visual.tone} name={entry.name} dimmed={disabled && !covered} selected={selected}
      open={isDirectory ? open : undefined} loading={loading} openLabel={t(open ? "explorerCollapse" : "explorerExpand")}
      onToggleOpen={isDirectory ? () => context.onToggleDirectory(entry.absolutePath) : undefined}
      hint={covered ? `${t("browserCoveredReason")} ${covered}` : unavailableReason(entry, t)}
      check={{ checked: selected || Boolean(covered), disabled, label: `${t("browserSelect")} ${entry.name}`, onChange: () => context.onTogglePath(entry.absolutePath) }}
      size={entry.kind === "regular_file" ? formatSize(entry.size ?? 0) : undefined}
      date={formatDate(entry.modifiedAt)} dateTitle={formatDateTime(entry.modifiedAt)}
    />
  );
}

export function MoreRow({ row, context }: { row: Of<"more">; context: FileRowContext }) {
  return (
    <div className="tree-note" style={{ "--depth": row.depth } as CSSProperties} role="treeitem" aria-level={row.depth + 1} data-row="" tabIndex={-1} onKeyDown={clickOnEnter}>
      <button className="tree-note__action" disabled={row.loading} type="button" tabIndex={-1} onClick={() => context.onMore(row.path)}>
        {row.loading && <LoaderCircle className="spin" size={13} aria-hidden="true" />}{context.t("browserMore")}
      </button>
    </div>
  );
}

export function NoteRow({ row, context }: { row: Of<"note">; context: FileRowContext }) {
  const { t } = context;
  return (
    <div className="tree-note" data-note={row.note} role="treeitem" aria-level={row.depth + 1} data-row="" tabIndex={-1} style={{ "--depth": row.depth } as CSSProperties} onKeyDown={clickOnEnter}>
      {row.note === "loading" && <LoaderCircle className="spin" size={14} aria-hidden="true" />}
      {row.note === "empty" && <span>{t("browserEmpty")}</span>}
      {row.note === "failure" && <>
        <CircleAlert size={14} aria-hidden="true" /><span role="alert">{row.text}</span>
        <button className="tree-note__icon" type="button" tabIndex={-1} onClick={() => context.onReload(row.path ?? "/")} {...tip(t("browserRetry"))}><RefreshCw size={13} aria-hidden="true" /></button>
      </>}
    </div>
  );
}

/** Enter or Space on a note row activates its single action (load more, retry). */
function clickOnEnter(event: KeyboardEvent<HTMLDivElement>) {
  if (event.target !== event.currentTarget || (event.key !== "Enter" && event.key !== " ")) return;
  event.preventDefault();
  event.currentTarget.querySelector("button")?.click();
}

function unavailableReason(entry: RemoteBrowseEntry, t: Translate): string | undefined {
  if (entry.unavailableReason === "symlink") return t("browserSymlinkReason");
  if (entry.unavailableReason === "special_file") return t("browserSpecialReason");
  return undefined;
}
