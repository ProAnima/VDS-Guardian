import { spawnSync } from "node:child_process";
import process from "node:process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const image = "vds-guardian-ssh-fixture:local";
const run = (command, args, options = {}) => {
  const result = spawnSync(command, args, { shell: false, encoding: "utf8", ...options });
  if (result.status !== 0) throw new Error(`${command} failed`);
  return result.stdout?.trim() ?? "";
};
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "vds-guardian-ssh-"));
const wait = (milliseconds) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, milliseconds);
const key = path.join(temporary, "backup-key");
let container = "";
try {
  run("docker", ["build", "--pull=false", "-t", image, "tests/ssh-fixture"], { stdio: "inherit" });
  run("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", key]);
  container = run("docker", ["run", "-d", "-p", "127.0.0.1::22", image]);
  run("docker", ["cp", `${key}.pub`, `${container}:/home/backup/.ssh/authorized_keys`]);
  run("docker", ["exec", container, "chown", "backup:backup", "/home/backup/.ssh/authorized_keys"]);
  const endpoint = run("docker", ["port", container, "22/tcp"]);
  const port = endpoint.match(/:(\d+)$/)?.[1];
  if (!port) throw new Error("unable to discover fixture SSH port");
  const hostKey = run("docker", ["exec", container, "cat", "/etc/ssh/ssh_host_ed25519_key.pub"]);
  const knownHosts = path.join(temporary, "known_hosts");
  fs.writeFileSync(knownHosts, `[127.0.0.1]:${port} ${hostKey}\n`, { mode: 0o600 });
  wait(750);
  const archive = path.join(temporary, "filesystem.tar.zst");
  const output = fs.openSync(archive, "w");
  const sshArgs = ["-F", "none", "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes", "-o", `UserKnownHostsFile=${knownHosts}`, "-o", "GlobalKnownHostsFile=none", "-o", "PasswordAuthentication=no", "-o", "KbdInteractiveAuthentication=no", "-o", "PreferredAuthentications=publickey", "-o", "IdentitiesOnly=yes", "-i", key, "-p", port, "backup@127.0.0.1", "tar --create --file=- --zstd --numeric-owner --one-file-system -- '/srv/app'"];
  const capture = spawnSync("ssh", sshArgs, { stdio: ["ignore", output, "pipe"], encoding: "utf8", shell: false });
  if (capture.status !== 0) throw new Error(`SSH capture failed: ${capture.stderr?.trim() || "no diagnostic"}`);
  fs.closeSync(output);
  if (fs.statSync(archive).size === 0) throw new Error("SSH capture stream was empty");
  fs.writeFileSync(knownHosts, `[127.0.0.1]:${port} ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA==\n`);
  const rejected = spawnSync("ssh", sshArgs, { stdio: "ignore", shell: false }).status;
  if (rejected === 0) throw new Error("changed host key was accepted");
  console.log("Disposable SSH capture and changed-key rejection passed.");
} finally {
  if (container) spawnSync("docker", ["rm", "-f", container], { stdio: "ignore", shell: false });
  fs.rmSync(temporary, { recursive: true, force: true });
}
