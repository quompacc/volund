// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { bulkBarMarkup, bulkChanges, bulkRequest, mountBulkEdit, selectableCardMarkup } from "./model-bulk-edit";
import type { ModelSummary, TagSummary } from "./types";

const model = { id: "m-1", revision: 4 } as ModelSummary;
const tag = { id: "t-1", name: "Halter <b>", active: true, modelCount: 1 } as TagSummary;

function form(markup: string): HTMLFormElement {
  const host = document.createElement("div");
  host.innerHTML = markup;
  return host.querySelector("form")!;
}

describe("bulk model edit", () => {
  it("renders selectable cards and an escaped action bar", () => {
    expect(selectableCardMarkup(model, "<article></article>", true)).toContain('data-bulk-model="m-1" data-bulk-revision="4" checked');
    expect(bulkBarMarkup(0, [tag], [])).toBe("");
    const bar = bulkBarMarkup(2, [tag], [{ id: "c-1", name: "Regal", slug: "regal", description: "", modelCount: 0, revision: 1 } as never]);
    expect(bar).toContain("2 Modelle ausgewählt");
    expect(bar).toContain("Halter &lt;b&gt;");
    expect(bar).toContain('value="c-1">Regal');
  });

  it("turns the form into only the requested changes", () => {
    const bar = form(bulkBarMarkup(1, [tag], []));
    expect(bulkChanges(bar)).toEqual({});
    (bar.elements.namedItem("addTag") as HTMLSelectElement).value = "t-1";
    (bar.elements.namedItem("kind") as HTMLSelectElement).value = "assembly";
    (bar.elements.namedItem("author") as HTMLInputElement).value = "  Grace  ";
    expect(bulkChanges(bar)).toEqual({ addTagIds: ["t-1"], kind: "assembly", authorName: "Grace" });
    (bar.elements.namedItem("clearAuthor") as HTMLInputElement).checked = true;
    expect(bulkChanges(bar).authorName).toBe("");
  });

  it("adds the confirmation phrase from ten models on", () => {
    const nine = new Map(Array.from({ length: 9 }, (_, index) => [`m-${index}`, 1] as [string, number]));
    expect(bulkRequest(nine, { kind: "part" }).confirmation).toBeUndefined();
    nine.set("m-9", 2);
    const request = bulkRequest(nine, { kind: "part" });
    expect(request.confirmation).toBe("EDIT 10 MODELS");
    expect(request.models.at(-1)).toEqual({ id: "m-9", expectedRevision: 2 });
  });

  it("selects cards, applies changes, reloads, and reports refusals", async () => {
    const grid = document.createElement("div");
    grid.innerHTML = selectableCardMarkup(model, "<article>Card</article>", false);
    const bar = document.createElement("div");
    const apply = vi.fn().mockResolvedValueOnce([model]).mockRejectedValueOnce(new Error("models changed meanwhile: m-1"));
    const reload = vi.fn();
    const bulk = mountBulkEdit(grid, bar, { apply, collections: () => Promise.resolve([]) }, () => [tag], reload);
    const box = grid.querySelector<HTMLInputElement>("[data-bulk-model]")!;
    box.checked = true;
    box.dispatchEvent(new Event("change", { bubbles: true }));
    expect(bar.textContent).toContain("1 Modell ausgewählt");
    const submit = () => bar.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    submit();
    expect(bar.textContent).toContain("Wähle mindestens eine Änderung.");
    (bar.querySelector("form")!.elements.namedItem("kind") as HTMLSelectElement).value = "project";
    submit();
    await vi.waitFor(() => expect(bar.textContent).toContain("1 Modell aktualisiert."));
    expect(apply).toHaveBeenCalledWith({ models: [{ id: "m-1", expectedRevision: 4 }], changes: { kind: "project" } });
    expect(reload).toHaveBeenCalled();
    expect(bulk.selection.size).toBe(0);

    box.checked = true;
    box.dispatchEvent(new Event("change", { bubbles: true }));
    (bar.querySelector("form")!.elements.namedItem("kind") as HTMLSelectElement).value = "part";
    submit();
    await vi.waitFor(() => expect(bar.textContent).toContain("models changed meanwhile"));
    expect(bulk.selection.size).toBe(1);
    bar.querySelector<HTMLButtonElement>("[data-bulk-clear]")!.click();
    expect(bulk.selection.size).toBe(0);
    expect(box.checked).toBe(false);
  });

  it("asks before changing ten or more models", () => {
    const grid = document.createElement("div");
    grid.innerHTML = Array.from({ length: 10 }, (_, index) => selectableCardMarkup({ id: `m-${index}`, revision: 1 } as ModelSummary, "", false)).join("");
    const bar = document.createElement("div");
    const apply = vi.fn().mockResolvedValue([]);
    const win = Object.assign(Object.create(window), { confirm: vi.fn().mockReturnValue(false) }) as Window;
    mountBulkEdit(grid, bar, { apply, collections: () => Promise.resolve([]) }, () => [], vi.fn(), win);
    grid.querySelectorAll<HTMLInputElement>("[data-bulk-model]").forEach((box) => {
      box.checked = true;
      box.dispatchEvent(new Event("change", { bubbles: true }));
    });
    (bar.querySelector("form")!.elements.namedItem("kind") as HTMLSelectElement).value = "part";
    bar.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    expect(win.confirm).toHaveBeenCalledWith("10 Modelle gemeinsam ändern?");
    expect(apply).not.toHaveBeenCalled();
  });
});
