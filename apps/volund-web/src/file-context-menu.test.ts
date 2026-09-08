// @vitest-environment happy-dom

import { describe, expect, it } from "vitest";
import { hideFileContextMenu } from "./file-context-menu";

describe("hideFileContextMenu", () => {
  it("hides the menu and restores focus to its trigger when requested", () => {
    const menu = document.createElement("div");
    const trigger = document.createElement("button");
    document.body.append(menu, trigger);
    menu.hidden = false;

    hideFileContextMenu(menu, trigger, true);

    expect(menu.hidden).toBe(true);
    expect(document.activeElement).toBe(trigger);
  });

  it("does not move focus for pointer-driven dismissal", () => {
    const menu = document.createElement("div");
    const trigger = document.createElement("button");
    const current = document.createElement("button");
    document.body.append(menu, trigger, current);
    current.focus();

    hideFileContextMenu(menu, trigger, false);

    expect(menu.hidden).toBe(true);
    expect(document.activeElement).toBe(current);
  });
});
