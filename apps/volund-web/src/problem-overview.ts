import { pagerMarkup } from "./model-list-view";

/** One entry of `GET /api/v1/problems` (ADR 0033). */
export interface OverviewProblem {
  key: string;
  scope: "model" | "library";
  modelId: string | null;
  modelName: string | null;
  libraryKey: string | null;
  severity: "info" | "warning" | "error";
  status: "open" | "ignored" | "resolved";
  code: string;
  message: string;
  sourceName: string | null;
  occurredAtUnixMs: number;
  remediation: string;
}

export interface ProblemOverviewPage {
  items: OverviewProblem[];
  limit: number;
  offset: number;
  total: number;
  counts: { error: number; warning: number; info: number };
  truncated: boolean;
}

export interface ProblemFilter {
  severity: "info" | "warning" | "error";
  status: "open" | "ignored" | "resolved" | "all";
  page: number;
}

export const PROBLEM_PAGE_SIZE = 50;
const SEVERITY_LABEL = { error: "Fehler", warning: "Warnung", info: "Hinweis" } as const;

export function problemQuery(filter: ProblemFilter): string {
  const parameters = new URLSearchParams({
    severity: filter.severity, status: filter.status,
    limit: String(PROBLEM_PAGE_SIZE), offset: String((filter.page - 1) * PROBLEM_PAGE_SIZE),
  });
  return `?${parameters}`;
}

/** Navigation badge: open errors and warnings, or nothing when there are none. */
export function problemBadge(page: ProblemOverviewPage): string {
  const count = page.counts.error + page.counts.warning;
  return count > 0 ? String(count) : "";
}

export function problemItemMarkup(problem: OverviewProblem): string {
  const target = problem.scope === "library"
    ? `<button type="button" class="text-action" data-problem-library="${escapeMarkup(problem.libraryKey ?? "")}">Bibliothek ${escapeMarkup(problem.libraryKey ?? "")}</button>`
    : `<button type="button" class="text-action" data-problem-model="${escapeMarkup(problem.modelId ?? "")}">${escapeMarkup(problem.modelName ?? "Modell")}</button>`;
  const when = problem.occurredAtUnixMs > 0 ? new Date(problem.occurredAtUnixMs).toLocaleString("de-DE") : "";
  const status = problem.status === "open" ? "" : ` · ${problem.status === "ignored" ? "ignoriert" : "erledigt"}`;
  return `<li class="problem-overview-item tone-${problem.severity}">
    <span class="problem-severity">${SEVERITY_LABEL[problem.severity]}${status}</span>
    <div><strong>${escapeMarkup(problem.message)}</strong>
      <p>${target}${problem.sourceName ? ` · <code>${escapeMarkup(problem.sourceName)}</code>` : ""} · <code>${escapeMarkup(problem.code)}</code>${when ? ` · ${escapeMarkup(when)}` : ""}</p>
      <p class="problem-remediation">${escapeMarkup(problem.remediation)}</p></div></li>`;
}

export function problemOverviewMarkup(page: ProblemOverviewPage, filter: ProblemFilter): string {
  const counts = `<p class="problem-counts">${page.counts.error} Fehler · ${page.counts.warning} Warnungen · ${page.counts.info} Hinweise</p>`;
  const truncated = page.truncated ? '<p class="form-message">Es wurden nur die zuletzt bearbeiteten 2000 Modelle mit Diagnosen geprüft.</p>' : "";
  const list = page.items.length > 0
    ? `<ul class="problem-overview">${page.items.map(problemItemMarkup).join("")}</ul>`
    : '<div class="empty-concept"><h2>Keine Probleme in dieser Auswahl</h2><p>Schweregrad oder Status anpassen, um weitere Einträge zu sehen.</p></div>';
  return `${counts}${truncated}${list}${pagerMarkup(filter.page, page.total, PROBLEM_PAGE_SIZE).replaceAll("data-model-page", "data-problem-page")}`;
}

/** The central problem view across all models and libraries. */
export function mountProblemOverview(
  host: HTMLElement,
  load: (query: string) => Promise<ProblemOverviewPage>,
  open: { model: (id: string) => void; library: () => void },
): () => void {
  let filter: ProblemFilter = { severity: "warning", status: "open", page: 1 };
  let sequence = 0;
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">QUALITÄT</p><h1>Probleme</h1>
    <p>Konvertierungsdiagnosen, fehlende Originale und fehlgeschlagene Scans aller Modelle und Bibliotheken.</p></div></header>
    <section class="content-section"><form id="problem-filters" class="model-list-filters">
      <label>Schweregrad<select name="severity"><option value="error">Nur Fehler</option><option value="warning" selected>Warnungen und Fehler</option><option value="info">Alle</option></select></label>
      <label>Status<select name="status"><option value="open" selected>Offen</option><option value="ignored">Ignoriert</option><option value="resolved">Erledigt</option><option value="all">Alle</option></select></label>
    </form><div id="problem-overview-results" aria-live="polite"><p class="loading-copy">Probleme werden geladen …</p></div></section></div>`;
  const results = host.querySelector<HTMLElement>("#problem-overview-results")!;
  const form = host.querySelector<HTMLFormElement>("#problem-filters")!;
  const render = async () => {
    const current = ++sequence;
    try {
      const page = await load(problemQuery(filter));
      if (current === sequence) results.innerHTML = problemOverviewMarkup(page, filter);
    } catch (error) {
      if (current === sequence) results.innerHTML = `<div class="empty-concept"><h2>Probleme konnten nicht geladen werden</h2><p>${escapeMarkup(error instanceof Error ? error.message : "Unbekannter Fehler")}</p></div>`;
    }
  };
  form.addEventListener("change", () => {
    filter = { severity: form.severity.value, status: form.status.value, page: 1 };
    void render();
  });
  results.addEventListener("click", (event) => {
    const target = event.target as HTMLElement;
    const model = target.closest<HTMLElement>("[data-problem-model]")?.dataset.problemModel;
    if (model) { open.model(model); return; }
    if (target.closest("[data-problem-library]")) { open.library(); return; }
    const page = target.closest<HTMLButtonElement>("[data-problem-page]");
    if (page && !page.disabled) { filter = { ...filter, page: Number(page.dataset.problemPage) }; void render(); }
  });
  void render();
  return () => { sequence += 1; };
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
