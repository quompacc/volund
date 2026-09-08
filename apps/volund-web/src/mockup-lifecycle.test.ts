// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogApi, identityApi } from "./api";
import { mountMockView } from "./mockup";

afterEach(() => vi.restoreAllMocks());

describe("view request lifecycle", () => {
  it("does not let a stale model response replace the current view", async () => {
    let resolveModel!: (value: Awaited<ReturnType<typeof catalogApi.model>>) => void;
    const model = new Promise<Awaited<ReturnType<typeof catalogApi.model>>>((resolve) => { resolveModel = resolve; });
    vi.spyOn(catalogApi, "model").mockReturnValue(model);
    vi.spyOn(catalogApi, "modelProblems").mockResolvedValue([]);
    vi.spyOn(identityApi, "preferences").mockResolvedValue({ problemMinimumSeverity: "warning" } as never);
    const host = document.createElement("main");
    document.body.append(host);

    const dispose = mountMockView(host, "model-problems", vi.fn(), "model-1", true, true);
    dispose();
    host.innerHTML = '<p id="current-view">Aktuelle Ansicht</p>';
    resolveModel({ id: "model-1", name: "Verspätetes Modell" } as never);
    await model;
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(host.querySelector("#current-view")?.textContent).toBe("Aktuelle Ansicht");
  });
});
