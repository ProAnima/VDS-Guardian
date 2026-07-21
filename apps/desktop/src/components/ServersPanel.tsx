import { useState } from "react";
import type { Translate } from "../i18n";
import { SshProfilePanel } from "./SshProfilePanel";

export function ServersPanel({ t }: { t: Translate }) {
  const [, setRevision] = useState(0);
  return <main className="dashboard">
    <SshProfilePanel onProfilesChanged={() => setRevision((current) => current + 1)} t={t} />
  </main>;
}
