import { describe, expect, it } from "vitest";
import { fileDetailMarkup, slicerHandoffStatusMarkup } from "./model-presentation";
import type { ModelFile, SlicerTarget } from "./types";

const printable: ModelFile = {
  id: "file-1", path: "parts/body.stl", rootKey: "models", rootName: "Models",
  role: "printable-mesh", primary: false, format: "stl", byteSize: 42,
  modifiedAtUnixMs: 0, missing: false, revision: 1, caption: "", description: "",
  notes: "", printable: true, printed: false, preSupported: false, upAxis: null,
  supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
};

describe("slicer handoff presentation", () => {
  it("offers only escaped configured targets", () => {
    const targets: SlicerTarget[] = [{
      id: 'orca" onclick="alert(1)', name: "Orca <Slicer>", scheme: "orcaslicer",
    }];
    const markup = fileDetailMarkup(printable, false, targets);
    expect(markup).toContain('data-slicer-target="orca&quot; onclick=&quot;alert(1)"');
    expect(markup).toContain("Orca &lt;Slicer&gt;");
    expect(markup).not.toContain("Orca <Slicer>");
  });

  it("explains the original-download fallback when no target is available", () => {
    const markup = fileDetailMarkup(printable, false, []);
    expect(markup).toContain("Kein Slicer-Ziel ist konfiguriert");
    expect(markup).toContain("Original herunterladen");
    expect(markup).not.toContain("data-slicer-target");
  });

  it("explains and safely links the fallback when no local program reacts", () => {
    const markup = slicerHandoffStatusMarkup('https://models.test/file?x="bad"&y=1');
    expect(markup).toContain("Falls kein Zielprogramm reagiert: Datei herunterladen");
    expect(markup).toContain('href="https://models.test/file?x=&quot;bad&quot;&amp;y=1"');
  });

  it("does not offer a handoff for missing or non-printable files", () => {
    expect(fileDetailMarkup({ ...printable, missing: true }, false, [])).not.toContain("Im Slicer öffnen");
    expect(fileDetailMarkup({ ...printable, printable: false }, false, [])).not.toContain("Im Slicer öffnen");
  });
});
