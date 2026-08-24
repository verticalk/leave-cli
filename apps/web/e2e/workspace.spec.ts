import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("guides a nontechnical user from computer checks to a running workspace", async ({ page }) => {
  const launchRequests: Array<Record<string, unknown>> = [];
  const status = {
    version: "0.1.0-alpha.1",
    platform: { id: "linux", label: "Linux", serviceLabel: "systemd user service" },
    devin: { installed: true, ready: true, label: "Devin", detail: "Logged in", path: "/opt/devin/bin/devin", url: null },
    tailscale: { installed: true, ready: true, label: "Phone access", detail: "Tailscale is connected.", path: null, url: "https://leave-host.example.ts.net" },
    browser: { installed: true, ready: true, label: "Browser preview", detail: "Chromium is ready.", path: "/opt/chromium", url: null },
    folderPickerAvailable: true,
    workspaceExample: "/home/you/Projects/my-app",
    hostPort: 8788
  };
  await page.route("**/api/v1/setup/status", async (route) => {
    expect(route.request().headers()["x-leave-setup-token"]).toBe("fixture-token");
    await route.fulfill({ contentType: "application/json", body: JSON.stringify(status) });
  });
  await page.route("**/api/v1/setup/launch", async (route) => {
    expect(route.request().headers()["x-leave-setup-token"]).toBe("fixture-token");
    launchRequests.push(route.request().postDataJSON() as Record<string, unknown>);
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        localUrl: "http://127.0.0.1:8788",
        awayUrl: "https://leave-host.example.ts.net",
        workspacePath: "/home/you/Projects/my-app",
        background: true
      })
    });
  });

  await page.goto("/setup?token=fixture-token");
  await expect(page.getByRole("heading", { name: "Check this computer" })).toBeVisible();
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByLabel("Workspace folder", { exact: true }).fill("/home/you/Projects/my-app");
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("checkbox", { name: /Open from my phone/ }).check();
  await page.getByRole("checkbox", { name: /Terminal/ }).check();
  await page.getByRole("button", { name: "Continue" }).click();

  await expect(page.getByText("Private Tailscale access")).toBeVisible();
  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations).toEqual([]);
  await page.getByRole("button", { name: "Start Leave" }).click();
  await expect(page.getByRole("heading", { name: "Workspace connected" })).toBeVisible();
  await expect(page.getByText("https://leave-host.example.ts.net")).toBeVisible();
  expect(launchRequests).toEqual([{
    workspacePath: "/home/you/Projects/my-app",
    port: 8788,
    away: true,
    background: true,
    terminal: true,
    preview: false,
    globalCustomization: false
  }]);
  expect(await page.evaluate(() => document.body.scrollWidth <= window.innerWidth)).toBe(true);
});

test("creates a real ACP session and completes a permission round trip", async ({ page, request }) => {
  await page.goto("/sessions");
  await expect(page.getByRole("heading", { name: "Sessions" })).toBeVisible();
  await page.getByRole("button", { name: "New session" }).click();
  await expect(page).toHaveURL(/\/sessions\/fixture-session-/);

  await page.getByRole("textbox", { name: "Message Devin" }).fill("Verify the browser ACP path");
  await page.getByRole("button", { name: "Send message to Devin" }).click();
  await expect(page.getByRole("button", { name: "Approve once" })).toBeVisible();

  const sessionId = decodeURIComponent(new URL(page.url()).pathname.split("/").at(-1) ?? "");
  const competingPrompt = await request.post(`/api/v1/local/sessions/${encodeURIComponent(sessionId)}/prompts`, {
    data: { commandId: crypto.randomUUID(), text: "This turn must not start concurrently" }
  });
  expect(competingPrompt.status()).toBe(503);
  expect(await competingPrompt.json()).toMatchObject({
    error: { message: "this session already has an active Devin turn" }
  });

  await page.getByRole("button", { name: "Approve once" }).click();
  await expect(page.getByText("Permission received. The real ACP round trip is working.")).toBeVisible();
  await expect(page.getByText("Live ACP stream")).toBeVisible();
  expect(await page.evaluate(() => document.body.scrollWidth <= window.innerWidth)).toBe(true);

  await page.getByRole("tab", { name: "Files" }).click();
  await expect(page.getByRole("button", { name: "note.txt" })).toBeVisible();
});

test("edits a guarded file and exposes structured Git status", async ({ page, request }, testInfo) => {
  const fileName = `note-${testInfo.project.name}.txt`;
  const seeded = await request.put("/api/v1/local/file", {
    data: {
      path: fileName,
      baseHash: "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262",
      content: "hello from Leave\n"
    }
  });
  expect(seeded.ok()).toBe(true);
  const created = await request.post("/api/v1/local/sessions", { data: { title: "Workspace tools" } });
  const session = await created.json() as { session_id: string };
  await page.goto(`/sessions/${session.session_id}`);
  await page.getByRole("tab", { name: "Files" }).click();
  await page.getByRole("button", { name: fileName }).click();
  const editor = page.locator(".cm-content");
  await expect(editor).toContainText("hello from Leave");
  await editor.click();
  await page.keyboard.press(process.platform === "darwin" ? "Meta+A" : "Control+A");
  await page.keyboard.type("edited safely from mobile");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Saved", { exact: true })).toBeVisible();

  const stored = await request.get(`/api/v1/local/file?path=${encodeURIComponent(fileName)}`);
  expect(await stored.json()).toMatchObject({ content: "edited safely from mobile" });

  await page.getByRole("tab", { name: "Git", exact: true }).click();
  await expect(page.getByText("main", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: fileName })).toBeVisible();
});

test("opens a real no-scrollback PTY and a managed browser preview", async ({ page, request }) => {
  const created = await request.post("/api/v1/local/sessions", { data: { title: "Host tools" } });
  const session = await created.json() as { session_id: string };
  await page.goto(`/sessions/${session.session_id}`);

  await page.getByRole("tab", { name: "Terminal" }).click();
  await page.getByRole("button", { name: "Open terminal" }).click();
  await expect(page.locator(".connection-text.open")).toBeVisible();
  await page.locator(".xterm-helper-textarea").focus();
  await page.keyboard.type("printf LEAVE_PTY_OK");
  await page.keyboard.press("Enter");
  await expect(page.locator(".xterm-rows")).toContainText("LEAVE_PTY_OK");

  await page.getByRole("tab", { name: "Preview" }).click();
  await page.getByRole("textbox", { name: "Loopback preview URL" }).fill("http://127.0.0.1:4174/");
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect(page.getByRole("img", { name: "Live browser preview" })).toBeVisible({ timeout: 15_000 });
});

test("manages Devin customization through documented commands", async ({ page }) => {
  await page.goto("/settings");
  await expect(page.getByText("project-rule", { exact: false })).toBeVisible();
  await page.getByRole("tab", { name: "Plugins" }).click();
  await expect(page.getByText("fixture-tools", { exact: false })).toBeVisible();
  await page.getByLabel("GitHub source or HTTPS Git URL").fill("owner/repo");
  await page.getByText("I reviewed the source, scope, and executable impact.").click();
  await page.getByLabel(/Type INSTALL PLUGIN owner\/repo/).fill("INSTALL PLUGIN owner/repo");
  await page.getByRole("button", { name: "Apply through Devin" }).click();
  await expect(page.getByText("Plugin install completed.")).toBeVisible();
});

test("production load has no console or request failures", async ({ page, request }) => {
  const problems: string[] = [];
  const failedRequests: string[] = [];
  page.on("console", (message) => {
    const isHarnessServiceWorkerBlock = message.text() === "Service Worker registration blocked by Playwright";
    if (!isHarnessServiceWorkerBlock && (message.type() === "error" || message.type() === "warning")) {
      problems.push(message.text());
    }
  });
  page.on("requestfailed", (failed) => failedRequests.push(failed.url()));

  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Your machine" })).toBeVisible();
  await expect(page.locator(".host-card").getByText("Devin ready")).toBeVisible();
  await page.waitForTimeout(300);

  const favicon = await request.get("/favicon.svg");
  expect(favicon.ok()).toBe(true);
  expect(problems).toEqual([]);
  expect(failedRequests).toEqual([]);
});

test("motion has a reduced-motion fallback", async ({ page }) => {
  await page.goto("/");
  expect(await page.locator(".route-stage").evaluate((element) => getComputedStyle(element).animationName)).toBe("route-in");

  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.reload();
  const durationMs = await page.locator(".route-stage").evaluate((element) => {
    const duration = getComputedStyle(element).animationDuration;
    return duration.endsWith("ms") ? Number.parseFloat(duration) : Number.parseFloat(duration) * 1_000;
  });
  expect(durationMs).toBeLessThanOrEqual(0.01);
});

test("primary pages pass automated accessibility checks", async ({ context, request }) => {
  const created = await request.post("/api/v1/local/sessions", { data: { title: "Accessibility session" } });
  expect(created.ok()).toBe(true);
  const session = await created.json() as { session_id: string };

  for (const theme of ["dark", "light"] as const) {
    for (const path of ["/", `/sessions/${session.session_id}`, "/settings"]) {
      const auditPage = await context.newPage();
      await auditPage.emulateMedia({ reducedMotion: "reduce" });
      await auditPage.addInitScript((selectedTheme) => {
        try {
          localStorage.setItem("leave-theme", selectedTheme);
        } catch {
          // about:blank has no storage; the same script runs again for the app origin.
        }
      }, theme);
      await auditPage.goto(path);
      await expect(auditPage.locator("html")).toHaveAttribute("data-theme", theme);
      await expect(auditPage.locator("main")).toBeVisible();
      const expectedTextToken = theme === "light" ? "#18222d" : "#f3f6f9";
      await auditPage.waitForFunction(
        (color) => getComputedStyle(document.documentElement).getPropertyValue("--text").trim() === color,
        expectedTextToken
      );
      const results = await new AxeBuilder({ page: auditPage }).analyze();
      expect(results.violations, `Accessibility violations in ${theme} mode at ${path}`).toEqual([]);
      await auditPage.close();
    }
  }
});
