import { describe, expect, it } from "vitest";
import { ancestorPaths, folderCountLabel, treeNodeKey } from "./folder-tree";

describe("raw folder tree", () => {
  it("builds stable root-scoped keys and every ancestor", () => {
    expect(treeNodeKey("cad", "Parts/Archive")).toBe("cad:Parts/Archive");
    expect(ancestorPaths("Parts/Archive")).toEqual(["", "Parts", "Parts/Archive"]);
    expect(ancestorPaths("")).toEqual([""]);
  });

  it("uses a grammatically correct file count", () => {
    expect(folderCountLabel(1)).toBe("1 Datei");
    expect(folderCountLabel(3)).toBe("3 Dateien");
  });
});
