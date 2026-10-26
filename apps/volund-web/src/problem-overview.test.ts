// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import {
  mountProblemOverview, problemBadge, problemItemMarkup, problemQuery,
  type OverviewProblem, type ProblemOverviewPage,
} from "./problem-overview";

const problem: OverviewProblem = {
  key: "m:run:0", scope: "model", modelId: "model-1", modelName: "Bracket <x>", libraryKey: null,
  severity: "error", status: "open", code: "step.invalid", message: "broken <shell>",
  sourceName: "parts/bracket.step", occurredAtUnixMs: 0, remediation: "Neu erzeugen.",
};
const library: OverviewProblem = {
  ...problem, key: "scan:1", scope: "library", modelId: null, modelName: null, libraryKey: "cad",
  code: "scan.failed", sourceName: null,
};

function page(items: OverviewProblem[], total = items.length): ProblemOverviewPage {
  return { items, limit: 50, offset: 0, total, counts: { error: 2, warning: 3, info: 1 }, truncated: false };
}

describe("problem overview", () => {
  it("builds queries and the navigation badge", () => {
    expect(problemQuery({ severity: "error", status: "all", page: 3 })).toBe("?severity=error&status=all&limit=50&offset=100");
    expect(problemBadge(page([]))).toBe("5");
    expect(problemBadge({ ...page([]), counts: { error: 0, warning: 0, info: 4 } })).toBe("");
  });

  it("escapes entries and links models and libraries", () => {
    const markup = problemItemMarkup(problem);
    expect(markup).toContain("broken &lt;shell&gt;");
    expect(markup).toContain('data-problem-model="model-1">Bracket &lt;x&gt;</button>');
    expect(markup).not.toContain("<shell>");
    expect(problemItemMarkup(library)).toContain('data-problem-library="cad"');
    expect(problemItemMarkup({ ...problem, status: "ignored" })).toContain("Fehler · ignoriert");
  });

  it("filters, pages, and opens the affected model or library", async () => {
    const load = vi.fn().mockImplementation((query: string) =>
      Promise.resolve(query.includes("offset=50") ? page([library], 60) : page([problem], 60)));
    const open = { model: vi.fn(), library: vi.fn() };
    const host = document.createElement("div");
    mountProblemOverview(host, load, open);
    await vi.waitFor(() => expect(host.textContent).toContain("2 Fehler · 3 Warnungen · 1 Hinweise"));
    expect(load).toHaveBeenCalledWith("?severity=warning&status=open&limit=50&offset=0");
    host.querySelector<HTMLButtonElement>("[data-problem-model]")!.click();
    expect(open.model).toHaveBeenCalledWith("model-1");
    host.querySelector<HTMLButtonElement>('[data-problem-page="2"]')!.click();
    await vi.waitFor(() => expect(host.querySelector("[data-problem-library]")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-problem-library]")!.click();
    expect(open.library).toHaveBeenCalled();
    const status = host.querySelector<HTMLSelectElement>('select[name="status"]')!;
    status.value = "ignored";
    status.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() => expect(load).toHaveBeenLastCalledWith("?severity=warning&status=ignored&limit=50&offset=0"));
  });

  it("explains an empty selection and a failed load", async () => {
    const empty = document.createElement("div");
    mountProblemOverview(empty, () => Promise.resolve({ ...page([]), counts: { error: 0, warning: 0, info: 0 } }), { model: vi.fn(), library: vi.fn() });
    await vi.waitFor(() => expect(empty.textContent).toContain("Keine Probleme in dieser Auswahl"));
    const failing = document.createElement("div");
    mountProblemOverview(failing, () => Promise.reject(new Error("db <down>")), { model: vi.fn(), library: vi.fn() });
    await vi.waitFor(() => expect(failing.innerHTML).toContain("db &lt;down&gt;"));
  });
});
