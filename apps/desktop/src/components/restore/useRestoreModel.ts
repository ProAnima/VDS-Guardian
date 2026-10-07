import type { Translate } from "../../i18n";
import { useRestoreAction } from "./useRestoreAction";
import { useRestoreBackups } from "./useRestoreBackups";
import { useRestoreResources } from "./useRestoreResources";
import { useRestoreSelection } from "./useRestoreSelection";

export function useRestoreModel(t: Translate) {
  const resources = useRestoreResources(t);
  const backups = useRestoreBackups(resources.repositoryId, t);
  const selection = useRestoreSelection(resources.profiles, resources.repositoryId, backups.backupId, t);
  const action = useRestoreAction(t, {
    repositoryId: resources.repositoryId, backupId: backups.backupId,
    profileId: selection.profileId, mode: selection.mode, targetPath: selection.targetPath,
  });
  return { resources, backups, selection, action };
}

export type RestoreModel = ReturnType<typeof useRestoreModel>;
