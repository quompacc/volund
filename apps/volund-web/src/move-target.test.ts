import { describe, expect, it } from "vitest";
import { moveTargetPath, parentDirectory } from "./move-target";

describe("managed move targets", () => {
  it("derives the physical parent directory", () => {
    expect(parentDirectory("Voron/CAD/assembly.step")).toBe("Voron/CAD");
    expect(parentDirectory("assembly.step")).toBe("");
  });

  it("preserves the filename when changing directories", () => {
    expect(moveTargetPath("Baugruppen", "assembly.step")).toBe("Baugruppen/assembly.step");
    expect(moveTargetPath("", "old/assembly.step")).toBe("assembly.step");
  });
});
