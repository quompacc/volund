import { describe, expect, it } from "vitest";
import { importErrorMessage } from "./import-errors";

describe("import error messages", () => {
  it.each(["incoming capacity would be exceeded", "zip_incoming_capacity_exceeded"])(
    "turns %s into actionable German guidance",
    (message) => {
      expect(importErrorMessage(message)).toBe(
        "Die Kapazität für eingehende Dateien ist ausgeschöpft. Brich einen offenen Import ab oder erhöhe das Limit.",
      );
    },
  );

  it("preserves unknown backend details", () => {
    expect(importErrorMessage("specific failure")).toBe("specific failure");
  });
});
