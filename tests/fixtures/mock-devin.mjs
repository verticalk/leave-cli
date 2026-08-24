#!/usr/bin/env node

const [group, action, ...rest] = process.argv.slice(2);

if (group === "acp") {
  await import("./mock-acp.mjs");
} else if (group === "rules" && action === "list") {
  process.stdout.write("project-rule  .windsurf/rules/project-rule.md\n");
} else if (group === "rules" && action === "show") {
  process.stdout.write(`# ${rest.at(-1)}\nKeep workspace changes small and reviewed.\n`);
} else if (group === "skills" && action === "list") {
  process.stdout.write("review  user + model\n");
} else if (group === "skills" && action === "show") {
  process.stdout.write(`Skill: ${rest.at(-1)}\nTrigger: user, model\n`);
} else if (group === "plugins" && action === "list") {
  process.stdout.write("fixture-tools  local  ready\n");
} else if (group === "plugins" && action === "info") {
  process.stdout.write(`Plugin: ${rest.at(-1)}\nSkills: review\n`);
} else if (group === "plugins") {
  process.stdout.write(`Plugin ${action} completed.\n`);
} else if (group === "mcp" && action === "list") {
  process.stdout.write("fixture-mcp  local  enabled\n");
} else if (group === "mcp" && action === "get") {
  process.stdout.write(`Server: ${rest.at(-1)}\nTransport: HTTP\n`);
} else if (group === "mcp") {
  process.stdout.write(`MCP ${action} completed.\n`);
} else if (group === "--version") {
  process.stdout.write("devin fixture\n");
} else if (group === "auth" && action === "status") {
  process.stdout.write("Logged in (fixture)\n");
} else {
  process.stderr.write(`Unsupported fixture command: ${process.argv.slice(2).join(" ")}\n`);
  process.exitCode = 2;
}
