import { catalogApi } from "./api";
import type { ModelHistoryEvent } from "./types";

export function mountModelHistory(host: HTMLElement, modelId: string): void {
  const target = host.querySelector<HTMLElement>("#model-history");
  if (!target) return;
  void load(target, modelId, 0);
}

async function load(target: HTMLElement, modelId: string, offset: number): Promise<void> {
  target.innerHTML = '<p class="loading-copy">Änderungsverlauf wird geladen …</p>';
  try {
    const page = await catalogApi.modelHistory(modelId, offset);
    target.innerHTML = historyMarkup(page.items) + pagination(page.offset, page.limit, page.total);
    target.querySelector<HTMLButtonElement>("[data-history-page=previous]")?.addEventListener("click", () => void load(target, modelId, Math.max(0, offset - page.limit)));
    target.querySelector<HTMLButtonElement>("[data-history-page=next]")?.addEventListener("click", () => void load(target, modelId, offset + page.limit));
  } catch (error) {
    target.innerHTML = `<div class="empty-concept"><h3>Verlauf nicht verfügbar</h3><p>${escapeMarkup(message(error))}</p><button id="retry-history" class="secondary-action">Erneut versuchen</button></div>`;
    target.querySelector("#retry-history")?.addEventListener("click", () => void load(target, modelId, offset));
  }
}

export function historyMarkup(events: ModelHistoryEvent[]): string {
  if (events.length === 0) return '<div class="empty-concept"><h3>Noch keine Änderungen</h3><p>Neue katalogisierte Änderungen erscheinen hier.</p></div>';
  return `<ol class="history-list">${events.map((event) => `<li><div><strong>${escapeMarkup(actionLabel(event.action))}</strong><span>${escapeMarkup(event.actorDisplayName || "System")} · ${escapeMarkup(formatTime(event.occurredAtUnixMs))}</span></div><p>${escapeMarkup(changeLabel(event.change))}</p></li>`).join("")}</ol>`;
}

function actionLabel(action: string): string {
  return ({
    "model.create": "Modell angelegt", "model.update": "Modelldaten geändert",
    "model.thumbnail.update": "Vorschaubild geändert", "model.component.add": "Komponente verknüpft",
    "model.component.remove": "Komponente entfernt", "model.collection.add": "Sammlung hinzugefügt",
    "model.collection.remove": "Aus Sammlung entfernt", "model.collection.clear": "Sammlung aufgelöst",
    "model.tag.merge": "Tag zusammengeführt", "model.tag.remove": "Tag entfernt",
    "model.author.merge": "Autor zusammengeführt", "model.import.commit": "Import veröffentlicht",
  } as Record<string, string>)[action] || "Katalog geändert";
}

function changeLabel(change: Record<string, unknown>): string {
  const fields = Array.isArray(change.fields) ? `Felder: ${change.fields.join(", ")}` : "";
  const counts = ["createdFiles", "reusedFiles", "relocatedFiles"].filter((key) => key in change)
    .map((key) => `${String(change[key])} ${key === "createdFiles" ? "neu" : key === "reusedFiles" ? "wiederverwendet" : "verschoben"}`).join(" · ");
  const kind = typeof change.kind === "string" ? `Auswahl: ${change.kind}` : "";
  const revision = typeof change.revision === "number" ? `Revision ${change.revision}` : "";
  return [fields, counts, kind, revision].filter(Boolean).join(" · ") || "Beziehung aktualisiert";
}

function pagination(offset: number, limit: number, total: number): string {
  if (total <= limit) return "";
  return `<nav class="pagination" aria-label="Verlaufsseiten"><button class="secondary-action" data-history-page="previous"${offset === 0 ? " disabled" : ""}>← Zurück</button><span>${offset + 1}–${Math.min(offset + limit, total)} von ${total}</span><button class="secondary-action" data-history-page="next"${offset + limit >= total ? " disabled" : ""}>Weiter →</button></nav>`;
}
function formatTime(timestamp: number): string { return new Intl.DateTimeFormat("de-DE", { dateStyle: "medium", timeStyle: "short" }).format(new Date(timestamp)); }
function message(error: unknown): string { return error instanceof Error ? error.message : "Verlauf konnte nicht geladen werden."; }
function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
