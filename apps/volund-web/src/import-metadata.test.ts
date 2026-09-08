import { describe, expect, it } from "vitest";
import { appendTags, buildMetadataRequest, parseTags, suggestedTags } from "./import-metadata";

describe("import metadata", () => {
  it("normalizes comma-separated tags without case-insensitive duplicates", () => {
    expect(parseTags(" Voron, CoreXY, voron, 3D-Druck ")).toEqual(["Voron", "CoreXY", "3D-Druck"]);
    expect(appendTags(["Voron"], " corexy, VORON ")).toEqual(["Voron", "corexy"]);
  });

  it("suggests existing tags and excludes selected or duplicate names", () => {
    expect(suggestedTags(["Voron", "3D-Drucker", "CoreXY", "voron"], ["Voron"], "r"))
      .toEqual(["3D-Drucker", "CoreXY"]);
    expect(suggestedTags(["Voron", "CoreXY"], [], "")).toEqual(["Voron", "CoreXY"]);
  });

  it("builds trimmed reviewed metadata and rejects missing names", () => {
    expect(buildMetadataRequest({
      modelName: " VORON 2.4 ", kind: "assembly", libraryRootId: "root-1", description: " Drucker ",
      authorName: " Voron Design ", tags: ["Voron", "CoreXY"], collectionIds: ["c1", "c1"],
    })).toEqual({
      modelName: "VORON 2.4", kind: "assembly", libraryRootId: "root-1", description: "Drucker",
      authorName: "Voron Design", tags: ["Voron", "CoreXY"], collectionIds: ["c1"],
      targetAction: "create", targetModelId: null, expectedModelRevision: null,
      licenseKind: "not-specified", licenseValue: null, primaryItemId: null, thumbnailItemId: null,
    });
    expect(() => buildMetadataRequest({
      modelName: " ", kind: "project", libraryRootId: "root-1", description: "", authorName: "", tags: [], collectionIds: [],
    })).toThrow("Modellnamen");
    expect(() => buildMetadataRequest({
      modelName: "Voron", kind: "project", libraryRootId: "", description: "", authorName: "", tags: [], collectionIds: [],
    })).toThrow("Zielbibliothek");
  });

  it("binds a safe extension to the selected model revision", () => {
    expect(buildMetadataRequest({
      modelName: "Bestehendes Modell", kind: "assembly", libraryRootId: "root-1", description: "",
      authorName: "", tags: [], collectionIds: [], targetAction: "extend",
      targetModel: { id: "model-1", revision: 7 }, primaryItemId: null,
    })).toMatchObject({
      targetAction: "extend", targetModelId: "model-1", expectedModelRevision: 7, primaryItemId: null,
    });
    expect(() => buildMetadataRequest({
      modelName: "Bestehendes Modell", kind: "assembly", libraryRootId: "root-1", description: "",
      authorName: "", tags: [], collectionIds: [], targetAction: "extend", targetModel: null,
    })).toThrow("Zielmodell");
  });
});
