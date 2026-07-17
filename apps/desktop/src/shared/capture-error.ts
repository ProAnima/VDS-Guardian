import { safeErrorText } from "./safe-error";

export function captureErrorText(error: unknown, fallback: string): string {
  return safeErrorText(error, fallback);
}
