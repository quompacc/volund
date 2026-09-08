import { catalogApi } from "./api";
import type { CollectionSummary, ModelFile, ModelSummary, TagSummary, UpdateModelRequest } from "./types";

const SPDX = [
  "0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "CC-BY-4.0",
  "CC-BY-SA-4.0", "CERN-OHL-P-2.0", "CERN-OHL-S-2.0", "CERN-OHL-W-2.0",
  "GPL-3.0-only", "MIT", "Unlicense",
];

export function modelEditorMarkup(
  model: ModelSummary,
  collections: CollectionSummary[],
  files: ModelFile[],
  tags: TagSummary[],
): string {
  const selectedCollections = new Set(model.collections.map((name) => name.toLocaleLowerCase()));
  const primaryOptions = files.filter((file) =>
    ["master-cad", "cad", "printable-mesh"].includes(file.role)
    && ["step", "iges", "brep", "stl", "3mf", "obj", "ply"].includes(file.format || ""));
  return `<dialog id="model-editor" class="model-editor-dialog"><form id="model-editor-form" method="dialog">
    <header class="move-dialog-head"><div><p class="eyebrow">MODELLPFLEGE</p><h2>Projekt bearbeiten</h2></div><button type="button" data-close-editor aria-label="Schließen">×</button></header>
    <div class="model-editor-body"><section class="model-editor-fields">
      <label>Name<input name="name" value="${escapeMarkup(model.name)}" maxlength="160" required></label>
      <label>Beschreibung<textarea name="description" maxlength="4000" rows="4">${escapeMarkup(model.description)}</textarea></label>
      <label>Typ<select name="kind"><option value="project"${selected(model.kind, "project")}>Projekt</option><option value="assembly"${selected(model.kind, "assembly")}>Baugruppe</option><option value="part"${selected(model.kind, "part")}>Einzelteil</option></select></label>
      <label>Lizenzstatus<select name="licenseKind"><option value="not-specified"${selected(model.licenseKind, "not-specified")}>Nicht angegeben</option><option value="spdx"${selected(model.licenseKind, "spdx")}>Bekannte SPDX-Lizenz</option><option value="custom"${selected(model.licenseKind, "custom")}>Benutzerdefiniert</option></select></label>
      <label>Lizenzangabe<input name="licenseValue" value="${escapeMarkup(model.licenseValue || "")}" maxlength="160" list="spdx-licenses" placeholder="z. B. CERN-OHL-S-2.0"><datalist id="spdx-licenses">${SPDX.map((value) => `<option value="${value}">`).join("")}</datalist></label>
      <label>Autor / Quelle<input name="authorName" value="${escapeMarkup(model.authorName || "")}" maxlength="160" placeholder="Neuer Name wird automatisch angelegt"></label>
      <fieldset><legend>Tags</legend><div class="tag-chip-picker">${tags.filter((tag) => tag.active).map((tag) => `<label><input type="checkbox" name="tagIds" value="${escapeMarkup(tag.id)}"${model.tagIds.includes(tag.id) ? " checked" : ""}><span title="${escapeMarkup(tag.name)}">${escapeMarkup(tag.name)}</span></label>`).join("") || "<small>Noch keine aktiven Tags. Administratoren können sie im Tag-Katalog anlegen.</small>"}</div><small>Stabile Tag-IDs werden gespeichert; Namen können später ohne Beziehungsverlust geändert werden.</small></fieldset>
    </section><section class="model-editor-fields">
      <label>Primäre Quelldatei<select name="primaryFileId"><option value="">Bewusst keine Primärdatei</option>${primaryOptions.map((file) => `<option value="${escapeMarkup(file.id)}"${selected(model.primaryFileId || "", file.id)}>${escapeMarkup(file.path)}${file.missing ? " · fehlt" : ""}</option>`).join("")}</select><small>Nur verknüpfte CAD- oder Mesh-Dateien. Fehlende Beziehungen bleiben erhalten.</small></label>
      <fieldset><legend>Sammlungen</legend><div id="editor-collections" class="editor-check-list">${collections.map((collection) => `<label><input type="checkbox" name="collectionIds" value="${escapeMarkup(collection.id)}"${selectedCollections.has(collection.name.toLocaleLowerCase()) ? " checked" : ""}><span>${escapeMarkup(collection.name)}</span></label>`).join("") || "<small>Noch keine Sammlung vorhanden.</small>"}</div><div class="editor-inline"><input id="editor-new-collection" maxlength="160" placeholder="Neue Sammlung"><button id="editor-create-collection" type="button">Anlegen</button></div></fieldset>
      <fieldset><legend>Ansichtsausrichtung</legend><p class="field-help">Reine Präsentationsmetadaten; CAD- und Vorschaudateien bleiben unverändert.</p><div class="rotation-grid">${rotationSelect("rotationX", "X", model.viewerRotation[0])}${rotationSelect("rotationY", "Y", model.viewerRotation[1])}${rotationSelect("rotationZ", "Z", model.viewerRotation[2])}</div></fieldset>
    </section></div>
    <p id="model-editor-error" class="form-message error" role="alert" tabindex="-1" hidden></p><p id="model-editor-status" class="form-message" aria-live="polite"></p><footer class="model-editor-actions"><button type="button" class="secondary-action" data-close-editor>Abbrechen</button><button class="primary-action" type="submit">Änderungen speichern</button></footer>
  </form></dialog>`;
}

export function mountModelEditor(
  host: HTMLElement,
  model: ModelSummary,
  collections: CollectionSummary[],
  files: ModelFile[],
  tags: TagSummary[],
  onChanged: () => void,
): void {
  host.insertAdjacentHTML("beforeend", modelEditorMarkup(model, collections, files, tags));
  const dialog = host.querySelector<HTMLDialogElement>("#model-editor")!;
  const form = host.querySelector<HTMLFormElement>("#model-editor-form")!;
  const error = host.querySelector<HTMLElement>("#model-editor-error")!;
  const status = host.querySelector<HTMLElement>("#model-editor-status")!;
  host.querySelectorAll("[data-close-editor]").forEach((button) =>
    button.addEventListener("click", () => dialog.close()));
  host.querySelector("#open-model-editor")?.addEventListener("click", () => dialog.showModal());
  host.querySelector("#editor-create-collection")?.addEventListener("click", async () => {
    const input = host.querySelector<HTMLInputElement>("#editor-new-collection")!;
    if (!input.value.trim()) return;
    try {
      const collection = await catalogApi.createCollection({ name: input.value.trim(), description: "" });
      host.querySelector("#editor-collections")?.insertAdjacentHTML("beforeend",
        `<label><input type="checkbox" name="collectionIds" value="${escapeMarkup(collection.id)}" checked><span>${escapeMarkup(collection.name)}</span></label>`);
      input.value = "";
      status.textContent = "Sammlung angelegt und ausgewählt.";
    } catch (reason) { showError(error, reason); }
  });
  form.addEventListener("submit", async (event) => {
    event.preventDefault();
    error.hidden = true;
    const submit = form.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    const data = new FormData(form);
    const licenseKind = String(data.get("licenseKind")) as UpdateModelRequest["licenseKind"];
    const licenseValue = String(data.get("licenseValue") || "").trim() || null;
    const request: UpdateModelRequest = {
      expectedRevision: model.revision,
      name: String(data.get("name") || "").trim(),
      description: String(data.get("description") || "").trim(),
      kind: String(data.get("kind")) as UpdateModelRequest["kind"],
      licenseKind,
      licenseValue: licenseKind === "not-specified" ? null : licenseValue,
      authorName: String(data.get("authorName") || "").trim() || null,
      tags: [],
      tagIds: data.getAll("tagIds").map(String),
      collectionIds: data.getAll("collectionIds").map(String),
      primaryFileId: String(data.get("primaryFileId") || "") || null,
      viewerRotation: [numberField(data, "rotationX"), numberField(data, "rotationY"), numberField(data, "rotationZ")],
    };
    submit.disabled = true;
    status.textContent = "Änderungen werden atomar gespeichert …";
    try {
      await catalogApi.updateModel(model.id, request);
      status.textContent = "Änderungen gespeichert.";
      onChanged();
    } catch (reason) {
      status.textContent = "";
      showError(error, reason);
    } finally { submit.disabled = false; }
  });
}

function rotationSelect(name: string, axis: string, current: number): string {
  const values = [-180, -90, 0, 90, 180];
  return `<label>${axis}<select name="${name}">${values.map((value) => `<option value="${value}"${value === current ? " selected" : ""}>${value > 0 ? "+" : ""}${value}°</option>`).join("")}</select></label>`;
}

function selected(current: string, expected: string): string { return current === expected ? " selected" : ""; }
function numberField(data: FormData, name: string): number { return Number(data.get(name) || 0); }
function showError(target: HTMLElement, reason: unknown): void {
  target.hidden = false;
  target.textContent = reason instanceof Error ? reason.message : "Änderung fehlgeschlagen.";
  target.focus();
}
function escapeMarkup(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}
