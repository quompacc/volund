import { catalogApi } from "./api";
import type { ModelFile, Preview } from "./types";

export function previewInspectionMarkup(file: ModelFile, previews: Preview[], canEdit: boolean): string {
  const recent = previews.slice(0, 10);
  const rows = recent.map((preview) => `<article><div><strong>${statusLabel(preview.status)}</strong><span>Profil ${escapeMarkup(preview.profile)} · Konverter ${escapeMarkup(preview.converterVersion)}</span></div><small>${preview.finishedAtUnixMs ? new Date(preview.finishedAtUnixMs).toLocaleString("de-DE") : "In Bearbeitung"}</small><nav>${preview.artifacts.map((artifact) => `<a href="${escapeMarkup(artifact.url)}" target="_blank" rel="noopener">${artifactLabel(artifact.kind)} · abgeleitet</a>`).join("")}</nav></article>`).join("");
  return `<section class="preview-inspection" aria-labelledby="preview-inspection-title"><h3 id="preview-inspection-title">Vorschau-Lebenszyklus</h3><p><strong>Quelle:</strong> ${escapeMarkup(file.path)}. Abgeleitete Artefakte sind unveränderliche Cache-Ausgaben; das Original bleibt separat erreichbar.</p>${canEdit ? '<div class="preview-profile-actions"><button type="button" data-preview-profile="web">Web-Profil anfordern</button><button type="button" data-preview-profile="fine">Fein-Profil anfordern</button></div>' : ""}<div class="preview-run-list">${rows || '<p>Noch kein Vorschaulauf. Fehler verdecken oder verändern das Original nicht.</p>'}</div><p class="form-message" aria-live="polite"></p></section>`;
}

export async function mountPreviewInspection(host: HTMLElement, file: ModelFile, canEdit: boolean): Promise<void> {
  let previews = (await catalogApi.previews(file.id)).items;
  const render = (): void => {
    host.innerHTML = previewInspectionMarkup(file, previews, canEdit);
    host.querySelectorAll<HTMLButtonElement>("[data-preview-profile]").forEach((button) => button.addEventListener("click", async () => {
      const message = host.querySelector<HTMLElement>(".form-message")!;
      button.disabled = true; message.textContent = "Vorschauanforderung wird geprüft …";
      try {
        const request = await catalogApi.enqueuePreview(file.id, button.dataset.previewProfile as "web" | "fine");
        message.textContent = request.status === "ready" ? "Passender unveränderlicher Cache wird wiederverwendet." : "Vorschau ist eingereiht; das Original bleibt verfügbar.";
        previews = (await catalogApi.previews(file.id)).items; render();
      } catch (error) {
        button.disabled = false; message.classList.add("error"); message.textContent = error instanceof Error ? error.message : "Vorschau konnte nicht angefordert werden.";
      }
    }));
  };
  render();
}

function statusLabel(status: string): string {
  return status === "ready" ? "BEREIT" : status === "failed" ? "FEHLER" : status === "running" ? "LÄUFT" : status === "cancelled" ? "ABGEBROCHEN" : "EINGEREIHT";
}
function artifactLabel(kind: string): string {
  return ({ "preview-glb": "3D-Vorschau", "thumbnail-raster": "Rasterbild", "assembly-manifest": "Struktur", diagnostics: "Diagnose", result: "Ergebnis" } as Record<string, string>)[kind] ?? kind;
}
function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;") }
