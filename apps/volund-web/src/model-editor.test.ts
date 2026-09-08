import { describe, expect, it } from "vitest";
import { modelEditorMarkup } from "./model-editor";
import type { CollectionSummary, ModelFile, ModelSummary } from "./types";

describe("model maintenance editor", () => {
  it("offers persistent metadata, orientation, and collections without fake CAD links", () => {
    const model: ModelSummary = {
      id: "model-1", slug: "legacy", name: "Wrong <name>", description: "Existing",
      kind: "project", licenseKind: "spdx", licenseValue: "CERN-OHL-S-2.0",
      authorName: "Voron Design", primaryFileId: "file-1", fileCount: 63, formats: ["step"],
      updatedAtUnixMs: 0, tags: ["Voron"], tagIds: ["tag-1"], collections: ["Printers"],
      viewerRotation: [-90, 0, 0], revision: 2,
      thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
    };
    const collections: CollectionSummary[] = [
      { id: "collection-1", slug: "printers", name: "Printers", description: "", modelCount: 1, revision: 1, active: true, updatedAtUnixMs: 0 },
    ];
    const files: ModelFile[] = [{ id: "file-1", path: "CAD/root.step", rootKey: "cad",
      rootName: "CAD", role: "master-cad", primary: true, format: "step", byteSize: 1,
      modifiedAtUnixMs: 0, missing: false, revision: 1, caption: "", description: "", notes: "",
      printable: false, printed: false, preSupported: false, upAxis: null, supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1 }];
    const markup = modelEditorMarkup(model, collections, files, [{ id: "tag-1", name: "Voron", active: true, mergedIntoId: null, modelCount: 1, revision: 1, updatedAtUnixMs: 0 }]);
    expect(markup).toContain("Projekt bearbeiten");
    expect(markup).toContain('value="Wrong &lt;name&gt;"');
    expect(markup).toContain('value="collection-1" checked');
    expect(markup).toContain('name="rotationX"');
    expect(markup).toContain("CERN-OHL-S-2.0");
    expect(markup).toContain('value="file-1" selected');
    expect(markup).toContain('value="-90" selected');
    expect(markup).not.toContain("Enthaltene Baugruppen & Teile");
    expect(markup).not.toContain("Verknüpfen");
  });
});
