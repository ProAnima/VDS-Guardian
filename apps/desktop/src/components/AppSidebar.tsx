import {
  Activity, Archive, LayoutDashboard, RotateCcw, Server, Settings,
  type LucideIcon,
} from "lucide-react";
import type { MessageKey } from "../i18n/messages-primary";
import type { Translate } from "../i18n";
import { BrandMark } from "./BrandMark";

interface NavItem {
  key: MessageKey;
  icon: LucideIcon;
  active?: boolean;
}

const primaryNav: NavItem[] = [
  { key: "navOverview", icon: LayoutDashboard, active: true },
  { key: "navServers", icon: Server },
  { key: "navBackups", icon: Archive },
  { key: "navRestore", icon: RotateCcw },
  { key: "navActivity", icon: Activity },
];

export function AppSidebar({ t }: { t: Translate }) {
  return (
    <aside className="sidebar">
      <BrandMark tagline={t("appTagline")} />
      <nav className="sidebar__nav" aria-label={t("appTagline")}>
        {primaryNav.map((item) => <NavButton key={item.key} item={item} t={t} />)}
      </nav>
      <div className="sidebar__footer">
        <NavButton item={{ key: "navSettings", icon: Settings }} t={t} />
        <div className="node-pill">
          <span className="node-pill__signal" aria-hidden="true" />
          <div><span>{t("nodeLabel")}</span><strong>{t("localNode")}</strong></div>
        </div>
      </div>
    </aside>
  );
}

function NavButton({ item, t }: { item: NavItem; t: Translate }) {
  const Icon = item.icon;
  return (
    <button className="nav-button" data-active={item.active || undefined} type="button" title={t(item.key)} disabled={!item.active}>
      <Icon size={18} strokeWidth={1.8} aria-hidden="true" />
      <span>{t(item.key)}</span>
      {item.active && <span className="nav-button__active" aria-hidden="true" />}
    </button>
  );
}
