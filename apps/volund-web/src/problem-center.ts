import { catalogApi } from "./api";
import type { ModelProblem } from "./problem-types";
import type { ModelSummary } from "./types";

const severityRank = { info: 0, warning: 1, error: 2 } as const;

export function problemCenterMarkup(problems: ModelProblem[], minimum: ModelProblem["severity"], canEdit: boolean): string {
  const visible = problems.filter((problem) => severityRank[problem.severity] >= severityRank[minimum]);
  const rows = visible.map((problem) => `<article class="problem-row severity-${problem.severity}" data-problem-status="${problem.status}">
    ${canEdit ? `<label class="problem-select"><input type="checkbox" name="problem" value="${escapeMarkup(problem.key)}"><span class="sr-only">Problem auswählen: ${escapeMarkup(problem.message)}</span></label>` : ""}
    <div><p><strong>${severityLabel(problem.severity)}</strong><code>${escapeMarkup(problem.code)}</code><span>${statusLabel(problem.status)}</span></p><h3>${escapeMarkup(problem.message)}</h3><small title="${escapeMarkup(problem.sourceName)}">${escapeMarkup(problem.sourceName)} · Profil ${escapeMarkup(problem.profile)}</small><p>${escapeMarkup(problem.remediation)}</p><nav><a href="${escapeMarkup(problem.sourceUrl)}">Original herunterladen</a>${problem.diagnosticsUrl ? `<a href="${escapeMarkup(problem.diagnosticsUrl)}" target="_blank" rel="noopener">Abgeleitete Diagnose öffnen</a>` : ""}</nav></div>
  </article>`).join("");
  return `<section class="content-section problem-center" aria-labelledby="problem-center-title"><div class="section-heading"><div><p class="eyebrow">PROBLEM-CENTER</p><h2 id="problem-center-title">Konvertierung & Inspektion</h2></div><span>${visible.length} von ${problems.length}</span></div>
    <div class="problem-filters"><label>Mindestschwere<select id="problem-severity"><option value="info"${minimum === "info" ? " selected" : ""}>Hinweis</option><option value="warning"${minimum === "warning" ? " selected" : ""}>Warnung</option><option value="error"${minimum === "error" ? " selected" : ""}>Fehler</option></select></label><label>Status<select id="problem-status-filter"><option value="all">Alle</option><option value="open">Offen</option><option value="ignored">Ignoriert</option><option value="resolved">Erledigt</option></select></label></div>
    ${canEdit ? '<div class="problem-bulk"><button type="button" data-problem-action="resolved">Auswahl erledigen</button><button type="button" data-problem-action="ignored">Auswahl ignorieren</button><button type="button" data-problem-action="open">Auswahl wieder öffnen</button></div>' : ""}
    <div class="problem-list">${rows || '<div class="empty-concept"><h3>Keine Probleme in diesem Filter</h3><p>Originale und verfügbare Vorschauen bleiben direkt im Modell erreichbar.</p></div>'}</div><p class="form-message" aria-live="polite"></p></section>`;
}

export function mountProblemCenter(host: HTMLElement, model: ModelSummary, problems: ModelProblem[], minimum: ModelProblem["severity"], canEdit: boolean): void {
  let current = problems;
  let severity = minimum;
  const render = (): void => {
    host.innerHTML = problemCenterMarkup(current, severity, canEdit);
    bind();
  };
  const bind = (): void => {
    host.querySelector<HTMLSelectElement>("#problem-severity")?.addEventListener("change", (event) => {
      severity = (event.currentTarget as HTMLSelectElement).value as ModelProblem["severity"];
      render();
    });
    host.querySelector<HTMLSelectElement>("#problem-status-filter")?.addEventListener("change", (event) => {
      const status = (event.currentTarget as HTMLSelectElement).value;
      host.querySelectorAll<HTMLElement>(".problem-row").forEach((row) => { row.hidden = status !== "all" && row.dataset.problemStatus !== status; });
    });
    host.querySelectorAll<HTMLButtonElement>("[data-problem-action]").forEach((button) => button.addEventListener("click", async () => {
      const keys = [...host.querySelectorAll<HTMLInputElement>('input[name="problem"]:checked')].map((input) => input.value);
      const message = host.querySelector<HTMLElement>(".form-message")!;
      if (keys.length === 0) { message.textContent = "Wähle mindestens ein Problem aus."; return; }
      button.disabled = true; message.textContent = "Problemstatus wird revisionssicher gespeichert …";
      try {
        current = await catalogApi.updateModelProblems(model, keys, button.dataset.problemAction as ModelProblem["status"]);
        render();
      } catch (error) {
        button.disabled = false; message.classList.add("error"); message.textContent = error instanceof Error ? error.message : "Problemstatus konnte nicht gespeichert werden.";
      }
    }));
  };
  render();
}

function severityLabel(value: ModelProblem["severity"]): string {
  return value === "error" ? "FEHLER" : value === "warning" ? "WARNUNG" : "HINWEIS";
}

function statusLabel(value: ModelProblem["status"]): string {
  return value === "ignored" ? "Ignoriert" : value === "resolved" ? "Erledigt" : "Offen";
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
