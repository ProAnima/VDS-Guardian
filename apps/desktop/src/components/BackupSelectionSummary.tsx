import { Boxes, Container, FileCheck, Folder, Trash2, X } from "lucide-react";
import type { Translate } from "../i18n";
import type { BackupSelectionItem } from "../shared/commands";

interface BackupSelectionSummaryProps {
  items: BackupSelectionItem[];
  onClear: () => void;
  onRemove: (item: BackupSelectionItem) => void;
  t: Translate;
}

export function BackupSelectionSummary({ items, onClear, onRemove, t }: BackupSelectionSummaryProps) {
  const pathCount = new Set(items.flatMap(pathsForItem)).size;
  return <section className="selection-summary" aria-labelledby="selection-summary-title">
    <header><div><span className="selection-summary__icon"><FileCheck size={17} /></span><div><strong id="selection-summary-title">{t("browserSelected")}</strong><p>{items.length === 0 ? t("browserSelectedEmpty") : `${items.length} ${t("selectionItemsCount")} · ${pathCount} ${t("dockerPathsCount")}`}</p></div></div>{items.length > 0 && <button type="button" onClick={onClear}><Trash2 size={14} />{t("selectionClear")}</button>}</header>
    {items.length === 0 ? <div className="selection-summary__empty"><Folder size={22} /><span>{t("selectionEmptyHint")}</span></div> : <div className="selection-summary__items">{items.map((item) => <SelectionRow item={item} key={selectionKey(item)} onRemove={onRemove} t={t} />)}</div>}
  </section>;
}

function SelectionRow({ item, onRemove, t }: { item: BackupSelectionItem; onRemove: (item: BackupSelectionItem) => void; t: Translate }) {
  const presentation = selectionPresentation(item, t);
  return <div className="selection-summary__row"><span className="selection-summary__type">{presentation.icon}</span><div><strong>{presentation.title}</strong><code>{presentation.path}</code><small>{presentation.detail}</small></div><button aria-label={`${t("browserRemove")} ${presentation.title}`} title={t("browserRemove")} type="button" onClick={() => onRemove(item)}><X size={15} /></button></div>;
}

function selectionPresentation(item: BackupSelectionItem, t: Translate) {
  if (item.kind === "remote_path") return { icon: <Folder size={16} />, title: leafName(item.absolutePath), path: item.absolutePath, detail: t("selectionFileSystem") };
  if (item.kind === "docker_mount") return { icon: <Container size={16} />, title: item.mountDestination, path: item.capturablePath, detail: t("selectionDockerMount") };
  return { icon: <Boxes size={16} />, title: item.groupId, path: item.capturablePaths.join(" · "), detail: `${t("selectionDockerGroup")} · ${item.capturablePaths.length} ${t("dockerPathsCount")}` };
}

function leafName(path: string): string { return path === "/" ? "/" : path.split("/").filter(Boolean).at(-1) ?? path; }
function pathsForItem(item: BackupSelectionItem): string[] { if (item.kind === "remote_path") return [item.absolutePath]; if (item.kind === "docker_mount") return [item.capturablePath]; return item.capturablePaths; }
function selectionKey(item: BackupSelectionItem): string { return JSON.stringify(item); }
