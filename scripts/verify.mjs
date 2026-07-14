import { spawnSync } from "node:child_process";
import process from "node:process";

const npmEntry = process.env.npm_execpath;
const npmCommand = npmEntry ? process.execPath : "npm";
const npmArgs = (args) => npmEntry ? [npmEntry, ...args] : args;
const steps = [
  ["Environment doctor", process.execPath, ["scripts/doctor.mjs"]],
  ["Rust formatting", "cargo", ["fmt", "--all", "--check"]],
  ["Frontend lint", npmCommand, npmArgs(["run", "lint", "--workspace", "@vds-guardian/desktop"])],
  ["Frontend types", npmCommand, npmArgs(["run", "check:types", "--workspace", "@vds-guardian/desktop"])],
  ["Frontend tests", npmCommand, npmArgs(["run", "test", "--workspace", "@vds-guardian/desktop"])],
  ["Frontend build", npmCommand, npmArgs(["run", "build", "--workspace", "@vds-guardian/desktop"])],
  ["Rust Clippy", "cargo", ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]],
  ["Rust tests", "cargo", ["test", "--workspace", "--all-targets"]],
];

for (const [label, command, args] of steps) {
  console.log(`\n==> ${label}`);
  const result = spawnSync(command, args, { stdio: "inherit", shell: false });
  if (result.status !== 0) {
    console.error(`\nVerification stopped: ${label} failed.`);
    process.exit(result.status ?? 1);
  }
}

console.log("\nAll canonical verification gates passed.");
