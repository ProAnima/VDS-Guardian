import { Archive, LayoutDashboard, RotateCcw, Server, type LucideIcon } from "lucide-react";
import type { MessageKey } from "../i18n/messages-primary";
import type { Translate } from "../i18n";
import type { ViewId } from "../App";
import { tip } from "../shared/tip";
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
];

interface AppSidebarProps {
  t: Translate;
  activeView: ViewId;
  onNavigate: (view: ViewId) => void;
}

export function AppSidebar({ t, activeView, onNavigate }: AppSidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand" data-tip="VDS Guardian" data-tip-side="end"><BrandMark label="VDS Guardian" /></div>
      <nav className="sidebar__nav" aria-label={t("appTagline")}>
        {primaryNav.map((item) => (
          <NavButton key={item.key} item={item} t={t} activeView={activeView} onNavigate={onNavigate} />
        ))}
      </nav>
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
    <button className="nav-button" data-active={active || undefined} data-tip-side="end" type="button" aria-current={active ? "page" : undefined} onClick={() => onNavigate(item.view)} {...tip(t(item.key))}>
      <Icon size={19} strokeWidth={1.8} aria-hidden="true" />
    </button>
  );
}
