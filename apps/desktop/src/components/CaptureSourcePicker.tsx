import { useState, type ReactNode } from "react";
import { Container, FolderTree } from "lucide-react";
import type { Translate } from "../i18n";
import type { BackupSelectionItem } from "../shared/commands";
import { DockerMountPicker } from "./DockerMountPicker";
import { RemoteFileExplorer } from "./RemoteFileExplorer";

interface CaptureSourcePickerProps {
  items: BackupSelectionItem[];
  onToggleDocker: (item: BackupSelectionItem) => void;
  onTogglePath: (path: string) => void;
  profileId: string;
  selectedPaths: string[];
  t: Translate;
}

type SourceId = "files" | "docker";

export function CaptureSourcePicker(props: CaptureSourcePickerProps) {
  const [source, setSource] = useState<SourceId>("files");
  return <section className="capture-source">
    <div aria-label={props.t("captureSources")} className="capture-source__tabs" role="tablist">
      <SourceTab active={source === "files"} icon={<FolderTree size={16} />} label={props.t("captureSourceFiles")} onSelect={() => setSource("files")} />
      <SourceTab active={source === "docker"} icon={<Container size={16} />} label={props.t("captureSourceDocker")} onSelect={() => setSource("docker")} />
    </div>
    <div className="capture-source__content" role="tabpanel">
      {source === "files"
        ? <RemoteFileExplorer profileId={props.profileId} selectedPaths={props.selectedPaths} onTogglePath={props.onTogglePath} t={props.t} />
        : <DockerMountPicker profileId={props.profileId} selectedItems={props.items} onToggleItem={props.onToggleDocker} t={props.t} />}
    </div>
  </section>;
}

function SourceTab({ active, icon, label, onSelect }: {
  active: boolean; icon: ReactNode; label: string; onSelect: () => void;
}) {
  return <button aria-selected={active} data-active={active || undefined} role="tab" type="button" onClick={onSelect}>{icon}{label}</button>;
}
