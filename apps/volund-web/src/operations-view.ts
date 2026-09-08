import { operationsApi } from "./api";
import type { OperationalComponent, OperationalState, OperationsHealth } from "./types";

export function operationsPanel(health: OperationsHealth, locale: string, timeZone: string): string {
  const database = health.database;
  const libraries = countStates(health.libraries.map((library) => library.storage.state));
  return `<section class="admin-section operations-panel" aria-labelledby="operations-heading">
    <div class="section-heading"><div><p class="eyebrow">BETRIEB</p><h2 id="operations-heading">Systemzustand</h2></div>${stateBadge(health.state)}</div>
    <p class="admin-copy">VÖLUND ${escapeMarkup(health.version)} · geprüft ${formatTime(health.checkedAtUnixMs, locale, timeZone)}</p>
    <button id="download-support-bundle" class="secondary-action">Geschwärztes Supportpaket herunterladen</button>
    <div class="admin-list">
      <article><div><strong>Datenbank</strong><small>PostgreSQL ${database.serverVersion} · Migrationen ${database.appliedMigrations}/${database.expectedMigrations} · Tabellen ${database.schemaTables}/${database.expectedSchemaTables}</small></div>${stateBadge(database.state)}</article>
      ${health.workers.map((worker) => componentRow(worker, locale, timeZone)).join("")}
      ${componentRow(health.backup, locale, timeZone)}
      <article><div><strong>Bibliotheksspeicher</strong><small>${health.libraries.length} Wurzeln · ${libraries.healthy} gesund · ${libraries.degraded} eingeschränkt · ${libraries.blocked} blockiert</small></div>${stateBadge(worstState(health.libraries.map((library) => library.storage.state)))}</article>
    </div>
    <div class="section-heading"><div><p class="eyebrow">LETZTE LÄUFE</p><h3>Scans</h3></div><span>${health.recentScans.length}</span></div>
    <div class="admin-list">${health.recentScans.length === 0 ? "<p class=\"admin-copy\">Noch keine Scans vorhanden.</p>" : health.recentScans.map((scan) => `<article><div><strong>${escapeMarkup(scan.libraryKey)}</strong><small>${scan.full ? "Vollständig" : "Inkrementell"} · ${formatTime(scan.requestedAtUnixMs, locale, timeZone)}</small></div><span class="role-badge">${escapeMarkup(scan.hasError ? "FEHLER" : scan.status.toUpperCase())}</span></article>`).join("")}</div>
  </section>`;
}

export function bindOperationsActions(host: HTMLElement): void {
  host.querySelector<HTMLButtonElement>("#download-support-bundle")?.addEventListener("click", (event) => {
    const button = event.currentTarget as HTMLButtonElement;
    button.disabled = true;
    button.textContent = "Supportpaket wird sicher erstellt …";
    void operationsApi.supportBundle().then(({ blob, filename }) => {
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = filename;
      link.click();
      URL.revokeObjectURL(url);
    }).catch((error: unknown) => {
      window.alert(error instanceof Error ? error.message : "Supportpaket fehlgeschlagen");
    }).finally(() => {
      button.disabled = false;
      button.textContent = "Geschwärztes Supportpaket herunterladen";
    });
  });
}

function componentRow(component: OperationalComponent, locale: string, timeZone: string): string {
  const success = component.lastSucceededAtUnixMs === null
    ? "noch nie erfolgreich"
    : `zuletzt erfolgreich ${formatTime(component.lastSucceededAtUnixMs, locale, timeZone)}`;
  const reasons = component.reasons.map(reasonLabel).join(" · ");
  return `<article><div><strong>${componentLabel(component.key)}</strong><small>${success} · Sollintervall ${formatInterval(component.expectedIntervalSeconds)}${reasons ? ` · ${escapeMarkup(reasons)}` : ""}</small></div>${stateBadge(component.state)}</article>`;
}

function formatInterval(seconds: number): string {
  if (seconds % 86400 === 0) return `${seconds / 86400} d`;
  if (seconds % 3600 === 0) return `${seconds / 3600} h`;
  if (seconds % 60 === 0) return `${seconds / 60} min`;
  return `${seconds} s`;
}

function componentLabel(key: string): string {
  return ({ "preview-worker": "Vorschau-Worker", "scan-worker": "Scan-Worker", backup: "Sicherung" } as Record<string, string>)[key] ?? key;
}

function reasonLabel(reason: string): string {
  return ({
    component_never_succeeded: "kein erfolgreicher Lauf erfasst",
    component_last_run_failed: "letzter Lauf fehlgeschlagen",
    component_heartbeat_expired: "Statussignal abgelaufen",
    component_heartbeat_stale: "Statussignal verspätet",
  } as Record<string, string>)[reason] ?? reason;
}

function stateBadge(state: OperationalState): string {
  const label = { healthy: "GESUND", degraded: "EINGESCHRÄNKT", blocked: "BLOCKIERT" }[state];
  return `<span class="role-badge state-${state}">${label}</span>`;
}

function countStates(states: OperationalState[]): Record<OperationalState, number> {
  return states.reduce<Record<OperationalState, number>>((counts, state) => {
    counts[state] += 1;
    return counts;
  }, { healthy: 0, degraded: 0, blocked: 0 });
}

function worstState(states: OperationalState[]): OperationalState {
  if (states.includes("blocked")) return "blocked";
  if (states.includes("degraded")) return "degraded";
  return "healthy";
}

function formatTime(timestamp: number, locale: string, timeZone: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short", timeZone }).format(timestamp);
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}
