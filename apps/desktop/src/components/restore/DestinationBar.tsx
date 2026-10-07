import { Eye, FolderPlus, Folder, LoaderCircle, RotateCcw, Server } from "lucide-react";
import type { Translate } from "../../i18n";
import { tip } from "../../shared/tip";
import type { RestoreModel } from "./useRestoreModel";

export function DestinationBar({ model, t }: { model: RestoreModel; t: Translate }) {
  const { selection, action, resources } = model;
  const replace = selection.mode === "replace";
  const locked = action.busy;
  return (
    <form className="destination" onSubmit={(event) => { event.preventDefault(); void action.preview(); }}>
      <div className="destination__modes" role="radiogroup" aria-label={t("restoreModeTitle")}>
        <button type="button" role="radio" aria-checked={!replace} data-active={!replace || undefined} disabled={locked} onClick={() => selection.setMode("separate")} data-tip-side="start" {...tip(t("restoreModeSeparate"))}><FolderPlus size={16} aria-hidden="true" /></button>
        <button
          type="button" role="radio" aria-checked={replace} data-active={replace || undefined} disabled={locked}
          aria-disabled={!selection.replacementReady || undefined} onClick={() => selection.replacementReady && selection.setMode("replace")} data-tip-side="start"
          {...tip(selection.replacementReady ? t("restoreModeReplace") : `${t("restoreModeReplace")}
${t("restoreReplaceUnavailable")}`)}
        ><RotateCcw size={16} aria-hidden="true" /></button>
      </div>
      <label className="picker" data-tip={t("deployTargetProfile")}>
        <Server size={15} aria-hidden="true" />
        <select value={selection.profileId} disabled={replace || locked} aria-label={t("deployTargetProfile")} onChange={(event) => selection.setProfileId(event.target.value)}>
          {resources.profiles.map((item) => <option key={item.profileId} value={item.profileId}>{item.label}</option>)}
        </select>
      </label>
      <label className="destination__path" data-tip={t(replace ? "restorePlanRollback" : "restoreDestinationHint")}>
        <Folder size={15} aria-hidden="true" />
        <input value={selection.targetPath} readOnly={replace || locked} onChange={(event) => selection.setTargetPath(event.target.value)} placeholder={t("deployTargetPathHint")} aria-label={t("deployTargetPath")} spellCheck={false} />
      </label>
      <button className="button button--primary" type="submit" disabled={action.busy || !selection.profileId || !selection.targetPath}>
        {action.busy ? <LoaderCircle className="spin" size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
        {action.busy ? t("restorePreviewing") : t("restorePreview")}
      </button>
    </form>
  );
}
