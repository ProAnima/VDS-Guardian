import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";

function run(command, args, env = process.env) {
  const result = spawnSync(command, args, {
    cwd: process.cwd(),
    env,
    stdio: "inherit",
    shell: false,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run("cargo", ["build", "--package", "guardian-cli", "--bin", "guardian-cli"]);

const configuredTarget = process.env.CARGO_TARGET_DIR;
const targetDir = configuredTarget
  ? path.resolve(process.cwd(), configuredTarget)
  : path.resolve(process.cwd(), "target");
const binary = path.join(
  targetDir,
  "debug",
  process.platform === "win32" ? "guardian-cli.exe" : "guardian-cli",
);

run(
  "cargo",
  [
    "test",
    "--package",
    "guardian-capture",
    "--test",
    "clean_room_drill",
    "--",
    "--ignored",
    "--nocapture",
    "--test-threads=1",
  ],
  { ...process.env, GUARDIAN_CLI_BIN: binary },
);
