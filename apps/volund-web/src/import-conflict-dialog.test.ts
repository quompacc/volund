// @vitest-environment happy-dom

import { describe, expect, it } from "vitest";
import { createImportConflictDialog } from "./import-conflict-dialog";

describe("import conflict dialog", () => {
  it("returns a reviewed target path without relying on a browser prompt", async () => {
    const dialog = createImportConflictDialog();
    document.body.append(dialog.element);
    const selected = dialog.requestPath("Projekte/frame/CAD/main.step");
    const input = dialog.element.querySelector<HTMLInputElement>("input")!;
    expect(input.value).toBe("Projekte/frame/CAD/main.step");
    input.value = "Projekte/frame/CAD/main-v2.step";
    dialog.element.querySelector<HTMLButtonElement>("button[type='submit']")!.click();
    await expect(selected).resolves.toBe("Projekte/frame/CAD/main-v2.step");
    expect(dialog.element.hasAttribute("open")).toBe(false);
  });

  it("returns null when the user cancels", async () => {
    const dialog = createImportConflictDialog();
    document.body.append(dialog.element);
    const selected = dialog.requestPath("Projekte/frame/CAD/main.step");
    dialog.element.querySelector<HTMLButtonElement>("button[type='button']")!.click();
    await expect(selected).resolves.toBeNull();
  });
});
