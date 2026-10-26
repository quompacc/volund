// Large-assembly viewer acceptance: runs tests/viewer-performance-browser.html in
// real browsers and checks its budgets. No WebDriver download is needed:
// Chromium-family browsers are driven over CDP, Firefox over WebDriver BiDi.
//
//   node tests/viewer-performance.mjs [--report results.json]
//
// EDGE_BINARY, CHROME_BINARY, and FIREFOX_BINARY select the browsers; any that
// is unset or missing is skipped. At least one browser must run.
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const viteUrl = "http://127.0.0.1:4191";
const page = "/tests/viewer-performance-browser.html";
const variants = ["", "?unique=1", "?unique=1&detail=high"];
const timeoutMs = 300000;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Wait for a browser to exit, then remove its throwaway profile if possible. */
async function retire(browser, profile) {
  const exited = new Promise((resolve) => { if (browser.exitCode !== null) resolve(); else browser.once("exit", resolve); });
  await Promise.race([exited, sleep(10000)]);
  if (browser.exitCode === null) browser.kill();
  try {
    rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  } catch (error) {
    console.warn(`temporäres Profil blieb liegen: ${profile} (${error.code})`);
  }
}

async function until(read, label, ms = timeoutMs) {
  const end = Date.now() + ms;
  for (;;) {
    try {
      const value = await read();
      if (value) return value;
    } catch { /* not ready yet */ }
    if (Date.now() > end) throw new Error(`timed out: ${label}`);
    await sleep(250);
  }
}

function socket(url) {
  const ws = new WebSocket(url);
  let next = 0;
  const pending = new Map();
  ws.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
    else waiter.resolve(message.result);
  });
  const opened = new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", () => reject(new Error(`cannot connect ${url}`)), { once: true });
  });
  return {
    opened,
    send(method, params = {}) {
      next += 1;
      const id = next;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        ws.send(JSON.stringify({ id, method, params }));
      });
    },
    close: () => ws.close(),
  };
}

async function chromium(binary, name, extra = []) {
  const profile = mkdtempSync(join(tmpdir(), "volund-perf-"));
  const port = 9333;
  const browser = spawn(binary, [
    "--headless=new", `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`,
    "--window-size=1280,900", "--enable-precise-memory-info", "--js-flags=--expose-gc",
    "--no-first-run", "--no-default-browser-check", ...extra, "about:blank",
  ], { stdio: "ignore" });
  let control;
  try {
    const version = await until(() => fetch(`http://127.0.0.1:${port}/json/version`).then((response) => response.ok && response.json()), `${name} start`, 30000);
    control = socket(version.webSocketDebuggerUrl);
    await control.opened;
    const results = [];
    for (const variant of variants) {
      const address = encodeURIComponent(`${viteUrl}${page}${variant}`);
      const target = await (await fetch(`http://127.0.0.1:${port}/json/new?${address}`, { method: "PUT" })).json();
      const cdp = socket(target.webSocketDebuggerUrl);
      await cdp.opened;
      const result = await until(async () => {
        const evaluated = await cdp.send("Runtime.evaluate", { expression: "window.__volundResult ?? null", returnByValue: true });
        return evaluated.result.value;
      }, `${name} ${variant || "repeated"} result`);
      results.push({ browser: name, ...result });
      cdp.close();
      await fetch(`http://127.0.0.1:${port}/json/close/${target.id}`);
    }
    return results;
  } finally {
    await control?.send("Browser.close").catch(() => undefined);
    await retire(browser, profile);
  }
}

async function firefox(binary) {
  const profile = mkdtempSync(join(tmpdir(), "volund-perf-ff-"));
  const port = 9334;
  const browser = spawn(binary, ["--headless", "--no-remote", "--profile", profile,
    `--remote-debugging-port=${port}`, "--width=1280", "--height=900"], { stdio: "ignore" });
  try {
    const bidi = await until(async () => {
      const candidate = socket(`ws://127.0.0.1:${port}/session`);
      await candidate.opened;
      return candidate;
    }, "Firefox start", 30000);
    await bidi.send("session.new", { capabilities: {} });
    const tree = await bidi.send("browsingContext.getTree", {});
    const context = tree.contexts[0].context;
    const results = [];
    for (const variant of variants) {
      await bidi.send("browsingContext.navigate", { context, url: `${viteUrl}${page}${variant}`, wait: "complete" });
      const result = await until(async () => {
        const evaluated = await bidi.send("script.evaluate", {
          expression: "JSON.stringify(window.__volundResult ?? null)", target: { context }, awaitPromise: false,
        });
        return JSON.parse(evaluated.result.value);
      }, `Firefox ${variant || "repeated"} result`);
      results.push({ browser: "Firefox", ...result });
    }
    await bidi.send("browser.close", {}).catch(() => undefined);
    return results;
  } finally {
    await retire(browser, profile);
  }
}

const vite = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--config", "tests/responsive-vite.config.ts"], { stdio: "ignore" });
let failed = false;
try {
  await until(() => fetch(`${viteUrl}${page}`).then((response) => response.ok), "Vite", 30000);
  const runs = [];
  const edge = process.env.EDGE_BINARY ?? "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";
  if (existsSync(edge)) runs.push(...await chromium(edge, "Edge"));
  // Informative lower bound without a GPU: reported, never gating.
  const software = process.env.VOLUND_PERF_SOFTWARE === "1" && existsSync(edge)
    ? (await chromium(edge, "Edge (SwiftShader)", ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"])).map((run) => ({ ...run, informative: true }))
    : [];
  if (process.env.CHROME_BINARY && existsSync(process.env.CHROME_BINARY)) runs.push(...await chromium(process.env.CHROME_BINARY, "Chrome"));
  const gecko = process.env.FIREFOX_BINARY ?? "C:/Program Files/Mozilla Firefox/firefox.exe";
  if (existsSync(gecko)) runs.push(...await firefox(gecko));
  if (runs.length === 0) throw new Error("no browser available");
  for (const run of [...runs, ...software]) {
    const verdict = run.failures?.length ? `FEHLER (${run.failures.join(", ")})` : "BESTANDEN";
    failed ||= !run.informative && Boolean(run.failures?.length);
    console.log(`${run.browser} ${run.variant}: ${verdict} · Laden ${Math.round(run.firstLoadMs)}/${Math.round(run.worstLoadMs)} ms · `
      + `Frame p95 ${run.frameP95Ms?.toFixed(1)} ms · ${run.triangles} Dreiecke · GLB ${(run.glbBytes / 1e6).toFixed(1)} MB · `
      + `Auswahl ${run.selectMs?.toFixed(1)} ms · Baum ${Math.round(run.treeMountMs)} ms · Heap +${run.heapGrowthBytes ?? "n/a"} B · ${run.renderer}`);
  }
  const report = process.argv.indexOf("--report");
  if (report > 0) writeFileSync(process.argv[report + 1], `${JSON.stringify([...runs, ...software], null, 2)}\n`);
} catch (error) {
  failed = true;
  console.error(error);
} finally {
  vite.kill();
}
process.exit(failed ? 1 : 0);
