import { useEffect, useState } from "react";
import {
  getFoundationStatus,
  previewStatus,
  type FoundationStatus,
} from "./shared/commands";

const principles = [
  ["Independent", "Every sealed backup is self-contained and movable."],
  ["Fail closed", "Unverified or suspicious runs never become restorable."],
  ["Recoverable", "A restore drill—not a green upload—proves readiness."],
] as const;

export function App() {
  const [status, setStatus] = useState<FoundationStatus>(previewStatus);

  useEffect(() => {
    void getFoundationStatus().then(setStatus);
  }, []);

  return (
    <main className="shell">
      <header className="masthead">
        <div className="mark" aria-hidden="true">VG</div>
        <div>
          <p className="eyebrow">Recovery control plane</p>
          <h1>{status.product}</h1>
        </div>
        <span className="version">v{status.version}</span>
      </header>

      <section className="hero" aria-labelledby="foundation-heading">
        <div>
          <p className="state">Foundation ready</p>
          <h2 id="foundation-heading">Designed for the day the server goes dark.</h2>
          <p className="summary">
            The cross-platform core, native shell, quality gates, security model,
            and recovery roadmap are in place. Live server operations remain
            deliberately locked until isolation and restore tests exist.
          </p>
        </div>
        <div className="lock-card">
          <span className="lock-dot" aria-hidden="true" />
          <strong>Live operations locked</strong>
          <span>{status.iteration}</span>
        </div>
      </section>

      <PrincipleGrid />

      <footer>
        <span>Windows + Linux</span>
        <span>Rust core · Tauri 2 · React</span>
        <span>Apache-2.0</span>
      </footer>
    </main>
  );
}

function PrincipleGrid() {
  return (
    <section className="principles" aria-label="Core principles">
      {principles.map(([title, copy], index) => (
        <article key={title}>
          <span>0{index + 1}</span>
          <h3>{title}</h3>
          <p>{copy}</p>
        </article>
      ))}
    </section>
  );
}
