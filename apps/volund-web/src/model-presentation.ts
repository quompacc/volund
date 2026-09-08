import { catalogApi } from "./api";
import { formatBytes } from "./format";
import { historyDetails } from "./catalog-history";
import type { ModelProblem } from "./problem-types";
import type { ModelFile, ModelSummary, SlicerTarget, ThumbnailCandidate } from "./types";

export function modelCardMarkup(model: ModelSummary): string {
  const collection = model.collections.join(" · ") || "Ohne Kollektion";
  const formats = model.formats.length > 0 ? model.formats : [model.kind];
  const tags = model.tags.map((tag, index) => `<button type="button" class="tag-chip" data-model-tag-filter="${escapeMarkup(model.tagIds[index] || "")}" aria-label="Nach Tag ${escapeMarkup(tag)} filtern">${escapeMarkup(tag)}</button>`).join("");
  const cover = model.slug.includes("voron") ? "voron" : "forge";
  const coverContent = model.thumbnail.status === "ready" && model.thumbnail.url
    ? `<span class="model-thumbnail-frame"><img class="model-thumbnail-backdrop" src="${escapeMarkup(model.thumbnail.url)}" alt="" aria-hidden="true"><img class="model-thumbnail" data-thumbnail-fit="contain" src="${escapeMarkup(model.thumbnail.url)}" alt="Vorschaubild für ${escapeMarkup(model.name)}"></span>`
    : `<span class="model-thumbnail-placeholder"><i>◇</i><small>Eigenes Bild auswählen</small></span>`;
  const thumbnailStatus = model.thumbnail.status === "fallback" ? '<span class="model-state thumbnail-fallback">Auswahl nicht verfügbar</span>' : model.thumbnail.status === "ready" ? '<span class="model-state"><i></i> Eigenes Vorschaubild</span>' : '<span class="model-state"><i></i> Katalogisiert</span>';
  return `<article class="model-card" data-model="${escapeMarkup(model.id)}"><button class="model-cover cover-${cover}" data-open-model="${escapeMarkup(model.id)}" aria-label="${escapeMarkup(model.name)} öffnen">${coverContent}${thumbnailStatus}</button><div class="model-card-body"><p class="eyebrow">${escapeMarkup(collection)}</p><h3>${escapeMarkup(model.name)}</h3><p>${escapeMarkup(model.description || `${model.fileCount} dauerhaft verknüpfte Projektdateien.`)}</p><div class="format-pills">${formats.map((format) => `<span>${escapeMarkup(format.toUpperCase())}</span>`).join("")}${tags}</div></div><footer><span>${model.fileCount} Dateien</span><span>${escapeMarkup(formatUpdated(model.updatedAtUnixMs))}</span><button data-open-model="${escapeMarkup(model.id)}">Öffnen →</button></footer></article>`;
}

export function modelProblemState(problems: ModelProblem[]): { tone: "ok" | "warning" | "error"; label: string; count: number } {
  const open = problems.filter((problem) => problem.status === "open");
  if (open.some((problem) => problem.severity === "error")) return { tone: "error", label: "Fehler", count: open.length };
  if (open.some((problem) => problem.severity === "warning")) return { tone: "warning", label: "Warnungen", count: open.length };
  return { tone: "ok", label: "Keine Probleme", count: 0 };
}

export function thumbnailPanelMarkup(model: ModelSummary, candidates: ThumbnailCandidate[], canEdit: boolean): string {
  const selected = model.thumbnail.candidateId;
  const visual = model.thumbnail.status === "ready" && model.thumbnail.url
    ? `<img class="selected-thumbnail" src="${escapeMarkup(model.thumbnail.url)}" alt="Ausgewähltes Vorschaubild für ${escapeMarkup(model.name)}">`
    : `<div class="thumbnail-default"><span>◇</span><small>${model.thumbnail.status === "fallback" ? "Die ausgewählte Bildquelle ist nicht mehr verfügbar. Bitte ein anderes echtes Bild wählen." : "Noch kein echtes Vorschaubild ausgewählt. Bis dahin wird ein neutraler Platzhalter gezeigt."}</small></div>`;
  if (!canEdit) return `<section class="thumbnail-panel"><p class="eyebrow">VORSCHAUBILD</p>${visual}</section>`;
  const imageCandidates = candidates.filter((candidate) => candidate.kind === "source-file");
  const unavailable = model.thumbnail.kind === "source-file" && selected && !imageCandidates.some((candidate) => candidate.id === selected)
    ? `<option value="${escapeMarkup(`${model.thumbnail.kind}|${selected}`)}" selected disabled>Ausgewählte Quelle ist nicht verfügbar</option>` : "";
  const options = imageCandidates.map((candidate) => `<option value="${escapeMarkup(`${candidate.kind}|${candidate.id}`)}"${candidate.id === selected ? " selected" : ""}>Bilddatei · ${escapeMarkup(candidate.label)}</option>`).join("");
  return `<section class="thumbnail-panel"><p class="eyebrow">VORSCHAUBILD</p>${visual}<form id="thumbnail-form"><label>Vorschaubild<select class="thumbnail-select" name="thumbnail"><option value=""${model.thumbnail.kind === "default" || model.thumbnail.status === "generated" ? " selected" : ""}>Neutralen Platzhalter verwenden</option>${unavailable}${options}</select></label><button class="secondary-action">Auswahl speichern</button><p id="thumbnail-status" class="form-message" aria-live="polite" tabindex="-1">${imageCandidates.length === 0 ? "Dem Modell ist noch keine verwendbare Bilddatei zugeordnet." : ""}</p></form></section>`;
}

export type ModelFileFilter = "all" | "step" | "stl" | "images" | "documents" | "other";

export function modelFileFilter(file: ModelFile): Exclude<ModelFileFilter, "all"> {
  if (file.format === "step") return "step";
  if (file.format === "stl") return "stl";
  if (file.role === "image") return "images";
  if (file.role === "document") return "documents";
  return "other";
}

export function modelFileFilterOptions(files: ModelFile[]): string {
  const labels: Array<[ModelFileFilter, string]> = [["all", "Alle"], ["step", "STEP"], ["stl", "STL"], ["images", "Bilder"], ["documents", "Dokumente"], ["other", "Weitere"]];
  return labels.map(([value, label]) => `<option value="${value}">${label} · ${value === "all" ? files.length : files.filter((file) => modelFileFilter(file) === value).length}</option>`).join("");
}

export function modelFilesMarkup(files: ModelFile[]): string {
  if (files.length === 0) return '<div class="empty-concept"><h2>Keine Dateien verknüpft</h2><p>Dieses Modell besitzt noch kein Fileset.</p></div>';
  const groups = [["3D & CAD", files.filter((file) => ["master-cad", "cad", "printable-mesh"].includes(file.role))], ["Bilder", files.filter((file) => file.role === "image")], ["Dokumente", files.filter((file) => file.role === "document")], ["Archive & weitere Dateien", files.filter((file) => ["archive", "other"].includes(file.role))]] as const;
  return `<div class="model-file-groups">${groups.filter(([, items]) => items.length > 0).map(([title, items]) => `<section class="model-file-group"><div class="model-file-group-title"><h3>${escapeMarkup(title)}</h3><span>${items.length}</span></div><div class="model-file-grid">${items.map(fileCard).join("")}</div></section>`).join("")}</div>`;
}

function fileCard(file: ModelFile): string {
  const name = file.path.split("/").at(-1) || file.path;
  const preview = file.format === "stl" && !file.missing ? `<canvas class="model-stl-preview" data-stl-preview="${escapeMarkup(file.id)}" aria-label="Drehbare 3D-Vorschau von ${escapeMarkup(name)}"></canvas><small data-stl-state>STL WIRD GELADEN</small>` : file.role === "image" && !file.missing ? `<img loading="lazy" src="${catalogApi.sourceContentUrl(file.id)}" alt="">` : `<span class="model-file-glyph">${fileGlyph(file)}</span><small>${escapeMarkup(file.format?.toUpperCase() || file.role.toUpperCase())}</small>`;
  return `<article class="model-file-card${file.missing ? " missing" : ""}" data-model-file="${escapeMarkup(file.id)}" data-open-file="${escapeMarkup(file.id)}" data-file-filter="${modelFileFilter(file)}"><span class="model-file-preview">${preview}${file.primary ? '<b>PRIMÄR</b>' : ""}</span><span class="model-file-card-copy"><span class="model-file-title"><strong>${escapeMarkup(file.caption || name)}</strong><button class="file-menu-trigger" data-file-menu="${escapeMarkup(file.id)}" type="button" aria-label="Menü für ${escapeMarkup(name)} öffnen" aria-haspopup="menu">•••</button></span><small title="${escapeMarkup(file.path)}">${escapeMarkup(file.path)}</small><em>${formatBytes(file.byteSize)}${file.printable ? " · DRUCKBAR" : ""}${file.missing ? " · NICHT VERFÜGBAR" : ""}</em></span></article>`;
}

export function fileDetailMarkup(file: ModelFile, canEdit: boolean, slicerTargets: SlicerTarget[]): string {
  const filename = file.path.split("/").at(-1) || "download";
  const download = file.missing ? "" : `<a class="primary-action" href="${catalogApi.sourceDownloadUrl(file.id)}" download="${escapeMarkup(filename)}">Original herunterladen</a>`;
  const slicers = file.printable && !file.missing
    ? `<section class="slicer-actions"><h3>Im Slicer öffnen</h3>${slicerTargets.length > 0
      ? `${slicerTargets.map((target) => `<button class="secondary-action" type="button" data-slicer-target="${escapeMarkup(target.id)}">${escapeMarkup(target.name)}</button>`).join("")}<p id="slicer-status" class="form-message" aria-live="polite"></p>`
      : '<p class="form-message">Kein Slicer-Ziel ist konfiguriert. Verwende stattdessen „Original herunterladen“.</p>'}</section>`
    : "";
  const history = historyDetails("source-file", file.id, "Dateiverlauf") + historyDetails("model-file", file.id, "Verknüpfungsverlauf");
  if (!canEdit) return `<div class="file-detail-copy"><p>${escapeMarkup(file.description || "Keine Beschreibung hinterlegt.")}</p>${download}${slicers}${history}</div>`;
  const orientation = file.orientation;
  const makePrimary = file.format === "step" && !file.primary && !file.missing
    ? `<button class="secondary-action" data-set-primary-file="${escapeMarkup(file.id)}" type="button">Als Primärdatei verwenden</button>` : "";
  return `<form id="model-file-metadata-form" class="file-metadata-form"><div class="file-form-columns"><section class="file-form-section"><h3>Beschreibung</h3><label>Titel<input name="caption" maxlength="160" value="${escapeMarkup(file.caption)}"></label><label>Beschreibung<textarea name="description" maxlength="4000" rows="4">${escapeMarkup(file.description)}</textarea></label><label>Notizen<textarea name="notes" maxlength="4000" rows="4">${escapeMarkup(file.notes)}</textarea></label></section><section class="file-form-section"><h3>Fertigung</h3><fieldset class="manufacturing-options"><legend class="sr-only">Fertigungsstatus</legend><label><input name="printable" type="checkbox"${file.printable ? " checked" : ""}> Druckbar (STL/3MF)</label><label><input name="printed" type="checkbox"${file.printed ? " checked" : ""}> Bereits gedruckt</label><label><input name="preSupported" type="checkbox"${file.preSupported ? " checked" : ""}> Bereits unterstützt</label></fieldset><div class="file-form-pair"><label>Oben-Achse<select name="upAxis"><option value="">Nicht angegeben</option>${["x", "y", "z"].map((axis) => `<option value="${axis}"${file.upAxis === axis ? " selected" : ""}>${axis.toUpperCase()}</option>`).join("")}</select></label><label>Support-Hinweis<input name="supportHint" maxlength="1000" value="${escapeMarkup(file.supportHint)}"></label></div><fieldset class="orientation-fields"><legend>Orientierung in Grad</legend>${orientation.map((value, index) => `<label>${["X", "Y", "Z"][index]}<input name="orientation${["X", "Y", "Z"][index]}" type="number" min="-360" max="360" step="0.1" value="${value}"></label>`).join("")}</fieldset>${slicers}</section></div><div class="dialog-actions">${download}${makePrimary}<button class="primary-action" type="submit">Dateiangaben speichern</button></div><p class="form-message" aria-live="polite"></p></form>${history}`;
}

export function slicerHandoffStatusMarkup(downloadUrl: string): string {
  return `Slicer wird geöffnet. <a href="${escapeMarkup(downloadUrl)}">Falls kein Zielprogramm reagiert: Datei herunterladen</a>`;
}

export function fileGlyph(file: ModelFile): string {
  if (file.role === "document") return "▤";
  if (file.role === "archive") return "▦";
  if (file.role === "image") return "▧";
  if (file.format) return "◇";
  return "·/·";
}

function formatUpdated(value: number): string {
  return new Intl.DateTimeFormat("de-DE", { dateStyle: "short", timeStyle: "short" }).format(new Date(value));
}

function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}
