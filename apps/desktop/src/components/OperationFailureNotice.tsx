import { CircleAlert } from "lucide-react";
import type { Translate } from "../i18n";
import type { MessageKey } from "../i18n/messages-primary";

interface OperationFailureNoticeProps {
  message: string;
  safe: MessageKey;
  changed: MessageKey;
  t: Translate;
}

export function OperationFailureNotice({ message, safe, changed, t }: OperationFailureNoticeProps) {
  return (
    <section className="operation-failure" role="alert">
      <CircleAlert size={16} aria-hidden="true" />
      <div>
        <p>{message}</p>
        <p><strong>{t("failureSafeLabel")}</strong> {t(safe)}</p>
        <p><strong>{t("failureChangedLabel")}</strong> {t(changed)}</p>
      </div>
    </section>
  );
}
