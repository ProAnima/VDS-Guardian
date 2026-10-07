import { Boxes, Check, Container, Database, FolderCheck, LoaderCircle, Trash2, X } from "lucide-react";
import type { Translate } from "../../i18n";
import type { BackupSelectionItem } from "../../shared/commands";
import { tip } from "../../shared/tip";
import { leafName } from "../explorer/format";
import { itemKey, pathsForItem } from "../explorer/selection";

interface SelectionBasketProps {
  items: BackupSelectionItem[];
  sqlitePath: string;
  reviewing: boolean;
  onSqliteChange: (value: string) => void;
  onRemove: (item: BackupSelectionItem) => void;
  onClear: () => void;
  onReview: () => void;
  t: Translate;
}

export function SelectionBasket(props: SelectionBasketProps) {
  const { items, t } = props;
  const pathCount = new Set(items.flatMap(pathsForItem)).size;
  return (
    <aside className="basket" aria-label={t("browserSelected")}>
      <header className="basket__header">
        <FolderCheck size={16} aria-hidden="true" />
        <strong>{items.length}</strong>
        <span className="basket__paths" {...tip(`${pathCount} ${t("dockerPathsCount")}`)}>{pathCount}</span>
        <button className="icon-button" type="button" disabled={items.length === 0} onClick={props.onClear} {...tip(t("selectionClear"))}><Trash2 size={14} aria-hidden="true" /></button>
      </header>
      <ul className="basket__list">
        {items.map((item) => <BasketRow item={item} key={itemKey(item)} onRemove={props.onRemove} t={t} />)}
        {items.length === 0 && <li className="basket__empty" data-tip={t("selectionEmptyHint")}><FolderCheck size={26} aria-hidden="true" /><span className="sr-only">{t("selectionEmptyHint")}</span></li>}
      </ul>
      <label className="basket__sqlite" data-tip={t("captureDatabaseHint")} data-tip-side="start">
        <Database size={15} aria-hidden="true" />
        <input value={props.sqlitePath} onChange={(event) => props.onSqliteChange(event.target.value)} placeholder="/srv/app/app.sqlite" aria-label={t("captureDatabasePath")} spellCheck={false} />
      </label>
      <button className="button button--primary basket__go" type="button" disabled={items.length === 0 || props.reviewing} onClick={props.onReview}>
        {props.reviewing ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <Check size={16} aria-hidden="true" />}
        {props.reviewing ? t("captureReviewing") : t("backupReview")}
      </button>
    </aside>
  );
}

function BasketRow({ item, onRemove, t }: { item: BackupSelectionItem; onRemove: (item: BackupSelectionItem) => void; t: Translate }) {
  const view = describe(item);
  const Icon = view.icon;
  return (
    <li className="basket__row" data-tip={view.paths} data-tip-side="start">
      <span className="basket__icon" data-tone={view.tone}><Icon size={15} aria-hidden="true" /></span>
      <span className="basket__name">{view.name}</span>
      <button className="icon-button" type="button" onClick={() => onRemove(item)} {...tip(`${t("browserRemove")} ${view.name}`)}><X size={13} aria-hidden="true" /></button>
    </li>
  );
}

function describe(item: BackupSelectionItem) {
  if (item.kind === "remote_path") return { icon: FolderCheck, tone: "folder", name: leafName(item.absolutePath), paths: item.absolutePath };
  if (item.kind === "docker_mount") return { icon: Container, tone: "docker", name: item.mountDestination, paths: item.capturablePath };
  return { icon: Boxes, tone: "docker", name: item.groupId, paths: item.capturablePaths.join("\n") };
}
