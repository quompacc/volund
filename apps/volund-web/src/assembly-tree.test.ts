import { describe, expect, it } from "vitest";
import { assemblyTreeMarkup, parseAssemblyManifest } from "./assembly-tree";

const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

describe("primary STEP assembly structure", () => {
  it("renders nested XCAF assemblies, parts, instances, and the primary source", () => {
    const manifest = parseAssemblyManifest({ contractVersion: 1, transformConvention: "row-major, parent-local", colorSpace: "sRGB",
      definitions: [
        { id: "0:1", name: "Printer", kind: "assembly", color: null },
        { id: "0:20", name: "Gantry definition", kind: "assembly", color: [0.2, 0.3, 0.4] },
        { id: "0:30", name: "Extrusion definition", kind: "part", color: null, properties: { Material: "Aluminium" } },
      ], roots: [{
      id: "0:1", name: "Printer <Root>", kind: "assembly", definition: "0:1", transform: identity, color: null, children: [
        { id: "0:2", name: "Gantry", kind: "assembly-instance", definition: "0:20", transform: identity, color: null, children: [
          { id: "0:3", name: "Extrusion", kind: "part-instance", definition: "0:30", transform: identity, color: null, children: [] },
        ] },
      ],
    }] });
    const markup = assemblyTreeMarkup(manifest, "main.step");
    expect(markup).toContain("PRIMÄRE CAD- ODER MESH-DATEI");
    expect(markup).toContain("main.step");
    expect(markup).toContain("2 Baugruppen · 1 Teile · 2 Instanzen · 3 Knoten");
    expect(markup).toContain("Printer &lt;Root&gt;");
    expect(markup).toContain("Gantry");
    expect(markup).toContain("Extrusion");
    expect(markup).toContain("Isolieren");

    expect(markup).toContain('role="tree"');
    expect(markup).toContain('data-assembly-id="0:3"');
    expect(markup).toContain('tabindex="0"');
  });

  it("rejects malformed manifests", () => {
    expect(() => parseAssemblyManifest({ contractVersion: 2, roots: [] })).toThrow("ungültig");
    expect(() => parseAssemblyManifest({ contractVersion: 1, transformConvention: "row-major, parent-local", colorSpace: "sRGB", definitions: [], roots: [{ name: "missing fields" }] })).toThrow("ungültigen Knoten");
    expect(() => parseAssemblyManifest({ contractVersion: 1, transformConvention: "row-major, parent-local", colorSpace: "sRGB", definitions: [{ id: "d", name: "x", kind: "part", color: [2, 0, 0] }], roots: [] })).toThrow("Definition");
  });

  it("rejects legacy transform and color conventions while accepting the current contract", () => {
    const current = { contractVersion: 1, transformConvention: "row-major, parent-local", colorSpace: "sRGB", definitions: [], roots: [] };
    expect(parseAssemblyManifest(current)).toEqual(current);
    for (const transformConvention of [undefined, null, "column-major"]) {
      expect(() => parseAssemblyManifest({ ...current, transformConvention })).toThrow("nicht unterstützt");
    }
    for (const colorSpace of [undefined, null, "linear"]) {
      expect(() => parseAssemblyManifest({ ...current, colorSpace })).toThrow("nicht unterstützt");
    }
  });

  it("accepts a production-shaped 1,500-mesh manifest within the declared budget", () => {
    const definitions = Array.from({ length: 1500 }, (_, index) => ({ id: `d-${index}`, name: `Teil ${index}`, kind: "part", color: null }));
    const roots = definitions.map((definition, index) => ({ id: `n-${index}`, name: definition.name, kind: "part-instance", definition: definition.id, transform: identity, color: null, children: [] }));
    const manifest = parseAssemblyManifest({ contractVersion: 1, transformConvention: "row-major, parent-local", colorSpace: "sRGB", definitions, roots });
    expect(manifest.roots).toHaveLength(1500);
    expect(assemblyTreeMarkup(manifest, "large.step")).toContain("1500 Teile");
  });
});
