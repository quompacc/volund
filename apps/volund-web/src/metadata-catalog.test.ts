import { describe, expect, it } from "vitest";
import { collectionMarkup, tagMarkup } from "./metadata-catalog";
import type { CollectionDetail, ModelSummary, TagSummary } from "./types";

const model: ModelSummary = {
  id: "model-1", slug: "frame", name: "Frame <script>", description: "", kind: "part",
  licenseKind: "not-specified", licenseValue: null, authorName: null, primaryFileId: null,
  fileCount: 1, formats: ["step"], updatedAtUnixMs: 0, tags: [], tagIds: [], collections: [],
  viewerRotation: [0, 0, 0], revision: 1,
  thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
};

describe("metadata administration markup", () => {
  it("escapes collections and exposes membership controls only to editors", () => {
    const collection: CollectionDetail = {
      id: "collection-1", slug: "print", name: "Print <img>", description: "Safe & stored",
      modelCount: 1, revision: 2, active: true, updatedAtUnixMs: 0, modelIds: [model.id],
    };
    const viewer = collectionMarkup([collection], [model], false, false);
    expect(viewer).toContain("Print &lt;img&gt;");
    expect(viewer).toContain("Frame &lt;script&gt;");
    expect(viewer).not.toContain("data-remove-model");
    expect(viewer).not.toContain("data-edit-collection");
    const editor = collectionMarkup([collection], [model], true, false);
    expect(editor).toContain("data-remove-model");
    expect(editor).not.toContain("data-remove-collection");
  });

  it("keeps tag mutations administrator-only and renders retained aliases", () => {
    const tags: TagSummary[] = [
      { id: "tag-1", name: "Core <XY>", active: true, mergedIntoId: null, modelCount: 2, revision: 1, updatedAtUnixMs: 0 },
      { id: "tag-2", name: "Legacy", active: false, mergedIntoId: "tag-1", modelCount: 0, revision: 2, updatedAtUnixMs: 0 },
      { id: "tag-3", name: "Printer", active: true, mergedIntoId: null, modelCount: 1, revision: 1, updatedAtUnixMs: 0 },
    ];
    expect(tagMarkup(tags, false)).not.toContain("data-edit-tag");
    const admin = tagMarkup(tags, true);
    expect(admin).toContain("Core &lt;XY&gt;");
    expect(admin).toContain("INAKTIVER ALIAS");
    expect(admin).toContain("data-merge-tag");
    expect(admin).toContain('data-lifecycle-action="tag.remove"');
  });
});
