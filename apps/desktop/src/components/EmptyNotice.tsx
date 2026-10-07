import type { LucideIcon } from "lucide-react";

interface EmptyNoticeProps {
  icon: LucideIcon;
  message: string;
  action?: string;
  onAction?: () => void;
}

/** One-line empty state: icon, short message, optional single action. */
export function EmptyNotice({ icon: Icon, message, action, onAction }: EmptyNoticeProps) {
  return (
    <div className="empty-notice">
      <Icon size={22} aria-hidden="true" />
      <p>{message}</p>
      {action && <button className="button button--secondary" type="button" disabled={!onAction} onClick={onAction}>{action}</button>}
    </div>
  );
}
