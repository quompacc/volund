import { jobsApi } from "./api";
import { confirmExact } from "./confirmation";
import type { JobPage, ManagedJob } from "./types";

type JobFilters = { kind?: string; status?: string; offset?: number };

export function jobPanel(page: JobPage, filters: JobFilters, locale: string, timeZone: string): string {
  const visible = page.items.slice(0, 8);
  const remaining = page.items.slice(8);
  return `<section id="job-panel" class="admin-section" aria-labelledby="jobs-heading">
    <div class="section-heading"><div><p class="eyebrow">AUFTRÄGE</p><h2 id="jobs-heading">Hintergrundarbeit</h2></div><span>${page.total}</span></div>
    <form id="job-filters" class="admin-form" aria-label="Aufträge filtern">
      <label><span>Auftragsart</span><select name="kind"><option value="">Alle Arten</option>${option("scan", "Scans", filters.kind)}${option("conversion", "Vorschauen", filters.kind)}</select></label>
      <label><span>Status</span><select name="status"><option value="">Alle Zustände</option>${["queued", "running", "completed", "ready", "failed", "cancelled", "timed-out"].map((status) => option(status, statusLabel(status), filters.status)).join("")}</select></label>
      <button class="secondary-action">Anwenden</button>
    </form>
    <div class="admin-list">${page.items.length === 0 ? '<p class="admin-copy">Keine passenden Aufträge.</p>' : visible.map((job) => jobRow(job, locale, timeZone)).join("")}</div>
    ${remaining.length ? `<details class="admin-job-overflow"><summary>Weitere ${remaining.length} Aufträge anzeigen</summary><div class="admin-list">${remaining.map((job) => jobRow(job, locale, timeZone)).join("")}</div></details>` : ""}
    ${page.total > page.limit ? `<nav class="pagination" aria-label="Auftragsseiten"><button type="button" class="secondary-action" data-job-page="previous" data-job-offset="${Math.max(0, page.offset - page.limit)}"${page.offset === 0 ? " disabled" : ""}>← Zurück</button><span data-job-page-info>${page.offset + 1}–${Math.min(page.offset + page.limit, page.total)} von ${page.total}</span><button type="button" class="secondary-action" data-job-page="next" data-job-offset="${page.offset + page.limit}"${page.offset + page.limit >= page.total ? " disabled" : ""}>Weiter →</button></nav>` : ""}
  </section>`;
}

export function bindJobActions(
  host: HTMLElement,
  setFilters: (filters: JobFilters) => void,
  refresh: () => Promise<void>,
): void {
  host.querySelector<HTMLFormElement>("#job-filters")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(event.currentTarget as HTMLFormElement);
    const kind = String(data.get("kind") || "");
    const status = String(data.get("status") || "");
    setFilters({ ...(kind ? { kind } : {}), ...(status ? { status } : {}), offset: 0 });
    void refresh();
  });
  host.querySelectorAll<HTMLButtonElement>("[data-job-page]").forEach((button) => {
    button.addEventListener("click", () => {
      if (button.disabled) return;
      button.disabled = true;
      const offset = Number(button.dataset.jobOffset);
      const form = host.querySelector<HTMLFormElement>("#job-filters")!;
      const data = new FormData(form);
      const kind = String(data.get("kind") || "");
      const status = String(data.get("status") || "");
      setFilters({ ...(kind ? { kind } : {}), ...(status ? { status } : {}), offset });
      void refresh();
    });
  });
  host.querySelectorAll<HTMLButtonElement>("[data-job-action]").forEach((button) => {
    button.addEventListener("click", () => {
      const job = JSON.parse(button.dataset.job!) as ManagedJob;
      const action = button.dataset.jobAction === "retry" ? "RETRY" : "CANCEL";
      const expected = `${action} ${job.id}`;
      button.disabled = true;
      void confirmExact(expected, action === "RETRY"
        ? "Der Auftrag wird erneut eingereiht."
        : "Der Auftrag wird abgebrochen; laufende Arbeit endet kooperativ.")
        .then((confirmation) => action === "RETRY" ? jobsApi.retry(job, confirmation) : jobsApi.cancel(job, confirmation))
        .then(refresh)
        .catch((error: unknown) => showJobError(host, error))
        .finally(() => { button.disabled = false; });
    });
  });
}

export function jobRow(job: ManagedJob, locale: string, timeZone: string): string {
  const total = job.progressTotal;
  const progress = total === null ? `${job.progressCurrent}` : `${job.progressCurrent}/${total}`;
  const progressLabel = job.kind === "scan"
    ? `Neu gehashte Dateien: ${job.progressCurrent}${total === null ? "" : ` · Entdeckte Dateien: ${total}`}`
    : total === null ? "Fortschritt nicht verfügbar" : `Fortschritt ${progress}`;
  const cancellation = job.cancellationRequestedAtUnixMs === null ? "" : " · Abbruch angefordert";
  const diagnostic = job.diagnostic ? `<small class="admin-message error">${escapeMarkup(job.diagnostic)}</small>` : "";
  return `<article><div><strong>${escapeMarkup(job.title)}</strong><small>${job.kind === "scan" ? "Scan" : "Vorschau"} · ${escapeMarkup(job.context)}${job.profile ? ` · ${escapeMarkup(job.profile)}` : ""} · Versuch ${job.attempt} · ${progressLabel} · ${formatTime(job.requestedAtUnixMs, locale, timeZone)}${cancellation}</small>${diagnostic}</div>
    <span class="role-badge state-${jobState(job.status)}">${escapeMarkup(statusLabel(job.status).toUpperCase())}</span>
    ${job.canRetry ? actionButton(job, "retry", "Wiederholen") : ""}${job.canCancel ? actionButton(job, "cancel", "Abbrechen") : ""}</article>`;
}

function actionButton(job: ManagedJob, action: "retry" | "cancel", label: string): string {
  return `<button class="secondary-action" data-job-action="${action}" data-job='${escapeAttribute(JSON.stringify(job))}'>${label}</button>`;
}

function option(value: string, label: string, selected?: string): string {
  return `<option value="${value}"${value === selected ? " selected" : ""}>${label}</option>`;
}

function jobState(status: string): "healthy" | "degraded" | "blocked" {
  if (["failed", "timed-out"].includes(status)) return "blocked";
  if (["queued", "running", "cancelled"].includes(status)) return "degraded";
  return "healthy";
}

function statusLabel(status: string): string {
  return ({ queued: "Eingeplant", running: "Läuft", completed: "Abgeschlossen", ready: "Bereit", partial: "Teilweise", failed: "Fehlgeschlagen", cancelled: "Abgebrochen", "timed-out": "Zeitlimit" } as Record<string, string>)[status] ?? status;
}

function formatTime(timestamp: number, locale: string, timeZone: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short", timeZone }).format(timestamp);
}

function showJobError(host: HTMLElement, error: unknown): void {
  const panel = host.querySelector<HTMLElement>("#job-panel");
  if (!panel) return;
  const message = document.createElement("p");
  message.className = "admin-message error";
  message.textContent = error instanceof Error ? error.message : "Auftragsaktion fehlgeschlagen";
  panel.prepend(message);
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function escapeAttribute(value: string): string {
  return escapeMarkup(value).replaceAll("'", "&#39;");
}
