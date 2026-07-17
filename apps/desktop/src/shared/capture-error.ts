import type { CaptureFailure } from "./commands";

export function captureErrorText(error: unknown, fallback: string): string {
  return isCaptureFailure(error) ? `${error.message} ${error.remediation}` : fallback;
}

function isCaptureFailure(error: unknown): error is CaptureFailure {
  return typeof error === "object" && error !== null && "message" in error && "remediation" in error
    && typeof error.message === "string" && typeof error.remediation === "string";
}
