import { catalogApi } from "./api";
import type { CatalogHistoryEvent } from "./types";

export function historyDetails(targetType: CatalogHistoryEvent["targetType"], targetId: string, label = "Vollständiger Verlauf"): string {
  return `<details class="catalog-history" data-history-type="${escapeMarkup(targetType)}" data-history-id="${escapeMarkup(targetId)}"><summary>${escapeMarkup(label)}</summary><div data-history-items><p>Beim Öffnen wird der aktuelle Verlauf geladen.</p></div></details>`;
}

export function bindCatalogHistories(host: HTMLElement): void {
  host.querySelectorAll<HTMLDetailsElement>("[data-history-type]").forEach((details) => {
    details.addEventListener("toggle", () => {
      if (!details.open || details.dataset.loaded === "true") return;
      const target = details.querySelector<HTMLElement>("[data-history-items]")!;
      target.innerHTML = '<p class="loading-copy">Verlauf wird geladen …</p>';
      void catalogApi.history(
        details.dataset.historyType as CatalogHistoryEvent["targetType"],
        details.dataset.historyId!,
      ).then((page) => {
        details.dataset.loaded = "true";
        target.innerHTML = historyMarkup(page.items);
      }).catch((error: unknown) => {
        target.innerHTML = `<p class="form-message error" role="alert">${escapeMarkup(error instanceof Error ? error.message : "Verlauf nicht verfügbar.")}</p>`;
      });
    });
  });
}

export function historyMarkup(items: CatalogHistoryEvent[]): string {
  if (items.length === 0) return "<p>Noch keine Ereignisse für diese Ressource.</p>";
  return `<ol class="catalog-history-list">${items.map((item) => `<li><div><strong>${escapeMarkup(actionLabel(item.action))}</strong><span>${escapeMarkup(item.actorName || "System")} · ${escapeMarkup(new Intl.DateTimeFormat("de-DE", { dateStyle: "medium", timeStyle: "short" }).format(new Date(item.timestampUnixMs)))}</span></div><span class="history-outcome ${escapeMarkup(item.outcome)}">${escapeMarkup(outcomeLabel(item.outcome))}</span><small>Ziel: ${escapeMarkup(item.targetName || item.targetId || "nicht mehr vorhanden")} · ${escapeMarkup(item.targetType)}${item.revision === null ? "" : ` · Revision ${item.revision}`}</small>${summaryMarkup(item.summary)}</li>`).join("")}</ol>`;
}

const SUMMARY_LABELS: Record<string, string> = {
  fields: "Geänderte Felder", revision: "Revision", planId: "Plan-ID", code: "Ergebniscode",
  kind: "Art", candidateId: "Kandidat", childModelId: "Untermodell", collectionId: "Sammlung",
  tagId: "Tag", sourceTagId: "Quell-Tag", targetTagId: "Ziel-Tag", sourceAuthorId: "Quellautor",
  targetAuthorId: "Zielautor", modelAction: "Modellaktion", totalFiles: "Dateien gesamt",
  createdFiles: "Neu angelegt", reusedFiles: "Wiederverwendet", relocatedFiles: "Verschoben",
  membershipsRemoved: "Entfernte Zuordnungen", modelsReassigned: "Neu zugeordnete Modelle",
};

function summaryMarkup(summary: Record<string, unknown>): string {
  const entries = Object.entries(summary);
  if (entries.length === 0) return '<p class="history-empty-summary">Keine zusätzlichen Änderungsdetails.</p>';
  return `<dl class="history-summary">${entries.map(([key, value]) => `<div><dt>${escapeMarkup(SUMMARY_LABELS[key] || key)}</dt><dd>${escapeMarkup(summaryValue(value))}</dd></div>`).join("")}</dl>`;
}

function summaryValue(value: unknown): string {
  if (Array.isArray(value)) return value.map(summaryValue).join(", ");
  if (value && typeof value === "object") return Object.entries(value).map(([key, nested]) => `${key}: ${summaryValue(nested)}`).join("; ");
  if (typeof value === "boolean") return value ? "Ja" : "Nein";
  if (value === null || value === undefined || value === "") return "—";
  return String(value);
}

function actionLabel(action: string): string {
  return action.replaceAll(".", " · ");
}

function outcomeLabel(outcome: CatalogHistoryEvent["outcome"]): string {
  return ({ success: "Erfolgreich", denied: "Abgelehnt", failure: "Fehlgeschlagen" })[outcome];
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
