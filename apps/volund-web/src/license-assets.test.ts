import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

describe("Lizenzvertrag", () => {
  it("liefert den vollständigen unveränderten AGPL-Text aus", () => {
    const root = readFileSync(new URL("../../../LICENSE", import.meta.url));
    const asset = readFileSync(new URL("../public/LICENSE.txt", import.meta.url));
    expect(asset.equals(root)).toBe(true);
    expect(createHash("sha256").update(root).digest("hex")).toBe(
      "0d96a4ff68ad6d4b6f1f30f713b18d5184912ba8dd389f86aa7710db079abcb0",
    );
  });

  it("bindet Rust und beide npm-Metadaten an dieselbe Lizenzentscheidung", () => {
    const cargo = readFileSync(new URL("../../../Cargo.toml", import.meta.url), "utf8");
    const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
    const lock = JSON.parse(readFileSync(new URL("../package-lock.json", import.meta.url), "utf8"));
    expect(cargo).toContain('license = "AGPL-3.0-only"');
    expect(pkg.license).toBe("AGPL-3.0-only");
    expect(lock.packages[""].license).toBe(pkg.license);
    expect(pkg.private).toBe(true);
  });

  it("erhält die Browser-Lizenztexte und den Vite-Helferhinweis", () => {
    const notices = readFileSync(new URL("../public/THIRD_PARTY_NOTICES.txt", import.meta.url), "utf8");
    for (const relative of ["three/LICENSE", "pdfjs-dist/LICENSE", "pdfjs-dist/wasm/LICENSE_JBIG2"]) {
      const original = readFileSync(new URL(`../node_modules/${relative}`, import.meta.url), "utf8");
      expect(notices).toContain(original.replaceAll("\r\n", "\n").trimEnd());
    }
    expect(notices).toContain("Vite 8.2.2 / MIT-Kernlizenz");
    expect(notices).toContain("Copyright 2024 Mozilla Foundation");
  });
});
