import { describe, expect, it } from "vitest";
import { fileDetailMarkup } from "./model-presentation";
import { isStralarImportable, loadStralarAvailability, pickStralarFile, stralarActionMarkup, stralarOpenUrl } from "./stralar-link";
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

describe("Stralar on the model page", () => {
  const make = (id: string, path: string, over: Partial<ModelFile> = {}): ModelFile => ({ ...file, id, path, primary: false, printable: false, ...over });

  it("picks a printable mesh first, then any mesh, then the primary STEP, never a missing file", () => {
    const step = make("s", "a/teil.step", { primary: true });
    const otherStep = make("s2", "a/variante.stp");
    const stl = make("m", "a/teil.stl");
    const printable = make("p", "a/teil_print.3mf", { printable: true });
    const gone = make("g", "a/weg.stl", { printable: true, missing: true });
    const pdf = make("d", "a/anleitung.pdf");
    expect(pickStralarFile([pdf, step, stl, printable, gone], null)?.id).toBe("p");
    expect(pickStralarFile([pdf, step, stl, gone], null)?.id).toBe("m");
    expect(pickStralarFile([otherStep, step, pdf], null)?.id).toBe("s");
    expect(pickStralarFile([otherStep, make("s3", "x.step")], "s3")?.id).toBe("s3");
    expect(pickStralarFile([pdf, gone], null)).toBeNull();
  });

  it("renders a header link that opens the chosen file in a new tab", () => {
    expect(stralarActionMarkup(null)).toBe("");
    const markup = stralarActionMarkup(make("p", "a/teil.3mf"));
    expect(markup).toContain("In Stralar öffnen");
    expect(markup).toContain('target="_blank"');
    expect(markup).toContain(`href="/stralar/#/open?file=${encodeURIComponent("/api/v1/files/p/content")}&amp;name=teil.3mf"`);
  });
});
