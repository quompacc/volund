import { catalogApi } from "./api";
import { bindCatalogHistories, historyDetails } from "./catalog-history";
import { bindLifecycleEntries, lifecycleEntry } from "./lifecycle-ui";
import type { AuthorInput, AuthorSummary } from "./types";

export function mountAuthors(host: HTMLElement, canManage: boolean): void {
  host.innerHTML = `<div class="page-scroll"><header class="page-hero"><div><p class="eyebrow">URHEBER & QUELLEN</p><h1>Autoren</h1><p>Normalisierte Identitäten mit Website und gespeicherter Provenienz.</p></div></header>
    <section class="content-section">${canManage ? authorForm() : ""}<div id="author-status" class="form-message" aria-live="polite"></div><div id="author-catalog" class="collection-grid"><p class="loading-copy">Autoren werden geladen …</p></div></section></div>`;
  void load(host, canManage);
}

async function load(host: HTMLElement, canManage: boolean): Promise<void> {
  const catalog = host.querySelector<HTMLElement>("#author-catalog")!;
  try {
    const page = await catalogApi.authors(true);
    catalog.innerHTML = authorMarkup(page.items, canManage);
    mountActions(host, page.items, canManage);
    bindCatalogHistories(host);
    bindLifecycleEntries(host, () => void load(host, canManage));
  } catch (error) {
    catalog.innerHTML = `<div class="empty-concept"><h2>Autoren nicht verfügbar</h2><p>${escapeMarkup(message(error))}</p><button id="retry-authors" class="secondary-action">Erneut versuchen</button></div>`;
    host.querySelector("#retry-authors")?.addEventListener("click", () => void load(host, canManage));
  }
}

export function authorMarkup(authors: AuthorSummary[], canManage: boolean): string {
  if (authors.length === 0) {
    return '<div class="empty-concept"><h2>Noch keine Autoren</h2><p>Autoren können kontextuell im Modelleditor oder hier angelegt werden.</p></div>';
  }
  const active = authors.filter((author) => author.active);
  return authors.map((author) => `<article data-author="${escapeMarkup(author.id)}">
    <p class="eyebrow">${author.active ? "AUTOR" : "ZUSAMMENGEFÜHRT"}</p><h2>${escapeMarkup(author.name)}</h2>
    <p>${author.website ? `<a href="${escapeMarkup(author.website)}" target="_blank" rel="noopener noreferrer">${escapeMarkup(author.website)}</a>` : "Keine Website"}</p>
    <p>${escapeMarkup(author.provenanceNote || provenanceLabel(author.provenanceSource))}</p>
    <strong>${author.modelCount} Modelle · Revision ${author.revision}</strong>
    ${author.mergedIntoId ? `<small>Ziel-ID: ${escapeMarkup(author.mergedIntoId)}</small>` : ""}
    ${canManage && author.active ? editForm(author) + mergeForm(author, active) + lifecycleEntry("author.remove", author.id, author.revision, "Autor entfernen") : ""}${historyDetails("author", author.id)}</article>`).join("");
}

function authorForm(): string {
  return `<form id="create-author" class="admin-form" aria-label="Autor anlegen">
    <input name="name" maxlength="160" placeholder="Name" aria-label="Name" required>
    <input name="website" maxlength="2048" placeholder="https://…" aria-label="Website">
    <select name="provenanceSource" aria-label="Provenienz"><option value="unknown">Nicht angegeben</option><option value="user">Manuell</option><option value="website">Website</option><option value="import">Import</option></select>
    <input name="provenanceNote" maxlength="2000" placeholder="Provenienznotiz" aria-label="Provenienznotiz">
    <button class="primary-action">Autor anlegen</button></form>`;
}

function editForm(author: AuthorSummary): string {
  return `<details><summary>Bearbeiten</summary><form data-edit-author="${escapeMarkup(author.id)}" class="admin-form">
    <input name="name" value="${escapeMarkup(author.name)}" maxlength="160" aria-label="Name" required>
    <input name="website" value="${escapeMarkup(author.website || "")}" maxlength="2048" aria-label="Website">
    <select name="provenanceSource" aria-label="Provenienz">${["unknown", "user", "website", "import"].map((value) => `<option value="${value}"${value === author.provenanceSource ? " selected" : ""}>${provenanceLabel(value)}</option>`).join("")}</select>
    <input name="provenanceNote" value="${escapeMarkup(author.provenanceNote || "")}" maxlength="2000" aria-label="Provenienznotiz">
    <button class="secondary-action">Revision ${author.revision} speichern</button></form></details>`;
}

function mergeForm(author: AuthorSummary, active: AuthorSummary[]): string {
  const targets = active.filter((target) => target.id !== author.id);
  if (targets.length === 0) return "";
  return `<details><summary>Zusammenführen</summary><form data-merge-author="${escapeMarkup(author.id)}" class="admin-form">
    <select name="targetAuthorId" aria-label="Zielautor">${targets.map((target) => `<option value="${escapeMarkup(target.id)}">${escapeMarkup(target.name)}</option>`).join("")}</select>
    <input name="confirmation" maxlength="128" aria-label="Bestätigung" placeholder="MERGE AUTHOR …" required>
    <button class="secondary-action">Zusammenführen</button><small>Exakt: MERGE AUTHOR ${escapeMarkup(author.id)} INTO &lt;Ziel-ID&gt;</small></form></details>`;
}

function mountActions(host: HTMLElement, authors: AuthorSummary[], canManage: boolean): void {
  if (!canManage) return;
  host.querySelector<HTMLFormElement>("#create-author")?.addEventListener("submit", (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    void run(host, () => catalogApi.createAuthor(input(new FormData(form))), "Autor angelegt.", canManage);
  });
  host.querySelectorAll<HTMLFormElement>("[data-edit-author]").forEach((form) => form.addEventListener("submit", (event) => {
    event.preventDefault();
    const author = authors.find((item) => item.id === form.dataset.editAuthor)!;
    void run(host, () => catalogApi.updateAuthor(author, input(new FormData(form))), "Autor gespeichert.", canManage);
  }));
  host.querySelectorAll<HTMLFormElement>("[data-merge-author]").forEach((form) => form.addEventListener("submit", (event) => {
    event.preventDefault();
    const data = new FormData(form);
    void run(host, () => catalogApi.mergeAuthor(form.dataset.mergeAuthor!, String(data.get("targetAuthorId")), String(data.get("confirmation"))), "Autoren sicher zusammengeführt.", canManage);
  }));
}

async function run(host: HTMLElement, action: () => Promise<unknown>, success: string, canManage: boolean): Promise<void> {
  const status = host.querySelector<HTMLElement>("#author-status")!;
  status.classList.remove("error");
  status.textContent = "Änderung wird gespeichert …";
  try { await action(); status.textContent = success; await load(host, canManage); }
  catch (error) { status.classList.add("error"); status.textContent = message(error); status.focus(); }
}

function input(data: FormData): AuthorInput {
  return { name: String(data.get("name") || ""), website: optional(data, "website"),
    provenanceSource: String(data.get("provenanceSource")) as AuthorInput["provenanceSource"],
    provenanceNote: optional(data, "provenanceNote") };
}
function optional(data: FormData, key: string): string | null { return String(data.get(key) || "").trim() || null; }
function provenanceLabel(value: string): string { return ({ unknown: "Nicht angegeben", user: "Manuell", website: "Website", import: "Import" } as Record<string, string>)[value] || value; }
function message(error: unknown): string { return error instanceof Error ? error.message : "Autorenaktion fehlgeschlagen."; }
function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
