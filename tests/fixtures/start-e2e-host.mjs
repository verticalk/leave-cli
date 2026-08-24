import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const fixtureDirectory = dirname(fileURLToPath(import.meta.url));
const repository = resolve(fixtureDirectory, "../..");
const leave = join(repository, "target/debug/leave");
const stateDirectory = mkdtempSync(join(tmpdir(), "leave-playwright-state-"));
const workspaceDirectory = mkdtempSync(join(tmpdir(), "leave-playwright-workspace-"));
writeFileSync(join(workspaceDirectory, "note.txt"), "hello from Leave\n", "utf8");
const gitInit = spawnSync("git", ["init", "--initial-branch=main", workspaceDirectory], { encoding: "utf8" });
if (gitInit.status !== 0) {
  process.stderr.write(gitInit.stderr || "Could not initialize the Git fixture.\n");
  process.exit(gitInit.status ?? 1);
}
const registration = spawnSync(leave, [
  "--data-dir", stateDirectory,
  "--json",
  "workspace", "add", workspaceDirectory,
  "--name", "Playwright workspace",
  "--expose-global-customization"
], { encoding: "utf8" });

if (registration.status !== 0) {
  process.stderr.write(registration.stderr || "Could not register the Playwright workspace. Build `leave` first.\n");
  process.exit(registration.status ?? 1);
}

const workspace = JSON.parse(registration.stdout);
const acpCommand = `${process.execPath} ${join(fixtureDirectory, "mock-acp.mjs")}`;
const mockDevin = join(fixtureDirectory, "mock-devin.mjs");
chmodSync(mockDevin, 0o755);
const child = spawn(leave, [
  "--data-dir", stateDirectory,
  "serve",
  "--workspace", workspace.id,
  "--port", "4174",
  "--web-dir", join(repository, "apps/web/dist"),
  "--acp-command", acpCommand,
  "--grant-terminal",
  "--grant-preview"
], { stdio: "inherit", env: { ...process.env, LEAVE_DEVIN_BIN: mockDevin } });

function stop() {
  child.kill("SIGTERM");
}

process.on("SIGINT", stop);
process.on("SIGTERM", stop);
child.on("exit", (code) => process.exit(code ?? 0));
