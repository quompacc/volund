import { modelCardMarkup } from "./model-presentation";
import type { ModelSearchPage, Page, TagSummary } from "./types";

export type ModelSort = "updated" | "name" | "created";

/** Filter state of the model list; mirrored in the page URL so views can be shared. */
export interface ModelListState {
  q: string;
  tag: string;
  kind: string;
  sort: ModelSort;
  page: number;
}

export const MODEL_PAGE_SIZE = 24;
const DASHBOARD_RECENT = 6;
const SORTS: ModelSort[] = ["updated", "name", "created"];
const KINDS = ["part", "assembly", "project"];

export interface ModelListSources {
  search: (query: string) => Promise<ModelSearchPage>;
  tags: () => Promise<Page<TagSummary>>;
}

export function readModelListState(search: string): ModelListState {
  const parameters = new URLSearchParams(search);
  const sort = parameters.get("sort") as ModelSort | null;
  const kind = parameters.get("kind") ?? "";
  const page = Number.parseInt(parameters.get("page") ?? "1", 10);
  return {
    q: (parameters.get("q") ?? "").slice(0, 200),
    tag: parameters.get("tag") ?? "",
    kind: KINDS.includes(kind) ? kind : "",
    sort: sort && SORTS.includes(sort) ? sort : "updated",
    page: Number.isFinite(page) && page > 0 ? page : 1,
  };
}

/** The API query for one page of `state`. */
export function modelSearchQuery(state: ModelListState, limit = MODEL_PAGE_SIZE): string {
  const parameters = new URLSearchParams();
  if (state.q.trim()) parameters.set("q", state.q.trim());
  if (state.tag) parameters.set("tag", state.tag);
  if (state.kind) parameters.set("kind", state.kind);
  if (state.sort !== "updated") parameters.set("sort", state.sort);
  parameters.set("limit", String(limit));
  parameters.set("offset", String((state.page - 1) * limit));
  return `?${parameters}`;
}

/** The page URL for `state`, keeping unrelated parameters such as `view`. */
export function modelListUrl(search: string, state: ModelListState): string {
  const parameters = new URLSearchParams(search);
  for (const key of ["q", "tag", "kind", "sort", "page"]) parameters.delete(key);
  if (state.q.trim()) parameters.set("q", state.q.trim());
  if (state.tag) parameters.set("tag", state.tag);
  if (state.kind) parameters.set("kind", state.kind);
  if (state.sort !== "updated") parameters.set("sort", state.sort);
  if (state.page > 1) parameters.set("page", String(state.page));
  return `?${parameters}`;
}

export function pageCount(total: number, pageSize = MODEL_PAGE_SIZE): number {
  return Math.max(1, Math.ceil(total / pageSize));
}

export function pagerMarkup(page: number, total: number, pageSize = MODEL_PAGE_SIZE): string {
  const pages = pageCount(total, pageSize);
  if (pages <= 1) return "";
  return `<nav class="model-pager" aria-label="Seiten der Modellliste">
    <button type="button" class="secondary-action" data-model-page="${page - 1}"${page <= 1 ? " disabled" : ""}>Zurück</button>
    <span>Seite ${page} von ${pages}</span>
    <button type="button" class="secondary-action" data-model-page="${page + 1}"${page >= pages ? " disabled" : ""}>Weiter</button></nav>`;
}

export function modelListToolbarMarkup(state: ModelListState): string {
  const option = (value: string, label: string, selected: string) =>
    `<option value="${value}"${value === selected ? " selected" : ""}>${label}</option>`;
  return `<form id="model-list-filters" class="model-list-filters" role="search">
    <label>Suche<input name="q" type="search" maxlength="200" placeholder="Name, Beschreibung, Autor oder Tag" value="${escapeMarkup(state.q)}"></label>
    <label>Art<select name="kind">${option("", "Alle Arten", state.kind)}${option("part", "Einzelteil", state.kind)}${option("assembly", "Baugruppe", state.kind)}${option("project", "Projekt", state.kind)}</select></label>
    <label>Sortierung<select name="sort">${option("updated", "Zuletzt bearbeitet", state.sort)}${option("name", "Name", state.sort)}${option("created", "Neueste zuerst", state.sort)}</select></label>
  </form>`;
}

export function tagFacetsMarkup(tags: TagSummary[], selected: string): string {
  const active = tags.filter((tag) => tag.active && tag.modelCount > 0);
  if (active.length === 0) return "";
  return `<button type="button" class="tag-chip" data-tag-facet="" aria-pressed="${selected === ""}">Alle Tags</button>${active.map((tag) =>
    `<button type="button" class="tag-chip" data-tag-facet="${escapeMarkup(tag.id)}" aria-pressed="${tag.id === selected}" title="${escapeMarkup(tag.name)}">${escapeMarkup(tag.name)} · ${tag.modelCount}</button>`).join("")}`;
}

export function modelResultsMarkup(page: ModelSearchPage, state: ModelListState): string {
  if (page.items.length > 0) return page.items.map(modelCardMarkup).join("");
  const filtered = state.q.trim() || state.tag || state.kind;
  return filtered
    ? '<div class="empty-concept"><h2>Keine passenden Modelle</h2><p>Suche oder Filter anpassen oder zurücksetzen.</p></div>'
    : '<div class="empty-concept"><h2>Noch keine Modelle</h2><p>Importiere ein Modell, um deinen Katalog zu füllen. Indexierte Rohdateien sind nicht automatisch Modelle.</p></div>';
}

/** The full catalog list: server search, filters, and pages, with URL state. */
export function mountModelList(host: HTMLElement, sources: ModelListSources, win: Window = window): () => void {
  let state = readModelListState(win.location.search);
  let sequence = 0;
  let debounce = 0;
  const filters = host.querySelector<HTMLElement>("#model-list-toolbar")!;
  const facets = host.querySelector<HTMLElement>("#model-tag-filters")!;
  const grid = host.querySelector<HTMLElement>("#model-grid-live")!;
  const summary = host.querySelector<HTMLElement>("#model-list-summary")!;
  const pager = host.querySelector<HTMLElement>("#model-list-pager")!;
  filters.innerHTML = modelListToolbarMarkup(state);
  let tags: TagSummary[] = [];
  const renderFacets = () => { facets.innerHTML = tagFacetsMarkup(tags, state.tag); };
  const load = async (): Promise<void> => {
    const current = ++sequence;
    win.history.replaceState(win.history.state, "", modelListUrl(win.location.search, state));
    grid.setAttribute("aria-busy", "true");
    try {
      const page = await sources.search(modelSearchQuery(state));
      if (current !== sequence) return;
      if (page.items.length === 0 && page.total > 0 && state.page > 1) {
        state = { ...state, page: pageCount(page.total) };
        await load();
        return;
      }
      grid.innerHTML = modelResultsMarkup(page, state);
      summary.textContent = `${page.total} ${page.total === 1 ? "Modell" : "Modelle"}`;
      pager.innerHTML = pagerMarkup(state.page, page.total);
    } catch (error) {
      if (current !== sequence) return;
      grid.innerHTML = `<div class="empty-concept"><h2>Modelle konnten nicht geladen werden</h2><p>${escapeMarkup(error instanceof Error ? error.message : "Unbekannter Fehler")}</p></div>`;
      pager.innerHTML = "";
    } finally {
      if (current === sequence) grid.removeAttribute("aria-busy");
    }
  };
  const update = (change: Partial<ModelListState>): void => {
    state = { ...state, ...change, page: change.page ?? 1 };
    renderFacets();
    void load();
  };
  const form = filters.querySelector<HTMLFormElement>("#model-list-filters")!;
  form.addEventListener("submit", (event) => { event.preventDefault(); win.clearTimeout(debounce); update({ q: form.q.value }); });
  form.q.addEventListener("input", () => {
    win.clearTimeout(debounce);
    debounce = win.setTimeout(() => update({ q: form.q.value }), 300);
  });
  form.kind.addEventListener("change", () => update({ kind: form.kind.value }));
  form.sort.addEventListener("change", () => update({ sort: form.sort.value as ModelSort }));
  facets.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-tag-facet]");
    if (button) update({ tag: button.dataset.tagFacet === state.tag ? "" : button.dataset.tagFacet ?? "" });
  });
  grid.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-model-tag-filter]");
    if (button?.dataset.modelTagFilter) update({ tag: button.dataset.modelTagFilter });
  });
  pager.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-model-page]");
    if (button && !button.disabled) update({ page: Number(button.dataset.modelPage) });
  });
  void sources.tags().then((page) => { tags = page.items; renderFacets(); }, () => undefined);
  void load();
  return () => { sequence += 1; win.clearTimeout(debounce); };
}

/** Dashboard: the most recently edited models plus catalog-wide totals. */
export async function loadDashboardModels(host: HTMLElement, sources: ModelListSources): Promise<void> {
  const grid = host.querySelector<HTMLElement>("#model-grid-live")!;
  const page = await sources.search(`?limit=${DASHBOARD_RECENT}&offset=0`);
  grid.innerHTML = modelResultsMarkup(page, readModelListState(""));
  const count = host.querySelector<HTMLElement>("#dashboard-model-count");
  if (count) count.textContent = String(page.total);
  const files = host.querySelector<HTMLElement>("#dashboard-model-files");
  if (files) files.textContent = String(page.fileLinkTotal);
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
