// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  loadDashboardModels, modelListUrl, modelSearchQuery, mountModelList, pageCount, pagerMarkup,
  readModelListState, savedViewQuery, savedViewsMarkup, stateFromSavedView, tagFacetsMarkup,
} from "./model-list-view";
import type { ModelSearchPage, SavedView } from "./model-list-types";
import type { ModelSummary, TagSummary } from "./types";

function model(name: string): ModelSummary {
  return {
    id: `id-${name}`, slug: name.toLowerCase(), name, description: "", kind: "part",
    licenseKind: "not-specified", licenseValue: null, authorName: null, primaryFileId: null,
    fileCount: 1, formats: ["step"], updatedAtUnixMs: 0, tags: [], tagIds: [], collections: [],
    viewerRotation: [0, 0, 0], revision: 1,
    thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
  };
}

function page(names: string[], total = names.length, offset = 0): ModelSearchPage {
  return { items: names.map(model), limit: 24, offset, total, fileLinkTotal: total * 2 };
}

const tag = { id: "11111111-2222-3333-4444-555555555555", name: "Halter <b>", active: true, modelCount: 2 } as TagSummary;

function host(): HTMLElement {
  const element = document.createElement("div");
  element.innerHTML = '<div id="model-list-toolbar"></div><div id="model-tag-filters"></div><p id="model-list-summary"></p><div id="model-grid-live"></div><div id="model-list-pager"></div>';
  return element;
}

beforeEach(() => { window.history.replaceState({}, "", "/?view=models"); });
afterEach(() => { vi.useRealTimers(); });

describe("model list URL state", () => {
  it("round-trips filters through the page URL and keeps the view", () => {
    const state = { q: " bracket ", tag: tag.id, kind: "part", sort: "name" as const, page: 3 };
    const url = modelListUrl("?view=models&q=old", state);
    expect(url).toBe(`?view=models&q=bracket&tag=${tag.id}&kind=part&sort=name&page=3`);
    expect(readModelListState(url)).toEqual({ ...state, q: "bracket" });
    expect(modelSearchQuery(state)).toBe(`?q=bracket&tag=${tag.id}&kind=part&sort=name&limit=24&offset=48`);
  });

  it("falls back to safe defaults for foreign or broken values", () => {
    expect(readModelListState("?kind=statue&sort=random&page=-2")).toEqual({ q: "", tag: "", kind: "", sort: "updated", page: 1 });
    expect(modelSearchQuery(readModelListState(""))).toBe("?limit=24&offset=0");
    expect(modelListUrl("?view=models", readModelListState(""))).toBe("?view=models");
  });

  it("pages and escapes facets", () => {
    expect(pageCount(0)).toBe(1);
    expect(pageCount(49)).toBe(3);
    expect(pagerMarkup(1, 10)).toBe("");
    const pager = pagerMarkup(1, 49);
    expect(pager).toContain("Seite 1 von 3");
    expect(pager).toMatch(/data-model-page="0" disabled/);
    expect(tagFacetsMarkup([tag], tag.id)).toContain("Halter &lt;b&gt; · 2");
    expect(tagFacetsMarkup([tag], tag.id)).toContain('aria-pressed="true" title');
  });
});

describe("mounted model list", () => {
  it("searches on the server, debounces typing, and pages", async () => {
    vi.useFakeTimers();
    const search = vi.fn().mockImplementation((query: string) =>
      Promise.resolve(query.includes("offset=24") ? page(["Zeta"], 25, 24) : page(["Alpha", "Beta"], 25)));
    const element = host();
    mountModelList(element, { search, tags: () => Promise.resolve({ items: [tag], limit: 100, offset: 0, total: 1 }) });
    await vi.waitFor(() => expect(element.querySelector("#model-list-summary")!.textContent).toBe("25 Modelle"));
    expect(element.querySelector("#model-grid-live")!.textContent).toContain("Alpha");
    const input = element.querySelector<HTMLInputElement>('input[name="q"]')!;
    input.value = "be";
    input.dispatchEvent(new Event("input"));
    input.value = "beta";
    input.dispatchEvent(new Event("input"));
    vi.advanceTimersByTime(300);
    await vi.waitFor(() => expect(search).toHaveBeenLastCalledWith("?q=beta&limit=24&offset=0"));
    expect(search.mock.calls.filter(([query]) => String(query).includes("q=be&")).length).toBe(0);
    expect(window.location.search).toBe("?view=models&q=beta");
    element.querySelector<HTMLButtonElement>('[data-model-page="2"]')!.click();
    await vi.waitFor(() => expect(element.querySelector("#model-grid-live")!.textContent).toContain("Zeta"));
    expect(window.location.search).toBe("?view=models&q=beta&page=2");
    const kind = element.querySelector<HTMLSelectElement>('select[name="kind"]')!;
    kind.value = "assembly";
    kind.dispatchEvent(new Event("change"));
    await vi.waitFor(() => expect(search).toHaveBeenLastCalledWith("?q=beta&kind=assembly&limit=24&offset=0"));
  });

  it("ignores a slower stale response and clamps a page past the end", async () => {
    let resolveFirst: (value: ModelSearchPage) => void = () => undefined;
    window.history.replaceState({}, "", "/?view=models&page=9");
    const search = vi.fn()
      .mockImplementationOnce(() => new Promise<ModelSearchPage>((resolve) => { resolveFirst = resolve; }))
      .mockImplementation((query: string) => Promise.resolve(query.includes("offset=0") ? page(["Fresh"], 3) : page([], 3)));
    const element = host();
    mountModelList(element, { search, tags: () => Promise.reject(new Error("no tags")) });
    const sort = element.querySelector<HTMLSelectElement>('select[name="sort"]')!;
    sort.value = "name";
    sort.dispatchEvent(new Event("change"));
    await vi.waitFor(() => expect(element.querySelector("#model-grid-live")!.textContent).toContain("Fresh"));
    resolveFirst(page(["Stale"]));
    await Promise.resolve();
    expect(element.querySelector("#model-grid-live")!.textContent).not.toContain("Stale");

    window.history.replaceState({}, "", "/?view=models&page=9");
    const clamped = host();
    const clampSearch = vi.fn().mockImplementation((query: string) =>
      Promise.resolve(query.includes("offset=0") ? page(["Only"], 3) : page([], 3)));
    mountModelList(clamped, { search: clampSearch, tags: () => Promise.resolve({ items: [], limit: 100, offset: 0, total: 0 }) });
    await vi.waitFor(() => expect(clamped.querySelector("#model-grid-live")!.textContent).toContain("Only"));
    expect(window.location.search).toBe("?view=models");
  });

  it("explains filtered and failed results", async () => {
    window.history.replaceState({}, "", "/?view=models&q=nothing");
    const element = host();
    mountModelList(element, { search: () => Promise.resolve(page([], 0)), tags: () => Promise.resolve({ items: [], limit: 100, offset: 0, total: 0 }) });
    await vi.waitFor(() => expect(element.textContent).toContain("Keine passenden Modelle"));
    const failing = host();
    mountModelList(failing, { search: () => Promise.reject(new Error("db <down>")), tags: () => Promise.resolve({ items: [], limit: 100, offset: 0, total: 0 }) });
    await vi.waitFor(() => expect(failing.textContent).toContain("db <down>"));
    expect(failing.innerHTML).toContain("db &lt;down&gt;");
  });

  it("fills the dashboard with recent models and catalog-wide totals", async () => {
    const element = document.createElement("div");
    element.innerHTML = '<strong id="dashboard-model-count"></strong><strong id="dashboard-model-files"></strong><div id="model-grid-live"></div>';
    const search = vi.fn().mockResolvedValue(page(["Recent"], 40));
    await loadDashboardModels(element, { search, tags: vi.fn() });
    expect(search).toHaveBeenCalledWith("?limit=6&offset=0");
    expect(element.querySelector("#dashboard-model-count")!.textContent).toBe("40");
    expect(element.querySelector("#dashboard-model-files")!.textContent).toBe("80");
    expect(element.querySelector("#model-grid-live")!.textContent).toContain("Recent");
  });
});

describe("saved model list views", () => {
  const view: SavedView = { id: "view-1", name: "Teile <A>", query: { q: "bracket", kind: "part", sort: "name" }, createdAtUnixMs: 0 };

  it("stores filters but never the page, and restores them", () => {
    const state = { q: " bracket ", tag: "", kind: "part", sort: "name" as const, page: 4 };
    expect(savedViewQuery(state)).toEqual({ q: "bracket", kind: "part", sort: "name" });
    expect(savedViewQuery(readModelListState(""))).toEqual({});
    expect(stateFromSavedView(view)).toEqual({ q: "bracket", tag: "", kind: "part", sort: "name", page: 1 });
    expect(savedViewsMarkup([view], "view-1")).toContain(">Teile &lt;A&gt;</option>");
    expect(savedViewsMarkup([view], "")).toMatch(/data-delete-view disabled/);
  });

  it("applies, saves, and deletes views from the toolbar", async () => {
    const search = vi.fn().mockResolvedValue(page(["Alpha"]));
    const views = [view];
    const api = {
      list: vi.fn().mockImplementation(() => Promise.resolve([...views])),
      create: vi.fn().mockImplementation((name: string, query: SavedView["query"]) => {
        const created = { id: "view-2", name, query, createdAtUnixMs: 1 };
        views.push(created);
        return Promise.resolve(created);
      }),
      remove: vi.fn().mockImplementation((id: string) => {
        views.splice(views.findIndex((item) => item.id === id), 1);
        return Promise.resolve();
      }),
    };
    const element = host();
    mountModelList(element, { search, tags: () => Promise.resolve({ items: [], limit: 100, offset: 0, total: 0 }), savedViews: api });
    await vi.waitFor(() => expect(element.querySelector('select[name="savedView"] option[value="view-1"]')).not.toBeNull());
    const select = element.querySelector<HTMLSelectElement>('select[name="savedView"]')!;
    select.value = "view-1";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => expect(search).toHaveBeenLastCalledWith("?q=bracket&kind=part&sort=name&limit=24&offset=0"));
    expect(element.querySelector<HTMLInputElement>('input[name="q"]')!.value).toBe("bracket");
    expect(element.querySelector<HTMLSelectElement>('select[name="kind"]')!.value).toBe("part");

    const name = element.querySelector<HTMLInputElement>('input[name="viewName"]')!;
    name.value = "Meine Teile";
    element.querySelector<HTMLFormElement>("#save-view-form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(element.textContent).toContain("Ansicht „Meine Teile“ gespeichert."));
    expect(api.create).toHaveBeenCalledWith("Meine Teile", { q: "bracket", kind: "part", sort: "name" });
    expect(element.querySelector('option[value="view-2"]')!.hasAttribute("selected")).toBe(true);

    element.querySelector<HTMLButtonElement>("[data-delete-view]")!.click();
    await vi.waitFor(() => expect(element.textContent).toContain("Ansicht gelöscht."));
    expect(api.remove).toHaveBeenCalledWith("view-2");
    expect(element.querySelector('option[value="view-2"]')).toBeNull();
  });

  it("reports a refused save without losing the filters", async () => {
    const api = {
      list: () => Promise.resolve([]),
      create: () => Promise.reject(new Error("a saved view named 'X' already exists")),
      remove: vi.fn(),
    };
    const element = host();
    mountModelList(element, { search: () => Promise.resolve(page([])), tags: () => Promise.resolve({ items: [], limit: 100, offset: 0, total: 0 }), savedViews: api });
    await vi.waitFor(() => expect(element.querySelector("#save-view-form")).not.toBeNull());
    element.querySelector<HTMLInputElement>('input[name="viewName"]')!.value = "X";
    element.querySelector<HTMLFormElement>("#save-view-form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(element.textContent).toContain("already exists"));
  });
});
