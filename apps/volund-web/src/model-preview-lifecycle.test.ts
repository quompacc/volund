// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogApi, identityApi } from "./api";
import { mountMockView } from "./mockup";
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

describe("model preview lifecycle", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("shows the bounded background state when no preview becomes available", async () => {
    vi.useFakeTimers();
    const model: ModelSummary = {
      id: "model-1", slug: "model-1", name: "Modell ohne Vorschau", description: "", kind: "part",
      licenseKind: "not-specified", licenseValue: null, authorName: null, primaryFileId: "file-1",
      fileCount: 1, formats: ["obj"], updatedAtUnixMs: 1, tags: [], tagIds: [], collections: [],
      viewerRotation: [0, 0, 0], revision: 1,
      thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
    };
    const file: ModelFile = {
      id: "file-1", path: "missing-preview.obj", rootKey: "test", rootName: "Test", role: "cad",
      primary: true, format: "obj", byteSize: 12, modifiedAtUnixMs: 1, missing: false, revision: 1,
      caption: "", description: "", notes: "", printable: false, printed: false, preSupported: false,
      upAxis: null, supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
    };
    const preferences: UserPreferences = {
      previewAutoLoad: "selected", background: "dark", gridVisible: true, contrast: "balanced",
      renderStyle: "solid", problemMinimumSeverity: "warning", revision: 1,
    };
    vi.spyOn(catalogApi, "model").mockResolvedValue(model);
    vi.spyOn(catalogApi, "modelFiles").mockResolvedValue({ items: [file], total: 1, limit: 200, offset: 0 });
    vi.spyOn(catalogApi, "collections").mockResolvedValue([]);
    vi.spyOn(catalogApi, "tags").mockResolvedValue({ items: [], total: 0, limit: 100, offset: 0 });
    vi.spyOn(catalogApi, "thumbnailCandidates").mockResolvedValue([]);
    vi.spyOn(catalogApi, "slicerTargets").mockResolvedValue([]);
    vi.spyOn(catalogApi, "modelProblems").mockResolvedValue([]);
    vi.spyOn(catalogApi, "modelHistory").mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    const previews = vi.spyOn(catalogApi, "previews").mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    const enqueue = vi.spyOn(catalogApi, "enqueuePreview").mockResolvedValue({ id: "queued", status: "queued" });
    vi.spyOn(identityApi, "settings").mockResolvedValue([]);
    vi.spyOn(identityApi, "preferences").mockResolvedValue(preferences);
    const host = document.createElement("main");
    document.body.append(host);

    const dispose = mountMockView(host, "model", vi.fn(), model.id, false, false);
    await vi.advanceTimersByTimeAsync(0);
    expect(enqueue).toHaveBeenCalledOnce();
    await vi.runAllTimersAsync();

    expect(previews).toHaveBeenCalledTimes(91);
    expect(host.textContent).toContain("3D-Vorschau wird im Hintergrund erzeugt");
    dispose();
  });
});
