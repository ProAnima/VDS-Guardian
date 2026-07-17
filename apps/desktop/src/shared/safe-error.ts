const ERROR_CODE = /^[a-z][a-z0-9_]{0,63}$/;
const MAX_TEXT_LENGTH = 1024;

interface SafeFailure {
  code: string;
  message: string;
  remediation: string;
}

export function safeErrorText(error: unknown, fallback: string): string {
  return isSafeFailure(error) ? `${error.message} ${error.remediation}` : fallback;
}

function isSafeFailure(error: unknown): error is SafeFailure {
  if (typeof error !== "object" || error === null) return false;
  if (!("code" in error) || !("message" in error) || !("remediation" in error)) return false;
  return validCode(error.code) && validText(error.message) && validText(error.remediation);
}

function validCode(value: unknown): value is string {
  return typeof value === "string" && ERROR_CODE.test(value);
}

function validText(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_TEXT_LENGTH;
}
