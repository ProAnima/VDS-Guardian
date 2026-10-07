import type { ReactNode } from "react";
import { Archive, Server } from "lucide-react";
import type { Translate } from "../../i18n";
import type { RepositorySummary, SshProfileSummary } from "../../shared/commands";

interface BackupToolbarProps {
  profiles: SshProfileSummary[];
  repositories: RepositorySummary[];
  profileId: string;
  repositoryId: string;
  disabled: boolean;
  onProfile: (id: string) => void;
  onRepository: (id: string) => void;
  children?: ReactNode;
  t: Translate;
}

export function BackupToolbar(props: BackupToolbarProps) {
  const { t } = props;
  return (
    <div className="backup-toolbar" role="toolbar" aria-label={t("backupChooseDataTitle")}>
      <label className="picker" data-tip={t("setupServer")}>
        <Server size={15} aria-hidden="true" />
        <select value={props.profileId} disabled={props.disabled} aria-label={t("setupServer")} onChange={(event) => props.onProfile(event.target.value)}>
          {props.profiles.map((profile) => <option key={profile.profileId} value={profile.profileId}>{profile.label}</option>)}
        </select>
      </label>
      <label className="picker" data-tip={t("setupStorage")}>
        <Archive size={15} aria-hidden="true" />
        <select value={props.repositoryId} disabled={props.disabled} aria-label={t("setupStorage")} onChange={(event) => props.onRepository(event.target.value)}>
          {props.repositories.map((repository) => <option key={repository.repositoryId} value={repository.repositoryId}>{repository.label}</option>)}
        </select>
      </label>
      <span className="backup-toolbar__spacer" />
      {props.children}
    </div>
  );
}
