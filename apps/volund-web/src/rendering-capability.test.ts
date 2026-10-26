// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogApi, identityApi } from "./api";
import { mountCatalogView } from "./catalog-views";
import {
  SOFTWARE_PREVIEW_LIMIT_BYTES, classifyRendering, previewGate, previewGateMarkup, resetRenderingCapability, tileAllowed,
} from "./rendering-capability";
import type { ModelFile, ModelSummary, UserPreferences } from "./types";

const loads = vi.hoisted(() => [] as string[]);
vi.mock("./pdf-viewer", () => ({ PdfViewer: class {}, pdfViewerMarkup: vi.fn() }));
vi.mock("./viewer", async () => {
  const actual = await vi.importActual<typeof import("./viewer")>("./viewer");
  return {
    ...actual,
    CadViewer: class {
      dispose(): void {}
      load(url: string): Promise<void> { loads.push(url); return Promise.resolve(); }
      refreshLayout(): void {}
      selectObject(): void {}
      setObjectVisible(): void {}
      isolateObject(): void {}
      resetVisibility(): void {}
    },
  };
});

function context(renderer: string) {
  return {
    RENDERER: 0x1f01,
    getExtension: (name: string) => name === "WEBGL_debug_renderer_info" ? { UNMASKED_RENDERER_WEBGL: 0x9246 } : null,
    getParameter: () => renderer,
  } as unknown as WebGL2RenderingContext;
}

describe("rendering capability", () => {
  it("classifies GPU, software, and missing WebGL", () => {
    expect(classifyRendering(() => context("ANGLE (NVIDIA GeForce RTX 4070)"))).toBe("hardware");
    expect(classifyRendering(() => context("ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device))"))).toBe("software");
    expect(classifyRendering(() => context("llvmpipe (LLVM 17.0.6, 256 bits)"))).toBe("software");
    // A major performance caveat refuses the strict context but allows the lenient one.
    expect(classifyRendering((attributes) => attributes?.failIfMajorPerformanceCaveat ? null : context("x"))).toBe("software");
    expect(classifyRendering(() => null)).toBe("none");
    expect(classifyRendering(() => { throw new Error("blocked"); })).toBe("none");
  });

  it("gates only large previews on software rendering, and everything without WebGL", () => {
    const large = SOFTWARE_PREVIEW_LIMIT_BYTES + 1;
    expect(previewGate("hardware", large, false)).toBe("render");
    expect(previewGate("software", SOFTWARE_PREVIEW_LIMIT_BYTES, false)).toBe("render");
    expect(previewGate("software", large, false)).toBe("confirm");
    expect(previewGate("software", large, true)).toBe("render");
    expect(previewGate("none", 1, true)).toBe("unavailable");
    expect(tileAllowed("hardware", large)).toBe(true);
    expect(tileAllowed("software", 3 * 1024 * 1024)).toBe(false);
    expect(tileAllowed("none", 1)).toBe(false);
    expect(previewGateMarkup("confirm", "Rahmen <A>", "9.0 MiB")).toContain("Rahmen &lt;A&gt;");
  });
});

describe("model page without a usable GPU", () => {
  afterEach(() => {
    resetRenderingCapability();
    vi.restoreAllMocks();
    loads.length = 0;
    document.body.innerHTML = "";
  });

  function mount(byteSize: number) {
    const model: ModelSummary = {
      id: "model-1", slug: "model-1", name: "Rahmen", description: "", kind: "part",
      licenseKind: "not-specified", licenseValue: null, authorName: null, primaryFileId: "file-1",
      fileCount: 1, formats: ["step"], updatedAtUnixMs: 1, tags: [], tagIds: [], collections: [],
      viewerRotation: [0, 0, 0], revision: 1,
      thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
    };
    const file: ModelFile = {
      id: "file-1", path: "frame.obj", rootKey: "test", rootName: "Test", role: "cad",
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
    vi.spyOn(catalogApi, "previews").mockResolvedValue({
      items: [{ id: "p-1", status: "ready", artifacts: [{ kind: "preview-glb", url: "/preview.glb", byteSize }] }],
      total: 1, limit: 50, offset: 0,
    } as never);
    vi.spyOn(identityApi, "settings").mockResolvedValue([]);
    vi.spyOn(identityApi, "preferences").mockResolvedValue(preferences);
    const host = document.createElement("main");
    document.body.append(host);
    return { host, dispose: mountCatalogView(host, "model", vi.fn(), model.id, false, false) };
  }

  it("explains a missing WebGL instead of starting a viewer", async () => {
    resetRenderingCapability("none");
    const { host, dispose } = mount(1024);
    await vi.waitFor(() => expect(host.textContent).toContain("Dieser Browser stellt kein WebGL bereit"));
    expect(loads).toEqual([]);
    dispose();
  });

  it("loads a large preview on software rendering only on request", async () => {
    resetRenderingCapability("software");
    const { host, dispose } = mount(SOFTWARE_PREVIEW_LIMIT_BYTES * 2);
    await vi.waitFor(() => expect(host.querySelector("[data-load-preview-anyway]")).not.toBeNull());
    expect(loads).toEqual([]);
    host.querySelector<HTMLButtonElement>("[data-load-preview-anyway]")!.click();
    await vi.waitFor(() => expect(loads).toEqual(["/preview.glb"]));
    dispose();
  });

  it("loads small previews on software rendering right away", async () => {
    resetRenderingCapability("software");
    const { dispose } = mount(1024);
    await vi.waitFor(() => expect(loads).toEqual(["/preview.glb"]));
    dispose();
  });
});
