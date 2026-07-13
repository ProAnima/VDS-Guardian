import { spawnSync } from "node:child_process";
import { accessSync, constants, statfsSync } from "node:fs";
import process from "node:process";

const checks = [];

function commandCheck(name, command, args, required = true) {
  const result = spawnSync(command, args, { encoding: "utf8", shell: false });
  const detail = result.status === 0
    ? (result.stdout || result.stderr).trim().split(/\r?\n/u)[0]
    : result.error?.message ?? (result.stderr || "not available").trim();
  checks.push({ name, ok: result.status === 0, required, detail });
}

commandCheck("Node.js 22+", process.execPath, ["--version"]);
const npmEntry = process.env.npm_execpath;
if (npmEntry) {
  commandCheck("npm", process.execPath, [npmEntry, "--version"]);
} else {
  commandCheck("npm", "npm", ["--version"]);
}
commandCheck("Rust compiler", "rustc", ["--version"]);
commandCheck("Cargo", "cargo", ["--version"]);
commandCheck("Git", "git", ["--version"]);
commandCheck("Docker (future integration tests)", "docker", ["version", "--format", "{{.Server.Version}}"], false);
commandCheck("OpenSSH client (future adapter spike)", "ssh", ["-V"], false);

try {
  accessSync(process.cwd(), constants.R_OK | constants.W_OK);
  const stats = statfsSync(process.cwd());
  const freeGiB = Number(stats.bavail * stats.bsize) / 1024 ** 3;
  checks.push({ name: "Workspace writable", ok: true, required: true, detail: `${freeGiB.toFixed(1)} GiB free` });
} catch (error) {
  checks.push({ name: "Workspace writable", ok: false, required: true, detail: String(error) });
}

for (const check of checks) {
  const label = check.ok ? "OK" : check.required ? "CRITICAL" : "WARN";
  console.log(`[${label}] ${check.name}: ${check.detail}`);
}

if (checks.some((check) => check.required && !check.ok)) {
  process.exitCode = 1;
}
