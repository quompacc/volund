const webdriverUrl = process.env.WEBDRIVER_URL ?? "http://127.0.0.1:14444";
const viteUrl = process.env.VITE_URL ?? "http://127.0.0.1:4191";
const firefoxBinary = process.env.FIREFOX_BINARY;
let sessionId;

async function command(method, path, body) {
  const response = await fetch(`${webdriverUrl}${path}`, {
    method,
    headers: body === undefined ? {} : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const payload = await response.json();
  if (!response.ok || payload.value?.error) {
    throw new Error(`${method} ${path}: ${payload.value?.message ?? response.statusText}`);
  }
  return payload.value;
}

async function execute(script) {
  return command("POST", `/session/${sessionId}/execute/sync`, { script, args: [] });
}

async function navigate(path) {
  await command("POST", `/session/${sessionId}/url`, { url: `${viteUrl}${path}` });
}

async function waitFor(expression, label) {
  const script = `
    const done = arguments[arguments.length - 1];
    const end = Date.now() + 15000;
    const check = () => {
      try {
        const value = (${expression});
        if (value) return done(value);
      } catch {}
      if (Date.now() >= end) return done(null);
      setTimeout(check, 50);
    };
    check();`;
  const value = await command("POST", `/session/${sessionId}/execute/async`, { script, args: [] });
  if (!value) throw new Error(`Zeitüberschreitung: ${label}`);
  return value;
}

async function waitForSettledScroll(predicate, label) {
  const script = `
    const done = arguments[arguments.length - 1];
    const end = Date.now() + 15000;
    let previous = null;
    let stable = 0;
    const check = () => {
      const value = document.querySelector('.app-nav').scrollLeft;
      stable = value === previous ? stable + 1 : 0;
      previous = value;
      if ((${predicate}) && stable >= 3) return done(value);
      if (Date.now() >= end) return done(null);
      setTimeout(check, 50);
    };
    check();`;
  const value = await command("POST", `/session/${sessionId}/execute/async`, { script, args: [] });
  if (value === null) throw new Error(`Zeitüberschreitung: ${label}`);
  return value;
}

async function setBrowserZoom(value) {
  await command("POST", `/session/${sessionId}/moz/context`, { context: "chrome" });
  try {
    await execute(`FullZoom.setZoom(${value}); return null;`);
  } finally {
    await command("POST", `/session/${sessionId}/moz/context`, { context: "content" });
  }
}

async function pointerTap(pointerType, x, y) {
  await command("POST", `/session/${sessionId}/actions`, {
    actions: [{
      type: "pointer",
      id: `${pointerType}-pointer`,
      parameters: { pointerType },
      actions: [
        { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(x), y: Math.round(y) },
        { type: "pointerDown", button: 0 },
        { type: "pause", duration: 80 },
        { type: "pointerUp", button: 0 },
      ],
    }],
  });
  await command("DELETE", `/session/${sessionId}/actions`);
}

async function keyPress(value) {
  await command("POST", `/session/${sessionId}/actions`, {
    actions: [{
      type: "key",
      id: "keyboard",
      actions: [{ type: "keyDown", value }, { type: "keyUp", value }],
    }],
  });
  await command("DELETE", `/session/${sessionId}/actions`);
}

async function readyLayout() {
  return waitFor(`(() => {
    const nav = document.querySelector('.app-nav');
    const page = document.querySelector('.page-scroll, .catalog-panel');
    if (!nav || !page || document.querySelector('.loading-copy')) return null;
    const header = document.querySelector('.topbar');
    return {
      fixtureError: document.documentElement.dataset.fixtureError ?? null,
      contentOverflow: page.scrollWidth - page.clientWidth,
      headerOverflow: header.scrollWidth - header.clientWidth,
      navOverflow: nav.scrollWidth - nav.clientWidth,
      innerWidth,
      ratio: devicePixelRatio,
    };
  })()`, "Anwendungslayout");
}

function assertLayout(layout, label) {
  if (layout.fixtureError) throw new Error(`${label}: Fixture fehlt: ${layout.fixtureError}`);
  if (layout.contentOverflow > 1) throw new Error(`${label}: Inhaltüberlauf ${layout.contentOverflow}px`);
  if (layout.headerOverflow > 1) throw new Error(`${label}: Kopfüberlauf ${layout.headerOverflow}px`);
}

async function elementCenter(selector, text) {
  const encodedSelector = JSON.stringify(selector);
  const encodedText = JSON.stringify(text ?? null);
  return execute(`
    const candidates = [...document.querySelectorAll(${encodedSelector})];
    const element = ${encodedText} === null
      ? candidates[0]
      : candidates.find(candidate => candidate.textContent.trim().includes(${encodedText}));
    if (!element) return null;
    element.scrollIntoView({ block: 'center', inline: 'nearest' });
    const rect = element.getBoundingClientRect();
    return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 };
  `);
}

async function runLayoutMatrix() {
  await navigate("/tests/responsive-browser.html");
  const script = `
    const done = arguments[arguments.length - 1];
    document.querySelector('#run').click();
    const end = Date.now() + 240000;
    const check = () => {
      const text = document.querySelector('#result').textContent;
      if (document.documentElement.dataset.matrixComplete === 'true') return done(text);
      if (Date.now() >= end) return done('ZEITUEBERSCHREITUNG\\n' + text);
      setTimeout(check, 100);
    };
    check();`;
  const result = await command("POST", `/session/${sessionId}/execute/async`, { script, args: [] });
  const lines = result.split("\n");
  const failures = lines.filter((line) => line.startsWith("FEHLER") || line.startsWith("ZEIT"));
  if (failures.length || lines.length !== 119) {
    throw new Error(`Firefox-Layoutmatrix: ${lines.length} Zeilen, ${failures.join(" | ")}`);
  }
  return lines.length;
}

async function runZoomMatrix() {
  await command("POST", `/session/${sessionId}/window/rect`, { width: 1440, height: 900, x: 0, y: 0 });
  await setBrowserZoom(2);
  const ratios = [];
  const views = [
    "dashboard", "models", "collections", "tags", "authors", "imports",
    "import-history", "administration", "model", "model-problems", "raw",
  ];
  for (const view of views) {
    const model = view.startsWith("model") ? "&model=00000000-0000-4000-8000-000000000001" : "";
    await navigate(`/tests/responsive-surface.html?view=${view}${model}`);
    const layout = await readyLayout();
    assertLayout(layout, `Firefox 200 % ${view}`);
    if (layout.ratio < 1.9) throw new Error(`Firefox-Vollzoom nicht aktiv: ${layout.ratio}`);
    ratios.push(layout.ratio);
  }
  await setBrowserZoom(1);
  return { views: views.length, minimumRatio: Math.min(...ratios) };
}

async function runPointerAndHistoryChecks() {
  await command("POST", `/session/${sessionId}/window/rect`, { width: 500, height: 900, x: 0, y: 0 });
  await navigate("/tests/responsive-surface.html?view=dashboard");
  const layout = await readyLayout();
  if (layout.navOverflow <= 0) throw new Error("Schmale Navigation läuft nicht horizontal über");

  const right = await elementCenter('.nav-scroll-hint button[aria-label="Navigation nach rechts scrollen"]');
  await pointerTap("touch", right.x, right.y);
  const afterTouch = await waitForSettledScroll("value > 0", "Touch-Scroll");

  const left = await elementCenter('.nav-scroll-hint button[aria-label="Navigation nach links scrollen"]');
  await pointerTap("mouse", left.x, left.y);
  const afterMouse = await waitForSettledScroll(`value < ${afterTouch}`, "Maus-Scroll");

  await pointerTap("touch", right.x, right.y);
  await pointerTap("touch", right.x, right.y);
  const administration = await elementCenter('[data-view="administration"]');
  await pointerTap("touch", administration.x, administration.y);
  await waitFor(`location.search.includes('view=administration')`, "Touch-Navigation");

  const models = await elementCenter('[data-view="models"]');
  await pointerTap("mouse", models.x, models.y);
  await waitFor(`location.search.includes('view=models')`, "Maus-Navigation");
  await command("POST", `/session/${sessionId}/back`, {});
  await waitFor(`location.search.includes('view=administration')`, "Browser zurück");
  await command("POST", `/session/${sessionId}/forward`, {});
  await waitFor(`location.search.includes('view=models')`, "Browser vorwärts");
  return { afterTouch, afterMouse };
}

async function runDialogChecks() {
  await navigate("/tests/responsive-surface.html?view=administration");
  await readyLayout();
  const storage = await elementCenter("[data-admin-area=storage]");
  await pointerTap("touch", storage.x, storage.y);
  await waitFor(`document.querySelector('[data-admin-area=storage]').getAttribute('aria-selected') === 'true'`, "Speichertab");
  const rename = await elementCenter("button", "Umbenennen");
  await pointerTap("mouse", rename.x, rename.y);
  await waitFor(`document.querySelector('dialog[open]') && true`, "Umbenennen-Dialog");
  await keyPress("\uE00C");
  await waitFor(`!document.querySelector('dialog[open]') && document.activeElement?.textContent.includes('Umbenennen')`, "Dialogabbruch und Fokus");

  await navigate("/tests/responsive-surface.html?view=model&model=00000000-0000-4000-8000-000000000001");
  assertLayout(await readyLayout(), "Direkte Modell-URL");
  const edit = await elementCenter("button", "Bearbeiten");
  await pointerTap("mouse", edit.x, edit.y);
  await waitFor(`document.querySelector('dialog[open]') && true`, "Modelldialog");
  await keyPress("\uE00C");
  await waitFor(`!document.querySelector('dialog[open]') && document.activeElement?.textContent.includes('Bearbeiten')`, "Modelldialogabbruch und Fokus");
  return 2;
}

try {
  const options = { args: ["-headless"], prefs: {
    "browser.shell.checkDefaultBrowser": false,
    "dom.w3c_touch_events.enabled": 1,
  } };
  if (firefoxBinary) options.binary = firefoxBinary;
  const created = await command("POST", "/session", {
    capabilities: { alwaysMatch: { browserName: "firefox", "moz:firefoxOptions": options } },
  });
  sessionId = created.sessionId;
  await command("POST", `/session/${sessionId}/timeouts`, { implicit: 0, pageLoad: 300000, script: 300000 });
  const layouts = await runLayoutMatrix();
  const zoom = await runZoomMatrix();
  const pointer = await runPointerAndHistoryChecks();
  const dialogs = await runDialogChecks();
  console.log(JSON.stringify({
    browser: created.capabilities.browserName,
    browserVersion: created.capabilities.browserVersion,
    geckodriverVersion: created.capabilities["moz:geckodriverVersion"],
    layouts, zoom, dialogs, pointer,
  }, null, 2));
} finally {
  if (sessionId) await command("DELETE", `/session/${sessionId}`).catch(() => {});
}
