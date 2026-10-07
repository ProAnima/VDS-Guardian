import { useState } from "react";
import type { Translate } from "../i18n";
import { SshProfilePanel } from "./SshProfilePanel";

export function ServersPanel({ t }: { t: Translate }) {
  const [, setRevision] = useState(0);
  return <main className="view"><SshProfilePanel onProfilesChanged={() => setRevision((current) => current + 1)} t={t} /></main>;
}
