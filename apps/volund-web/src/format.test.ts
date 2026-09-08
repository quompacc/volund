import { describe, expect, it } from "vitest";
import { fileName, formatBytes, newestReadyPreview, parentPath, previewLabel } from "./format";
import type { Preview } from "./types";

describe("catalog formatting", () => {
  it("separates portable relative paths", () => {
    expect(fileName("assemblies/voron.step")).toBe("voron.step");
    expect(parentPath("assemblies/voron.step")).toBe("assemblies");
    expect(parentPath("part.stl")).toBe("Wurzelverzeichnis");
  });

  it("formats binary file sizes", () => {
    expect(formatBytes(800)).toBe("800 B");
    expect(formatBytes(1_572_864)).toBe("1,5 MB");
  });

  it("selects only ready previews that contain a GLB", () => {
    const base: Preview = {
      id: "one", profile: "web", status: "running", converterVersion: "test",
      requestedAtUnixMs: 0, finishedAtUnixMs: null, artifacts: [],
    };
    const ready: Preview = {
      ...base, id: "two", status: "ready",
      artifacts: [{ kind: "preview-glb", url: "/model", sha256: "a".repeat(64), byteSize: 3, mediaType: "model/gltf-binary" }],
    };
    expect(newestReadyPreview([base, ready])).toBe(ready);
    expect(newestReadyPreview([{ ...ready, artifacts: [] }])).toBeUndefined();
    expect(previewLabel("queued")).toBe("Vorschau eingeplant");
    expect(previewLabel("custom")).toBe("custom");
  });
});
