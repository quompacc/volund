import { describe, expect, it } from "vitest";
import { importUploadItems, matchesResumeSelection } from "./import-resume";
import type { ImportDraft } from "./types";

const items = [
  { originalPath: "tiny.zip", byteSize: 120, category: "archive", uploadStatus: "uploaded" },
  { originalPath: "notes.txt", byteSize: 4, category: "document", uploadStatus: "pending" },
  { originalPath: "tiny.step", byteSize: 4, category: "cad", uploadStatus: "uploaded" },
] as ImportDraft["items"];
const selected: [{ path: string; byteSize: number }, { path: string; byteSize: number }] = [{ path: "tiny.zip", byteSize: 120 }, { path: "notes.txt", byteSize: 4 }];
describe("resuming expanded ZIP imports", () => {
  it("accepts original files without server-extracted files and uploads only required items", () => {
    expect(matchesResumeSelection(items, selected)).toBe(true);
    expect(importUploadItems(items).map((item) => item.originalPath)).toEqual(["tiny.zip", "notes.txt"]);
  });
  it("rejects missing ZIPs, missing pending files, changed sizes, foreign and duplicate paths", () => {
    for (const entries of [selected.slice(0, 1), selected.slice(1),
      [{ path: "tiny.zip", byteSize: 121 }, selected[1]],
      [...selected, { path: "foreign.step", byteSize: 4 }], [...selected, selected[0]]]) {
      expect(matchesResumeSelection(items, entries)).toBe(false);
    }
  });
  it("allows already uploaded ordinary files to be omitted or included without retransmission", () => {
    const ordinary = items.slice(1);
    expect(matchesResumeSelection(ordinary, selected.slice(1))).toBe(true);
    expect(matchesResumeSelection(ordinary, [selected[1], { path: "tiny.step", byteSize: 4 }])).toBe(true);
    expect(importUploadItems(ordinary)).toEqual([items[1]]);
  });
});
