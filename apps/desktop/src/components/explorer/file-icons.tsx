import {
  Database, File, FileArchive, FileCode, FileCog, FileImage, FileJson, FileKey, FileText, Folder,
  FolderOpen, Link2, Ban, ScrollText, type LucideIcon,
} from "lucide-react";
import type { RemoteEntryKind } from "../../shared/commands";

export type EntryTone = "folder" | "file" | "data" | "archive" | "media" | "config" | "code" | "log" | "secret" | "link" | "special";
export interface EntryVisual { Icon: LucideIcon; tone: EntryTone }

const byExtension: Record<string, EntryVisual> = {};
const register = (extensions: string, visual: EntryVisual) => extensions.split(" ").forEach((extension) => { byExtension[extension] = visual; });

register("db sqlite sqlite3 sql mdb rdb", { Icon: Database, tone: "data" });
register("zip tar gz tgz zst xz bz2 7z rar lz4 deb rpm", { Icon: FileArchive, tone: "archive" });
register("png jpg jpeg gif svg webp bmp ico avif", { Icon: FileImage, tone: "media" });
register("log out err", { Icon: ScrollText, tone: "log" });
register("json", { Icon: FileJson, tone: "config" });
register("conf cfg ini env yml yaml toml properties service timer", { Icon: FileCog, tone: "config" });
register("sh bash py js mjs ts php rb go rs java c h cpp pl lua", { Icon: FileCode, tone: "code" });
register("txt md rst csv tsv", { Icon: FileText, tone: "file" });
register("key pem crt cer p12 pfx pub", { Icon: FileKey, tone: "secret" });

const specialNames: Record<string, EntryVisual> = {
  dockerfile: { Icon: FileCode, tone: "code" },
  ".env": { Icon: FileCog, tone: "config" },
  "docker-compose.yml": { Icon: FileCog, tone: "config" },
  "docker-compose.yaml": { Icon: FileCog, tone: "config" },
};

/** One visual per remote entry; the same classification drives icon and colour so type is never colour-only. */
export function visualForEntry(entry: { name: string; kind: RemoteEntryKind }, expanded: boolean): EntryVisual {
  if (entry.kind === "directory") return { Icon: expanded ? FolderOpen : Folder, tone: "folder" };
  if (entry.kind === "symlink") return { Icon: Link2, tone: "link" };
  if (entry.kind === "other") return { Icon: Ban, tone: "special" };
  const name = entry.name.toLowerCase();
  const special = specialNames[name];
  if (special) return special;
  const dot = name.lastIndexOf(".");
  return (dot > 0 ? byExtension[name.slice(dot + 1)] : undefined) ?? { Icon: File, tone: "file" };
}
