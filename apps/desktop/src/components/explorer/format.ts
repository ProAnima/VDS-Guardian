export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (let index = 1; value >= 1024 && index < units.length; index += 1) { value /= 1024; unit = units[index]; }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${unit}`;
}

export function formatDate(value: string | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? "" : date.toLocaleDateString(undefined, { day: "2-digit", month: "short", year: "numeric" });
}

export function formatDateTime(value: string | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? "" : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function leafName(path: string): string {
  return path === "/" ? "/" : path.split("/").filter(Boolean).at(-1) ?? path;
}
