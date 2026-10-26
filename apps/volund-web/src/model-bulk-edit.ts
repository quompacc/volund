import type { CollectionSummary, ModelSummary, TagSummary } from "./types";

/** From this many models on, the server requires the count as confirmation (ADR 0034). */
export const BULK_CONFIRM_FROM = 10;

export interface BulkChanges {
  addTagIds?: string[];
  removeTagIds?: string[];
  addCollectionIds?: string[];
  removeCollectionIds?: string[];
  kind?: string;
  authorName?: string;
  licenseKind?: string;
  licenseValue?: string;
}

export interface BulkEditRequest {
  models: Array<{ id: string; expectedRevision: number }>;
  changes: BulkChanges;
  confirmation?: string;
}

export interface BulkEditSources {
  apply: (request: BulkEditRequest) => Promise<ModelSummary[]>;
  collections: () => Promise<CollectionSummary[]>;
}

/** Checkbox wrapper around one model card; carries the revision the user saw. */
export function selectableCardMarkup(model: ModelSummary, card: string, selected: boolean): string {
  return `<div class="bulk-card"><label class="bulk-select"><input type="checkbox" data-bulk-model="${escapeMarkup(model.id)}" data-bulk-revision="${model.revision}"${selected ? " checked" : ""}> Auswählen</label>${card}</div>`;
}

export function bulkBarMarkup(count: number, tags: TagSummary[], collections: CollectionSummary[]): string {
  if (count === 0) return "";
  const options = (items: Array<{ id: string; name: string }>) => items.map((item) => `<option value="${escapeMarkup(item.id)}">${escapeMarkup(item.name)}</option>`).join("");
  const activeTags = tags.filter((tag) => tag.active);
  return `<form id="bulk-edit-form" class="bulk-bar" aria-label="Ausgewählte Modelle bearbeiten">
    <strong>${count} ${count === 1 ? "Modell" : "Modelle"} ausgewählt</strong>
    <label>Tag hinzufügen<select name="addTag"><option value="">—</option>${options(activeTags)}</select></label>
    <label>Tag entfernen<select name="removeTag"><option value="">—</option>${options(activeTags)}</select></label>
    <label>Zu Sammlung<select name="addCollection"><option value="">—</option>${options(collections)}</select></label>
    <label>Aus Sammlung<select name="removeCollection"><option value="">—</option>${options(collections)}</select></label>
    <label>Art<select name="kind"><option value="">Unverändert</option><option value="part">Einzelteil</option><option value="assembly">Baugruppe</option><option value="project">Projekt</option></select></label>
    <label>Autor<input name="author" maxlength="160" placeholder="Unverändert"></label>
    <label class="bulk-clear"><input type="checkbox" name="clearAuthor"> Autor entfernen</label>
    <div class="bulk-actions"><button class="primary-action" type="submit">Auf Auswahl anwenden</button><button class="secondary-action" type="button" data-bulk-clear>Auswahl aufheben</button></div>
    <p id="bulk-edit-message" class="form-message" aria-live="polite"></p>
  </form>`;
}

/** Translate the bar's form into changes; an empty object means nothing to do. */
export function bulkChanges(form: HTMLFormElement): BulkChanges {
  const value = (name: string) => (form.elements.namedItem(name) as HTMLInputElement | HTMLSelectElement | null)?.value.trim() ?? "";
  const changes: BulkChanges = {};
  if (value("addTag")) changes.addTagIds = [value("addTag")];
  if (value("removeTag")) changes.removeTagIds = [value("removeTag")];
  if (value("addCollection")) changes.addCollectionIds = [value("addCollection")];
  if (value("removeCollection")) changes.removeCollectionIds = [value("removeCollection")];
  if (value("kind")) changes.kind = value("kind");
  const clear = (form.elements.namedItem("clearAuthor") as HTMLInputElement | null)?.checked;
  if (clear) changes.authorName = "";
  else if (value("author")) changes.authorName = value("author");
  return changes;
}

export function bulkRequest(selection: Map<string, number>, changes: BulkChanges): BulkEditRequest {
  const models = [...selection].map(([id, expectedRevision]) => ({ id, expectedRevision }));
  const request: BulkEditRequest = { models, changes };
  if (models.length >= BULK_CONFIRM_FROM) request.confirmation = `EDIT ${models.length} MODELS`;
  return request;
}

/**
 * Wire selection checkboxes inside `grid` and the action bar in `bar`.
 * `reload` refreshes the list after a successful edit; the selection then ends.
 */
export function mountBulkEdit(
  grid: HTMLElement,
  bar: HTMLElement,
  sources: BulkEditSources,
  tags: () => TagSummary[],
  reload: () => void,
  win: Window = window,
): { selection: Map<string, number>; clear: () => void } {
  const selection = new Map<string, number>();
  let collections: CollectionSummary[] = [];
  void sources.collections().then((items) => { collections = items; render(); }, () => undefined);
  const render = () => { bar.innerHTML = bulkBarMarkup(selection.size, tags(), collections); };
  const clear = () => { selection.clear(); grid.querySelectorAll<HTMLInputElement>("[data-bulk-model]").forEach((box) => { box.checked = false; }); render(); };
  grid.addEventListener("change", (event) => {
    const box = (event.target as HTMLElement).closest<HTMLInputElement>("[data-bulk-model]");
    if (!box) return;
    if (box.checked) selection.set(box.dataset.bulkModel!, Number(box.dataset.bulkRevision));
    else selection.delete(box.dataset.bulkModel!);
    render();
  });
  bar.addEventListener("click", (event) => {
    if ((event.target as HTMLElement).closest("[data-bulk-clear]")) clear();
  });
  bar.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.target as HTMLFormElement;
    const message = form.querySelector<HTMLElement>("#bulk-edit-message")!;
    const changes = bulkChanges(form);
    if (Object.keys(changes).length === 0) { message.textContent = "Wähle mindestens eine Änderung."; return; }
    const request = bulkRequest(selection, changes);
    if (request.confirmation && !win.confirm(`${selection.size} Modelle gemeinsam ändern?`)) return;
    message.textContent = "Änderungen werden angewendet …";
    void sources.apply(request).then((updated) => {
      selection.clear();
      reload();
      render();
      bar.insertAdjacentHTML("afterbegin", `<p class="form-message bulk-done" aria-live="polite">${updated.length} ${updated.length === 1 ? "Modell" : "Modelle"} aktualisiert.</p>`);
    }, (error: Error) => { message.textContent = error.message; });
  });
  return { selection, clear };
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
