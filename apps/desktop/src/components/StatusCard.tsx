import type { LucideIcon } from "lucide-react";

interface StatusCardProps {
  icon: LucideIcon;
  label: string;
  value: string;
  tone: "green" | "amber" | "neutral";
}

export function StatusCard({ icon: Icon, label, value, tone }: StatusCardProps) {
  return (
    <article className="status-card" data-tone={tone}>
      <div className="status-card__icon"><Icon size={19} strokeWidth={1.8} aria-hidden="true" /></div>
      <span>{label}</span>
      <strong>{value}</strong>
      <i className="status-card__indicator" aria-hidden="true" />
    </article>
  );
}
