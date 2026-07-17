import { Archive, LayoutDashboard, Rocket, RotateCcw, Server, type LucideIcon } from "lucide-react";
import type { MessageKey } from "../i18n/messages-primary";
import type { Translate } from "../i18n";
import type { ViewId } from "../App";
import { BrandMark } from "./BrandMark";

interface NavItem {
  key: MessageKey;
  icon: LucideIcon;
  view: ViewId;
}

const primaryNav: NavItem[] = [
  { key: "navOverview", icon: LayoutDashboard, view: "overview" },
  { key: "navServers", icon: Server, view: "servers" },
  { key: "navBackups", icon: Archive, view: "backup" },
  { key: "navRestore", icon: RotateCcw, view: "restore" },
  { key: "navDeploy", icon: Rocket, view: "deploy" },
];

interface AppSidebarProps {
  t: Translate;
  activeView: ViewId;
  onNavigate: (view: ViewId) => void;
}

export function AppSidebar({ t, activeView, onNavigate }: AppSidebarProps) {
  return (
    <aside className="sidebar">
      <BrandMark tagline={t("appTagline")} />
      <nav className="sidebar__nav" aria-label={t("appTagline")}>
        {primaryNav.map((item) => (
          <NavButton key={item.key} item={item} t={t} activeView={activeView} onNavigate={onNavigate} />
        ))}
      </nav>
      <div className="sidebar__footer">
        <div className="node-pill">
          <span className="node-pill__signal" aria-hidden="true" />
          <div><span>{t("nodeLabel")}</span><strong>{t("localNode")}</strong></div>
        </div>
      </div>
    </aside>
  );
}

interface NavButtonProps {
  item: NavItem;
  t: Translate;
  activeView: ViewId;
  onNavigate: (view: ViewId) => void;
}

function NavButton({ item, t, activeView, onNavigate }: NavButtonProps) {
  const Icon = item.icon;
  const active = item.view === activeView;
  return (
    <button className="nav-button" data-active={active || undefined} type="button" title={t(item.key)} onClick={() => onNavigate(item.view)}>
      <Icon size={18} strokeWidth={1.8} aria-hidden="true" />
      <span>{t(item.key)}</span>
      {active && <span className="nav-button__active" aria-hidden="true" />}
    </button>
  );
}
