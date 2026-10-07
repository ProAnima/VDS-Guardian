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
commandCheck("Docker (clean-room drill)", "docker", ["version", "--format", "{{.Server.Version}}"], false);
openSshCheck();

/**
 * OpenSSH is the SSH transport. Password logins rely on SSH_ASKPASS_REQUIRE=force, which
 * OpenSSH only honours from 8.4 on; key and agent logins work with older clients.
 */
function openSshCheck() {
  const name = "OpenSSH client 8.4+ (SSH transport; password login needs SSH_ASKPASS_REQUIRE)";
  const result = spawnSync("ssh", ["-V"], { encoding: "utf8", shell: false });
  const banner = `${result.stderr ?? ""}${result.stdout ?? ""}`.trim().split(/\r?\n/u)[0] ?? "";
  if (result.status !== 0) {
    checks.push({ name, ok: false, required: false, detail: result.error?.message ?? (banner || "not available") });
    return;
  }
  const version = /OpenSSH[^\d]*(\d+)\.(\d+)/u.exec(banner);
  const major = Number(version?.[1] ?? 0);
  const minor = Number(version?.[2] ?? 0);
  const ok = major > 8 || (major === 8 && minor >= 4);
  checks.push({ name, ok, required: false, detail: ok ? banner : `${banner || "unknown version"} (password login unavailable before 8.4)` });
}

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
