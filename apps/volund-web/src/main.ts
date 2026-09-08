import "./styles.css";
import "./responsive.css";
import "./phase5.css";
import { mountAdministration } from "./admin";
import { catalogApi, identityApi } from "./api";
import { appViewUrl, readAppView, readModelId } from "./app-state";
import type { AppView } from "./app-state";
import { requireSession } from "./auth";
import { onSessionExpired } from "./session-expiry";
import { mountNavigationScroll } from "./navigation-scroll";
import { mountMockView } from "./mockup";
import { mountRawCatalog } from "./raw-catalog";
import type { CurrentSession } from "./types";

const app = document.querySelector<HTMLDivElement>("#app")!;
void boot();

async function boot(): Promise<void> {
  try {
    const session = await requireSession(app);
    mountApplication(session);
  } catch (error) {
    const message = error instanceof Error ? error.message : "Unbekannter Fehler";
    app.innerHTML = `<main class="access-shell"><section class="access-card"><p class="eyebrow">START FEHLGESCHLAGEN</p><h1>VÖLUND ist nicht bereit</h1><p>${escapeMarkup(message)}</p><button id="retry" class="primary-action">Erneut versuchen</button></section></main>`;
    app.querySelector("#retry")?.addEventListener("click", () => void boot());
  }
}

function mountApplication(session: CurrentSession): void {
  onSessionExpired(() => window.location.reload());
  const canAdminister = session.role === "owner" || session.role === "administrator";
  const canEdit = session.role !== "viewer";
  app.innerHTML = `<header class="topbar">
    <button class="brand" data-view="dashboard" aria-label="VÖLUND Übersicht"><span class="brand-mark">V</span><span><strong>VÖLUND</strong><small>THE SOVEREIGN CAD VAULT</small></span></button>
    <div class="top-actions">${canEdit ? '<button class="top-import" data-view="imports">＋ Importieren</button>' : ""}<div class="health"><span id="health-dot"></span><span id="health-text">Verbindung wird geprüft</span></div><button id="logout" class="account-button">${escapeMarkup(session.displayName)} · Abmelden</button></div>
  </header><div class="app-shell"><aside class="app-nav">
    <div class="nav-group"><p>VÖLUND</p><button data-view="dashboard"><i>◇</i><span>Übersicht</span></button><button data-view="models"><i>⬡</i><span>Modelle</span><b id="nav-model-count">—</b></button><button data-view="collections"><i>▱</i><span>Sammlungen</span></button><button data-view="tags"><i>⌗</i><span>Tags</span></button><button data-view="authors"><i>♙</i><span>Autoren</span></button></div>
    <div class="nav-group"><p>DATEN</p><button data-view="raw"><i>⌁</i><span>Rohdateien</span><b id="nav-file-count">—</b></button>${canEdit ? '<button data-view="imports"><i>⇣</i><span>Importe</span></button><button data-view="import-history"><i>◷</i><span>Importhistorie</span></button>' : ""}</div>
    <div class="nav-group"><p>SYSTEM</p><button data-view="administration"><i>⚙</i><span>${canAdminister ? "Administration" : "Mein Konto"}</span></button></div>
    <div class="nav-foot"><span>${escapeMarkup(session.role.toUpperCase())}</span><p>Angemeldet als ${escapeMarkup(session.email)}. Zugriffe werden serverseitig geprüft.</p></div>
  </aside><main id="workspace" class="workspace"></main></div>`;
  const workspace = app.querySelector<HTMLElement>("#workspace")!;
  mountNavigationScroll(app.querySelector<HTMLElement>(".app-nav")!);
  let currentView: AppView | undefined;
  let currentModelId: string | null = null;
  let disposeView: (() => void) | undefined;
  const navigate = (view: AppView, modelId?: string): void => {
    window.history.pushState({}, "", appViewUrl(view, modelId));
    render(view);
  };
  const render = (view = readAppView(window.location.search)): void => {
    const modelId = readModelId(window.location.search);
    if (view === currentView && modelId === currentModelId) return;
    disposeView?.();
    currentView = view;
    currentModelId = modelId;
    workspace.className = `workspace view-${view}`;
    app.querySelectorAll<HTMLElement>("[data-view]").forEach((element) => {
      const active = element.dataset.view === view || (["model", "model-problems"].includes(view) && element.dataset.view === "models");
      element.classList.toggle("active", active);
    });
    if (view === "raw") disposeView = mountRawCatalog(workspace);
    else if (view === "administration") disposeView = mountAdministration(workspace, session);
    else disposeView = mountMockView(workspace, view, navigate, modelId, canEdit, canAdminister);
  };
  app.querySelectorAll<HTMLElement>("[data-view]").forEach((element) => {
    element.addEventListener("click", () => navigate(element.dataset.view as AppView));
  });
  app.querySelector("#logout")?.addEventListener("click", () => {
    void identityApi.logout().finally(() => window.location.reload());
  });
  window.addEventListener("popstate", () => render());
  void catalogApi.health().then(
    (health) => setHealth(`System bereit · v${health.version}`, true),
    () => setHealth("System nicht erreichbar", false),
  );
  void catalogApi.roots().then((roots) => {
    const total = roots.reduce((sum, root) => sum + root.fileCount - root.missingFileCount, 0);
    app.querySelector<HTMLElement>("#nav-file-count")!.textContent = String(total);
  });
  void catalogApi.models().then((models) => {
    app.querySelector<HTMLElement>("#nav-model-count")!.textContent = String(models.length);
  });
  render();
}

function setHealth(status: string, online: boolean): void {
  app.querySelector<HTMLElement>("#health-text")!.textContent = status;
  app.querySelector("#health-dot")?.classList.toggle("online", online);
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}
