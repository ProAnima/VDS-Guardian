import { Container, Server } from "lucide-react";
import type { Translate } from "../../i18n";
import type { BackupRestoreDescription } from "../../shared/commands";
import { visualForEntry } from "../explorer/file-icons";
import { leafName } from "../explorer/format";
import { isActive } from "../explorer/docker-model";

export function BackupContents({ description, t }: { description: BackupRestoreDescription; t: Translate }) {
  const hidden = description.totalEntries - description.entries.length;
  return (
    <div className="contents">
      <ul className="contents__roots" aria-label={t("restorePlanSource")}>
        {description.roots.map((root) => <li key={root} data-tip={t("restorePlanSource")}><Server size={14} aria-hidden="true" /><code>{root}</code></li>)}
      </ul>
      <ul className="contents__entries" aria-label={t("restoreImpactAdds")}>
        {description.entries.map((entry) => {
          const visual = visualForEntry({ name: leafName(entry.path), kind: entry.kind === "directory" ? "directory" : "regular_file" }, false);
          return <li key={entry.path}><span className="tree-row__icon" data-tone={visual.tone}><visual.Icon size={15} aria-hidden="true" /></span><code>{entry.path}</code></li>;
        })}
        {hidden > 0 && <li className="contents__more">+{hidden}</li>}
      </ul>
      {description.dockerWorkloads.length > 0 && (
        <ul className="contents__workloads" aria-label={t("restoreImpactWorkloads")}>
          {description.dockerWorkloads.map((item) => (
            <li key={item.containerId} data-tip={t(isActive(item.state) ? "dockerActive" : "dockerStopped")}>
              <Container size={14} aria-hidden="true" /><span>{item.containerName}</span>
              <i className="tree-row__state" data-state={isActive(item.state) ? "active" : "stopped"} aria-hidden="true" /><code>{item.image}</code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
