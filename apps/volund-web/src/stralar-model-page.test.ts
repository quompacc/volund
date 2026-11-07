// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogApi, identityApi } from "./api";
import { mountCatalogView } from "./catalog-views";
import type { ModelFile, ModelSummary, UserPreferences } from "./types";

vi.mock("./pdf-viewer", () => ({ PdfViewer: class {}, pdfViewerMarkup: vi.fn() }));
vi.mock("./viewer", async () => {
  const actual = await vi.importActual<typeof import("./viewer")>("./viewer");
  return {
    ...actual,
    CadViewer: class {
      dispose(): void {}
      load(): Promise<void> { return Promise.resolve(); }
      refreshLayout(): void {}
      selectObject(): void {}
      setObjectVisible(): void {}
      isolateObject(): void {}
      resetVisibility(): void {}
    },
  };
});

const model: ModelSummary = {
  id: "model-1", slug: "model-1", name: "Lampenhalter", description: "", kind: "part",
  licenseKind: "not-specified", licenseValue: null, authorName: null, primaryFileId: "step-1",
  fileCount: 2, formats: ["step", "stl"], updatedAtUnixMs: 1, tags: [], tagIds: [], collections: [],
  viewerRotation: [0, 0, 0], revision: 1,
  thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
};
const fileBase: Omit<ModelFile, "id" | "path" | "format" | "role" | "primary" | "printable"> = {
  rootKey: "main", rootName: "Main", byteSize: 12, modifiedAtUnixMs: 1, missing: false, revision: 1,
  caption: "", description: "", notes: "", printed: false, preSupported: false, upAxis: null, supportHint: "",
  orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
};
const step: ModelFile = { ...fileBase, id: "step-1", path: "lampe/halter.step", format: "step", role: "master-cad", primary: true, printable: false };
const stl: ModelFile = { ...fileBase, id: "stl-1", path: "lampe/halter.stl", format: "stl", role: "printable-mesh", primary: false, printable: true };
const preferences: UserPreferences = {
  previewAutoLoad: "manual", background: "dark", gridVisible: true, contrast: "balanced",
  renderStyle: "solid", problemMinimumSeverity: "warning", revision: 1,
};

async function mountModelPage(available: boolean): Promise<HTMLElement> {
  vi.spyOn(catalogApi, "model").mockResolvedValue(model);
  vi.spyOn(catalogApi, "modelFiles").mockResolvedValue({ items: [step, stl], total: 2, limit: 200, offset: 0 });
  vi.spyOn(catalogApi, "collections").mockResolvedValue([]);
  vi.spyOn(catalogApi, "tags").mockResolvedValue({ items: [], total: 0, limit: 100, offset: 0 });
  vi.spyOn(catalogApi, "thumbnailCandidates").mockResolvedValue([]);
  vi.spyOn(catalogApi, "slicerTargets").mockResolvedValue([]);
  vi.spyOn(catalogApi, "modelProblems").mockResolvedValue([]);
  vi.spyOn(catalogApi, "modelHistory").mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
  vi.spyOn(catalogApi, "previews").mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
  vi.spyOn(catalogApi, "enqueuePreview").mockResolvedValue({ id: "queued", status: "queued" });
  vi.spyOn(catalogApi, "stralar").mockResolvedValue({ available });
  vi.spyOn(identityApi, "settings").mockResolvedValue([]);
  vi.spyOn(identityApi, "preferences").mockResolvedValue(preferences);
  const host = document.createElement("main");
  document.body.append(host);
  mountCatalogView(host, "model", vi.fn(), model.id, false, false);
  await vi.waitFor(() => { if (!host.querySelector(".model-header")) throw new Error("not rendered"); });
  return host;
}

describe("Stralar on the model page (in-app)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("shows 'In Stralar öffnen' in the header with the printable mesh", async () => {
    const host = await mountModelPage(true);
    const action = host.querySelector<HTMLAnchorElement>(".model-header [data-stralar-open]");
    expect(action?.textContent).toContain("In Stralar öffnen");
    expect(action?.getAttribute("href")).toContain(encodeURIComponent("/api/v1/files/stl-1/content"));
    expect(action?.target).toBe("_blank");
  });

  it("offers it in the file menu for STEP and STL files", async () => {
    const host = await mountModelPage(true);
    host.querySelector<HTMLButtonElement>('[data-file-menu="step-1"]')!.click();
    const link = host.querySelector<HTMLAnchorElement>('[data-context-action="stralar"]')!;
    expect(link.hidden).toBe(false);
    expect(link.getAttribute("href")).toContain(encodeURIComponent("/api/v1/files/step-1/content"));
  });

  it("shows nothing when the server has no Stralar", async () => {
    const host = await mountModelPage(false);
    expect(host.querySelector("[data-stralar-open]")).toBeNull();
    host.querySelector<HTMLButtonElement>('[data-file-menu="stl-1"]')!.click();
    expect(host.querySelector<HTMLAnchorElement>('[data-context-action="stralar"]')!.hidden).toBe(true);
  });
});
