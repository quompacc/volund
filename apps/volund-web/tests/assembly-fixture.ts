import type { AssemblyManifest } from "../src/assembly-tree";

const identity = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
export const manifest: AssemblyManifest = {
  contractVersion: 1, transformConvention: "row-major, parent-local", colorSpace: "sRGB",
  definitions: [
    { id: "root-def", name: "Baugruppe", kind: "assembly", color: null },
    { id: "group-def", name: "Unterbaugruppe", kind: "assembly", color: null },
    { id: "part-def", name: "Würfel", kind: "part", color: null },
  ],
  roots: [{
    id: "root", name: "Baugruppe", kind: "assembly", definition: "root-def",
    transform: identity, color: null, children: [
      { id: "group", name: "Unterbaugruppe", kind: "assembly-instance", definition: "group-def", transform: identity, color: null, children: [
        { id: "red", name: "Rot", kind: "part-instance", definition: "part-def", transform: identity, color: [1, 0, 0], children: [] },
        { id: "blue", name: "Blau", kind: "part-instance", definition: "part-def", transform: identity, color: [0, 0, 1], children: [] },
      ] },
    ],
  }],
};
