import { describe, expect, it } from "vitest";
import { fileDetailMarkup } from "./model-presentation";
import { isStralarImportable, loadStralarAvailability, stralarOpenUrl } from "./stralar-link";
import type { ModelFile } from "./types";

const file: ModelFile = {
  id: "0f8fad5b-d9cb-469f-a165-70867728950e", path: "Lampe/lampen_halter_turm.step", rootKey: "main", rootName: "Main",
  role: "master-cad", primary: true, format: "step", byteSize: 1, modifiedAtUnixMs: 0, missing: false, revision: 1,
  caption: "", description: "", notes: "", printable: false, printed: false, preSupported: false, upAxis: null,
  supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
};

describe("Stralar link", () => {
  it("accepts mesh and CAD formats Stralar imports", () => {
    for (const path of ["a.stl", "b.3MF", "c.obj", "d.step", "e.STP", "f.iges", "g.igs"]) expect(isStralarImportable(path)).toBe(true);
    for (const path of ["a.pdf", "b.png", "c.zip", "stl"]) expect(isStralarImportable(path)).toBe(false);
  });

  it("links the same-origin download and the file name", () => {
    const url = stralarOpenUrl(file.id, file.path);
    expect(url.startsWith("/stralar/#/open?")).toBe(true);
    const params = new URLSearchParams(url.split("?")[1]);
    expect(params.get("file")).toBe(`/api/v1/files/${file.id}/content`);
    expect(params.get("name")).toBe("lampen_halter_turm.step");
  });

  it("treats failures and a negative answer as not available", async () => {
    expect(await loadStralarAvailability(async () => ({ available: true }))).toBe(true);
    expect(await loadStralarAvailability(async () => ({ available: false }))).toBe(false);
    expect(await loadStralarAvailability(async () => { throw new Error("503"); })).toBe(false);
  });

  it("offers 'In Stralar öffnen' only when available, for importable, present files", () => {
    expect(fileDetailMarkup(file, false, [], true)).toContain("In Stralar öffnen");
    expect(fileDetailMarkup(file, true, [], true)).toContain("In Stralar öffnen");
    expect(fileDetailMarkup(file, false, [])).not.toContain("In Stralar öffnen");
    expect(fileDetailMarkup({ ...file, missing: true }, false, [], true)).not.toContain("In Stralar öffnen");
    expect(fileDetailMarkup({ ...file, path: "doc/readme.pdf" }, false, [], true)).not.toContain("In Stralar öffnen");
    expect(fileDetailMarkup(file, false, [], true)).toContain('target="_blank"');
  });
});
