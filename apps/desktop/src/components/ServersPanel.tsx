import { Server } from "lucide-react";
import { useState } from "react";
import type { Translate } from "../i18n";
import { SshProfilePanel } from "./SshProfilePanel";

export function ServersPanel({ t }: { t: Translate }) {
  const [, setRevision] = useState(0);
  return <main className="dashboard">
    <section className="hero-panel hero-panel--compact">
      <div className="hero-panel__content">
        <p className="eyebrow"><Server size={15} aria-hidden="true" />{t("setupServerEyebrow")}</p>
        <h1>{t("serverManagerTitle")}</h1>
        <p>{t("serversBody")}</p>
      </div>
    </section>
    <SshProfilePanel onProfilesChanged={() => setRevision((current) => current + 1)} t={t} />
  </main>;
}
