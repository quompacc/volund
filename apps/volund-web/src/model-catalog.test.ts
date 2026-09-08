import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { fileDetailMarkup, modelCardMarkup, modelFileFilter, modelFilesMarkup, modelProblemState, thumbnailPanelMarkup } from "./model-presentation";
import type { ModelFile, ModelSummary, ThumbnailCandidate } from "./types";

describe("persistent model catalog presentation", () => {
  it("renders collections, tags, file counts, and escapes stored metadata", () => {
    const model: ModelSummary = {
      id: "model-1", slug: "voron-trident", name: "Voron <Trident>", description: "Core & XY",
      kind: "project", licenseKind: "not-specified", licenseValue: null,
      authorName: "Voron Design", primaryFileId: "file-1", fileCount: 179, formats: ["step", "stl"],
      updatedAtUnixMs: Date.UTC(2026, 7, 29, 5, 0), tags: ["Voron", "3D-Drucker"], tagIds: ["tag-1", "tag-2"],
      collections: ["3D-Drucker"],
      viewerRotation: [0, 0, 0], revision: 1,
      thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
    };
    const markup = modelCardMarkup(model);
    expect(markup).toContain("3D-Drucker");
    expect(markup).toContain("179 Dateien");
    expect(markup).toContain("Voron &lt;Trident&gt;");
    expect(markup).toContain("Core &amp; XY");
    expect(markup).toContain('data-model-tag-filter="tag-1"');
    expect(markup).toContain('aria-label="Nach Tag Voron filtern"');
  });

  it("uses the shared thumbnail state and keeps broken selections visible", () => {
    const base: ModelSummary = {
      id: "model-1", slug: "frame", name: "Frame <X>", description: "", kind: "part",
      licenseKind: "not-specified", licenseValue: null, authorName: null, primaryFileId: null,
      fileCount: 1, formats: [], updatedAtUnixMs: 0, tags: [], tagIds: [], collections: [],
      viewerRotation: [0, 0, 0], revision: 1,
      thumbnail: { kind: "source-file", candidateId: "file-1", url: "/api/v1/files/file-1/content", status: "ready" },
    };
    expect(modelCardMarkup(base)).toContain('class="model-thumbnail"');
    expect(modelCardMarkup(base)).toContain('class="model-thumbnail-backdrop"');
    expect(modelCardMarkup(base)).toContain('data-thumbnail-fit="contain"');
    expect(modelCardMarkup(base)).toContain('aria-hidden="true"');
    const broken = { ...base, thumbnail: { ...base.thumbnail, url: null, status: "fallback" as const } };
    expect(modelCardMarkup(broken)).toContain("Auswahl nicht verfügbar");
    const candidates: ThumbnailCandidate[] = [{ id: "file-1", kind: "source-file", label: "cover <bad>.png", url: "/safe", mediaType: "image/png", byteSize: 1 }];
    const editor = thumbnailPanelMarkup(base, candidates, true);
    expect(editor).toContain("cover &lt;bad&gt;.png");
    expect(editor).toContain('class="thumbnail-select"');
    expect(editor).toContain('value="source-file|file-1" selected');
    expect(editor).not.toContain("Neu erzeugen");
    expect(editor).toContain("Neutralen Platzhalter verwenden");
    expect(editor).not.toContain("Standardansicht");

    const generated = { ...base, thumbnail: { kind: "default" as const, candidateId: null, url: "/api/v1/artifacts/raster/content", status: "generated" as const } };
    expect(modelCardMarkup(generated)).toContain("Eigenes Bild auswählen");
    expect(modelCardMarkup(generated)).not.toContain('class="model-thumbnail"');
    expect(thumbnailPanelMarkup(generated, [], false)).toContain("neutraler Platzhalter");
  });

  it("keeps the foreground thumbnail uncropped after the complete CSS cascade", () => {
    const stylesCss = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    const responsiveCss = readFileSync(new URL("./responsive.css", import.meta.url), "utf8");
    const phase5Css = readFileSync(new URL("./phase5.css", import.meta.url), "utf8");
    const cssInBundleOrder = `${stylesCss}\n${responsiveCss}\n${phase5Css}`;
    const thumbnailRules = [...cssInBundleOrder.matchAll(/[.]model-thumbnail[ \t\r\n]*[{]([^}]*)[}]/g)];
    expect(thumbnailRules.length).toBeGreaterThan(0);
    expect(thumbnailRules.at(-1)?.[1]).toContain("object-fit: contain");
  });
});

describe("model fileset presentation", () => {
  it("groups linked files into visual CAD and image cards", () => {
    const file: ModelFile = {
      id: "file-1", path: "Voron/Trident/main.step", rootKey: "cad", rootName: "CAD & Archiv",
      role: "master-cad", primary: true, format: "step", byteSize: 1_572_864,
      modifiedAtUnixMs: 0, missing: false, revision: 1, caption: "", description: "", notes: "",
      printable: false, printed: false, preSupported: false, upAxis: null, supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
    };
    const markup = modelFilesMarkup([file, { ...file, id: "file-2", path: "Voron/<preview>.png", role: "image", primary: false, format: null, missing: true }]);
    expect(markup).toContain("3D &amp; CAD");
    expect(markup).toContain("Bilder");
    expect(markup).toContain("main.step");
    expect(markup).toContain("PRIMÄR");
    expect(markup).toContain("STEP");
    expect(markup).toContain("1,5 MB");
    expect(markup).toContain("&lt;preview&gt;.png");
    expect(markup).toContain("NICHT VERFÜGBAR");
    expect(markup).toContain('data-file-filter="step"');
    expect(markup).toContain('data-file-filter="images"');
    expect(markup).toContain('class="file-menu-trigger"');
  });

  it("embeds available STL files as directly interactive 3D canvases", () => {
    const stl: ModelFile = {
      id: "stl-file", path: "Parts/gantry.stl", rootKey: "cad", rootName: "CAD",
      role: "printable-mesh", primary: false, format: "stl", byteSize: 42,
      modifiedAtUnixMs: 0, missing: false, revision: 1, caption: "", description: "", notes: "",
      printable: true, printed: false, preSupported: false, upAxis: "z", supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
    };
    const markup = modelFilesMarkup([stl]);
    expect(markup).toContain('data-stl-preview="stl-file"');
    expect(markup).toContain("Drehbare 3D-Vorschau von gantry.stl");
    expect(markup).toContain("STL WIRD GELADEN");
    expect(modelFileFilter(stl)).toBe("stl");
  });

  it("preserves the original filename for browser downloads", () => {
    const file: ModelFile = {
      id: "file-1", path: "Teile/Lüfter & Halter.stl", rootKey: "cad", rootName: "CAD",
      role: "printable-mesh", primary: false, format: "stl", byteSize: 42,
      modifiedAtUnixMs: 0, missing: false, revision: 1, caption: "", description: "", notes: "",
      printable: true, printed: false, preSupported: false, upAxis: null, supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
    };
    expect(fileDetailMarkup(file, false, [])).toContain('download="Lüfter &amp; Halter.stl"');
  });

  it("offers a direct primary-source action for a non-primary STEP file", () => {
    const file: ModelFile = {
      id: "step-file", path: "CAD/Switchwire_Assembly.step", rootKey: "cad", rootName: "CAD",
      role: "cad", primary: false, format: "step", byteSize: 42, modifiedAtUnixMs: 0,
      missing: false, revision: 1, caption: "", description: "", notes: "", printable: false,
      printed: false, preSupported: false, upAxis: null, supportHint: "", orientation: [0, 0, 0],
      lifecycleState: "available", lifecycleRevision: 1,
    };
    expect(fileDetailMarkup(file, true, [])).toContain('data-set-primary-file="step-file"');
    expect(fileDetailMarkup({ ...file, primary: true }, true, [])).not.toContain("data-set-primary-file");
  });
});

describe("model problem traffic light", () => {
  const problem = { key: "p", severity: "warning" as const, status: "open" as const, code: "W1", message: "Warnung", sourceId: "f", sourceName: "f.step", sourceUrl: "/f", diagnosticsUrl: null, previewId: "v", profile: "web", occurredAtUnixMs: 0, remediation: "Prüfen" };

  it("uses green, yellow, and red states for open findings", () => {
    expect(modelProblemState([])).toEqual({ tone: "ok", label: "Keine Probleme", count: 0 });
    expect(modelProblemState([problem])).toEqual({ tone: "warning", label: "Warnungen", count: 1 });
    expect(modelProblemState([problem, { ...problem, key: "e", severity: "error" }])).toEqual({ tone: "error", label: "Fehler", count: 2 });
    expect(modelProblemState([{ ...problem, status: "resolved" }])).toEqual({ tone: "ok", label: "Keine Probleme", count: 0 });
  });
});
