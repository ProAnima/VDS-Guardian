import { spawnSync } from "node:child_process";
import process from "node:process";

const image = "vds-guardian-ssh-fixture:local";
const result = spawnSync("docker", ["build", "--pull=false", "-t", image, "tests/ssh-fixture"], {
  stdio: "inherit",
  shell: false,
});
if (result.status !== 0) process.exit(result.status ?? 1);
console.log("SSH fixture image built. Disposable-host Rust tests will consume this image next.");
