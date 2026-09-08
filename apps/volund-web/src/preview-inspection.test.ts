import { describe, expect, it } from "vitest";
import { previewInspectionMarkup } from "./preview-inspection";
import type { ModelFile, Preview } from "./types";

describe("preview inspection lifecycle", () => {
  it("keeps source, profile, status, and derived artifacts explicit", () => {
    const file = { id: "file", path: "CAD/<main>.step" } as ModelFile;
    const preview: Preview = { id: "preview", profile: "fine", status: "ready", converterVersion: "0.37.0", requestedAtUnixMs: 1, finishedAtUnixMs: 2,
      artifacts: [{ kind: "preview-glb", url: "/derived", sha256: "a".repeat(64), byteSize: 1, mediaType: "model/gltf-binary" }] };
    const markup = previewInspectionMarkup(file, [preview], true);
    expect(markup).toContain("CAD/&lt;main&gt;.step");
    expect(markup).toContain("Profil fine");
    expect(markup).toContain("3D-Vorschau · abgeleitet");
    expect(markup).toContain("Fein-Profil anfordern");
  });
});
