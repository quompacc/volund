// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { mountNavigationScroll } from "./navigation-scroll";

describe("seitliche Navigation", () => {
  it("bietet benannte Maustasten für beide Richtungen ohne Fokusverlust", () => {
    document.body.innerHTML = '<aside class="app-nav"></aside><main></main>';
    const nav = document.querySelector<HTMLElement>("aside")!;
    Object.defineProperty(nav, "clientWidth", { value: 320 });
    nav.scrollBy = vi.fn();
    mountNavigationScroll(nav);
    const controls = document.querySelector<HTMLElement>(".nav-scroll-hint")!;
    expect(controls.textContent).toContain("Navigation seitlich scrollen");
    const buttons = controls.querySelectorAll("button");
    expect(buttons[0]!.getAttribute("aria-label")).toBe("Navigation nach links scrollen");
    expect(buttons[1]!.getAttribute("aria-label")).toBe("Navigation nach rechts scrollen");
    buttons[1]!.focus(); buttons[1]!.click();
    expect(nav.scrollBy).toHaveBeenLastCalledWith({ left: 256, behavior: "smooth" });
    expect(document.activeElement).toBe(buttons[1]);
    buttons[0]!.click();
    expect(nav.scrollBy).toHaveBeenLastCalledWith({ left: -256, behavior: "smooth" });
  });
});
