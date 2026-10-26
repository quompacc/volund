import { mountBulkEdit, selectableCardMarkup, type BulkEditSources } from "./model-bulk-edit";
import { modelCardMarkup } from "./model-presentation";
import type { ModelSearchPage, SavedView, SavedViewQuery } from "./model-list-types";
import type { Page, TagSummary } from "./types";

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
  /** Present only for accounts that may edit the catalog. */
  bulk?: BulkEditSources;
  savedViews?: {
    list: () => Promise<SavedView[]>;
    create: (name: string, query: SavedViewQuery) => Promise<SavedView>;
    remove: (id: string) => Promise<void>;
  };
}

/** The persisted part of a list state: filters, never the page. */
export function savedViewQuery(state: ModelListState): SavedViewQuery {
  const query: SavedViewQuery = {};
  if (state.q.trim()) query.q = state.q.trim();
  if (state.tag) query.tag = state.tag;
  if (state.kind) query.kind = state.kind;
  if (state.sort !== "updated") query.sort = state.sort;
  return query;
}

export function stateFromSavedView(view: SavedView): ModelListState {
  return readModelListState(`?${new URLSearchParams(view.query as Record<string, string>)}`);
}

export function savedViewsMarkup(views: SavedView[], selected: string): string {
  return `<label>Gespeicherte Ansicht<select name="savedView"><option value="">Keine</option>${views.map((view) =>
    `<option value="${escapeMarkup(view.id)}"${view.id === selected ? " selected" : ""}>${escapeMarkup(view.name)}</option>`).join("")}</select></label>
    <button type="button" class="secondary-action" data-delete-view${selected ? "" : " disabled"}>Ansicht löschen</button>
    <form id="save-view-form" class="save-view-form"><label>Neue Ansicht<input name="viewName" maxlength="80" required placeholder="Name der aktuellen Filter"></label><button class="secondary-action" type="submit">Ansicht speichern</button></form>
    <p id="saved-view-message" class="form-message" aria-live="polite"></p>`;
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

export function modelResultsMarkup(
  page: ModelSearchPage,
  state: ModelListState,
  wrap: (model: ModelSearchPage["items"][number], card: string) => string = (_model, card) => card,
): string {
  if (page.items.length > 0) return page.items.map((model) => wrap(model, modelCardMarkup(model))).join("");
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
  filters.innerHTML = `${modelListToolbarMarkup(state)}<div id="saved-views" class="saved-views"></div>`;
  let tags: TagSummary[] = [];
  const bar = host.querySelector<HTMLElement>("#model-bulk-bar");
  const bulk = sources.bulk && bar ? mountBulkEdit(grid, bar, sources.bulk, () => tags, () => void load(), win) : null;
  const wrap = bulk ? (model: ModelSearchPage["items"][number], card: string) => selectableCardMarkup(model, card, bulk.selection.has(model.id)) : undefined;
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
      grid.innerHTML = modelResultsMarkup(page, state, wrap);
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
  if (sources.savedViews) mountSavedViews(filters.querySelector<HTMLElement>("#saved-views")!, sources.savedViews, () => state, (next) => {
    form.q.value = next.q;
    form.kind.value = next.kind;
    form.sort.value = next.sort;
    update(next);
  });
  void load();
  return () => { sequence += 1; win.clearTimeout(debounce); };
}

function mountSavedViews(
  host: HTMLElement,
  api: NonNullable<ModelListSources["savedViews"]>,
  current: () => ModelListState,
  apply: (state: ModelListState) => void,
): void {
  let views: SavedView[] = [];
  let selected = "";
  const message = (text: string) => { host.querySelector<HTMLElement>("#saved-view-message")!.textContent = text; };
  const render = () => { host.innerHTML = savedViewsMarkup(views, selected); };
  const refresh = async () => { views = await api.list(); render(); };
  host.addEventListener("change", (event) => {
    const select = (event.target as HTMLElement).closest<HTMLSelectElement>('select[name="savedView"]');
    if (!select) return;
    selected = select.value;
    const view = views.find((item) => item.id === selected);
    render();
    if (view) apply(stateFromSavedView(view));
  });
  host.addEventListener("click", (event) => {
    const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-delete-view]");
    if (!button || !selected) return;
    void api.remove(selected).then(async () => { selected = ""; await refresh(); message("Ansicht gelöscht."); },
      (error: Error) => message(error.message));
  });
  host.addEventListener("submit", (event) => {
    const form = (event.target as HTMLElement).closest<HTMLFormElement>("#save-view-form");
    if (!form) return;
    event.preventDefault();
    const name = (form.elements.namedItem("viewName") as HTMLInputElement).value.trim();
    if (!name) return;
    void api.create(name, savedViewQuery(current())).then(async (view) => {
      selected = view.id;
      await refresh();
      message(`Ansicht „${view.name}“ gespeichert.`);
    }, (error: Error) => message(error.message));
  });
  render();
  void refresh().catch(() => message("Gespeicherte Ansichten sind nicht verfügbar."));
}

/** Says when the dashboard shows only the most recent part of the catalog. */
export function dashboardMoreMarkup(shown: number, total: number): string {
  if (total <= shown) return "";
  return `<p id="dashboard-model-more" class="dashboard-more">Zuletzt bearbeitet: ${shown} von ${total} Modellen · <button type="button" class="text-action" data-navigate="models">Alle ${total} Modelle anzeigen</button></p>`;
}

/** Dashboard: the most recently edited models plus catalog-wide totals. */
export async function loadDashboardModels(host: HTMLElement, sources: ModelListSources): Promise<void> {
  const grid = host.querySelector<HTMLElement>("#model-grid-live")!;
  const page = await sources.search(`?limit=${DASHBOARD_RECENT}&offset=0`);
  grid.innerHTML = modelResultsMarkup(page, readModelListState(""));
  host.querySelector("#dashboard-model-more")?.remove();
  grid.insertAdjacentHTML("afterend", dashboardMoreMarkup(page.items.length, page.total));
  const count = host.querySelector<HTMLElement>("#dashboard-model-count");
  if (count) count.textContent = String(page.total);
  const files = host.querySelector<HTMLElement>("#dashboard-model-files");
  if (files) files.textContent = String(page.fileLinkTotal);
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
