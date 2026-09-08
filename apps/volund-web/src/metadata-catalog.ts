import { catalogApi } from "./api";
import { bindCatalogHistories, historyDetails } from "./catalog-history";
import { bindLifecycleEntries, lifecycleEntry } from "./lifecycle-ui";
import type { CollectionDetail, CollectionSummary, ModelSummary, TagSummary } from "./types";

export function mountCollections(host: HTMLElement, canEdit: boolean, canAdminister: boolean): void {
  host.dataset.metadataOffset = "0";
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">ORDNUNG</p><h1>Sammlungen</h1><p>Modelle unabhängig von ihren physischen Speicherorten gruppieren.</p></div></header>
    <section class="content-section">${canEdit ? collectionCreateForm() : ""}<div id="collection-status" class="form-message" aria-live="polite" tabindex="-1"></div><div id="collection-catalog" class="collection-grid"><p class="loading-copy">Sammlungen werden geladen …</p></div></section></div>`;
  void loadCollections(host, canEdit, canAdminister);
}

async function loadCollections(host: HTMLElement, canEdit: boolean, canAdminister: boolean): Promise<void> {
  const target = host.querySelector<HTMLElement>("#collection-catalog")!;
  try {
    const offset = metadataOffset(host);
    const [page, models] = await Promise.all([catalogApi.collectionPage(offset), catalogApi.models()]);
    const details = await Promise.all(page.items.map((item) => catalogApi.collection(item.id)));
    target.innerHTML = collectionMarkup(details, models, canEdit, canAdminister) + paginationMarkup(page.offset, page.limit, page.total);
    bindCollectionActions(host, details, canEdit, canAdminister);
    bindCatalogHistories(host);
    bindLifecycleEntries(host, () => void loadCollections(host, canEdit, canAdminister));
    bindPagination(host, page.offset, page.limit, () => void loadCollections(host, canEdit, canAdminister));
  } catch (error) {
    target.innerHTML = errorMarkup("Sammlungen nicht verfügbar", error);
    host.querySelector("#retry-metadata")?.addEventListener("click", () => void loadCollections(host, canEdit, canAdminister));
  }
}

export function collectionMarkup(items: CollectionDetail[], models: ModelSummary[], canEdit: boolean, canAdminister: boolean): string {
  if (items.length === 0) return '<div class="empty-concept"><h2>Noch keine Sammlungen</h2><p>Editoren können hier oder beim Import eine Sammlung anlegen.</p></div>';
  return items.map((item) => `<article data-collection="${escapeMarkup(item.id)}"><p class="eyebrow">SAMMLUNG</p><h2>${escapeMarkup(item.name)}</h2><p>${escapeMarkup(item.description || "Keine Beschreibung")}</p><strong>${item.modelCount} Modelle · Revision ${item.revision}</strong>
    ${memberList(item, models, canEdit)}${canAdminister ? collectionAdminForms(item) : ""}${historyDetails("collection", item.id)}</article>`).join("");
}

function collectionCreateForm(): string {
  return `<form id="create-collection" class="admin-form" aria-label="Sammlung anlegen"><input name="name" maxlength="160" placeholder="Name" aria-label="Name" required><input name="description" maxlength="4000" placeholder="Beschreibung" aria-label="Beschreibung"><button class="primary-action">Sammlung anlegen</button></form>`;
}

function memberList(item: CollectionDetail, models: ModelSummary[], canEdit: boolean): string {
  const selected = models.filter((model) => item.modelIds.includes(model.id));
  const available = models.filter((model) => !item.modelIds.includes(model.id));
  const list = selected.length > 0 ? `<ul class="relation-list">${selected.map((model) => `<li>${escapeMarkup(model.name)}${canEdit ? `<button type="button" data-remove-model="${escapeMarkup(model.id)}">Entfernen</button>` : ""}</li>`).join("")}</ul>` : "<small>Keine Modelle zugeordnet.</small>";
  if (!canEdit || available.length === 0) return list;
  return `${list}<form data-add-member class="admin-form"><select name="modelId" aria-label="Modell hinzufügen">${available.map((model) => `<option value="${escapeMarkup(model.id)}">${escapeMarkup(model.name)}</option>`).join("")}</select><button class="secondary-action">Modell hinzufügen</button></form>`;
}

function collectionAdminForms(item: CollectionDetail): string {
  return `<details><summary>Bearbeiten</summary><form data-edit-collection class="admin-form"><input name="name" value="${escapeMarkup(item.name)}" maxlength="160" required aria-label="Name"><input name="description" value="${escapeMarkup(item.description)}" maxlength="4000" aria-label="Beschreibung"><button class="secondary-action">Speichern</button></form></details>
    ${lifecycleEntry("collection.remove", item.id, item.revision, "Sammlung entfernen")}`;
}

function bindCollectionActions(host: HTMLElement, items: CollectionDetail[], canEdit: boolean, canAdminister: boolean): void {
  if (canEdit) host.querySelector<HTMLFormElement>("#create-collection")?.addEventListener("submit", (event) => {
    event.preventDefault(); const data = new FormData(event.currentTarget as HTMLFormElement);
    void runCollection(host, () => catalogApi.createCollection({ name: String(data.get("name")), description: String(data.get("description") || "") }), "Sammlung angelegt.", canEdit, canAdminister);
  });
  host.querySelectorAll<HTMLElement>("[data-collection]").forEach((card) => {
    const item = items.find((entry) => entry.id === card.dataset.collection)!;
    card.querySelector<HTMLFormElement>("[data-add-member]")?.addEventListener("submit", (event) => {
      event.preventDefault(); const modelId = String(new FormData(event.currentTarget as HTMLFormElement).get("modelId"));
      void runCollection(host, () => catalogApi.setCollectionMember(item, modelId, true), "Modell hinzugefügt.", canEdit, canAdminister);
    });
    card.querySelectorAll<HTMLButtonElement>("[data-remove-model]").forEach((button) => button.addEventListener("click", () => {
      void runCollection(host, () => catalogApi.setCollectionMember(item, button.dataset.removeModel!, false), "Modell entfernt.", canEdit, canAdminister);
    }));
    card.querySelector<HTMLFormElement>("[data-edit-collection]")?.addEventListener("submit", (event) => {
      event.preventDefault(); const data = new FormData(event.currentTarget as HTMLFormElement);
      void runCollection(host, () => catalogApi.updateCollection(item, { name: String(data.get("name")), description: String(data.get("description") || "") }), "Sammlung gespeichert.", canEdit, canAdminister);
    });
  });
}

async function runCollection(host: HTMLElement, action: () => Promise<unknown>, success: string, canEdit: boolean, canAdminister: boolean): Promise<void> {
  const status = host.querySelector<HTMLElement>("#collection-status")!; status.classList.remove("error"); status.textContent = "Änderung wird gespeichert …";
  try { await action(); status.textContent = success; await loadCollections(host, canEdit, canAdminister); }
  catch (error) { status.classList.add("error"); status.textContent = message(error); status.focus(); }
}

export function mountTags(host: HTMLElement, canManage: boolean): void {
  host.dataset.metadataOffset = "0";
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">KLASSIFIKATION</p><h1>Tags</h1><p>Eine normalisierte, wiederverwendbare Taxonomie für den Modellkatalog.</p></div></header><section class="content-section">${canManage ? tagCreateForm() : ""}<div id="tag-status" class="form-message" aria-live="polite" tabindex="-1"></div><div id="tag-catalog" class="collection-grid"><p class="loading-copy">Tags werden geladen …</p></div></section></div>`;
  void loadTags(host, canManage);
}

async function loadTags(host: HTMLElement, canManage: boolean): Promise<void> {
  const target = host.querySelector<HTMLElement>("#tag-catalog")!;
  try { const page = await catalogApi.tags(true, metadataOffset(host)); target.innerHTML = tagMarkup(page.items, canManage) + paginationMarkup(page.offset, page.limit, page.total); bindTagActions(host, page.items, canManage); bindCatalogHistories(host); bindLifecycleEntries(host, () => void loadTags(host, canManage)); bindPagination(host, page.offset, page.limit, () => void loadTags(host, canManage)); }
  catch (error) { target.innerHTML = errorMarkup("Tags nicht verfügbar", error); host.querySelector("#retry-metadata")?.addEventListener("click", () => void loadTags(host, canManage)); }
}

export function tagMarkup(tags: TagSummary[], canManage: boolean): string {
  if (tags.length === 0) return '<div class="empty-concept"><h2>Noch keine Tags</h2><p>Tags können kontextuell im Modelleditor oder hier angelegt werden.</p></div>';
  const active = tags.filter((tag) => tag.active);
  return tags.map((tag) => `<article data-tag="${escapeMarkup(tag.id)}"><p class="eyebrow">${tag.active ? "TAG" : "INAKTIVER ALIAS"}</p><h2>${escapeMarkup(tag.name)}</h2><strong>${tag.modelCount} Modelle · Revision ${tag.revision}</strong>${tag.mergedIntoId ? `<small>Ziel-ID: ${escapeMarkup(tag.mergedIntoId)}</small>` : ""}${canManage && tag.active ? tagAdminForms(tag, active) : ""}${historyDetails("tag", tag.id)}</article>`).join("");
}

function tagCreateForm(): string { return '<form id="create-tag" class="admin-form" aria-label="Tag anlegen"><input name="name" maxlength="50" placeholder="Tagname" aria-label="Name" required><button class="primary-action">Tag anlegen</button></form>'; }
function tagAdminForms(tag: TagSummary, active: TagSummary[]): string {
  const targets = active.filter((item) => item.id !== tag.id);
  const merge = targets.length === 0 ? "" : `<details><summary>Zusammenführen</summary><form data-merge-tag class="admin-form"><select name="targetTagId" aria-label="Zieltag">${targets.map((item) => `<option value="${escapeMarkup(item.id)}">${escapeMarkup(item.name)}</option>`).join("")}</select><input name="confirmation" maxlength="128" required aria-label="Bestätigung" placeholder="MERGE TAG …"><button class="secondary-action">Zusammenführen</button><small>Exakt: MERGE TAG ${escapeMarkup(tag.id)} INTO &lt;Ziel-ID&gt;</small></form></details>`;
  return `<details><summary>Umbenennen</summary><form data-edit-tag class="admin-form"><input name="name" value="${escapeMarkup(tag.name)}" maxlength="50" required aria-label="Name"><button class="secondary-action">Speichern</button></form></details>${merge}${lifecycleEntry("tag.remove", tag.id, tag.revision, "Tag entfernen")}`;
}

function bindTagActions(host: HTMLElement, tags: TagSummary[], canManage: boolean): void {
  if (!canManage) return;
  host.querySelector<HTMLFormElement>("#create-tag")?.addEventListener("submit", (event) => { event.preventDefault(); const data = new FormData(event.currentTarget as HTMLFormElement); void runTag(host, () => catalogApi.createTag(String(data.get("name"))), "Tag angelegt.", canManage); });
  host.querySelectorAll<HTMLElement>("[data-tag]").forEach((card) => {
    const tag = tags.find((item) => item.id === card.dataset.tag)!;
    card.querySelector<HTMLFormElement>("[data-edit-tag]")?.addEventListener("submit", (event) => { event.preventDefault(); const data = new FormData(event.currentTarget as HTMLFormElement); void runTag(host, () => catalogApi.updateTag(tag, String(data.get("name"))), "Tag gespeichert.", canManage); });
    card.querySelector<HTMLFormElement>("[data-merge-tag]")?.addEventListener("submit", (event) => { event.preventDefault(); const data = new FormData(event.currentTarget as HTMLFormElement); void runTag(host, () => catalogApi.mergeTag(tag, String(data.get("targetTagId")), String(data.get("confirmation"))), "Tags zusammengeführt.", canManage); });
  });
}

async function runTag(host: HTMLElement, action: () => Promise<unknown>, success: string, canManage: boolean): Promise<void> {
  const status = host.querySelector<HTMLElement>("#tag-status")!; status.classList.remove("error"); status.textContent = "Änderung wird gespeichert …";
  try { await action(); status.textContent = success; await loadTags(host, canManage); }
  catch (error) { status.classList.add("error"); status.textContent = message(error); status.focus(); }
}

function errorMarkup(title: string, error: unknown): string { return `<div class="empty-concept"><h2>${title}</h2><p>${escapeMarkup(message(error))}</p><button id="retry-metadata" class="secondary-action">Erneut versuchen</button></div>`; }
function metadataOffset(host: HTMLElement): number { return Number(host.dataset.metadataOffset || 0); }
function paginationMarkup(offset: number, limit: number, total: number): string {
  if (total <= limit) return "";
  return `<nav class="pagination" aria-label="Metadaten-Seiten"><button class="secondary-action" data-metadata-page="previous"${offset === 0 ? " disabled" : ""}>← Zurück</button><span>${offset + 1}–${Math.min(offset + limit, total)} von ${total}</span><button class="secondary-action" data-metadata-page="next"${offset + limit >= total ? " disabled" : ""}>Weiter →</button></nav>`;
}
function bindPagination(host: HTMLElement, offset: number, limit: number, reload: () => void): void {
  host.querySelectorAll<HTMLButtonElement>("[data-metadata-page]").forEach((button) => button.addEventListener("click", () => {
    host.dataset.metadataOffset = String(button.dataset.metadataPage === "next" ? offset + limit : Math.max(0, offset - limit));
    reload();
  }));
}
function message(error: unknown): string { return error instanceof Error ? error.message : "Metadatenaktion fehlgeschlagen."; }
function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
