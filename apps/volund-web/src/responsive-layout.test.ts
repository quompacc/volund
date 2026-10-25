import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const styles = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
const phase5 = readFileSync(new URL("./phase5.css", import.meta.url), "utf8");
const theme = readFileSync(new URL("./theme.css", import.meta.url), "utf8");

function themeTokens(selector: string): Record<string, string> {
  const start = theme.indexOf(selector);
  const block = theme.slice(start, theme.indexOf("}", start));
  return Object.fromEntries([...block.matchAll(/--([a-z0-9-]+):\s*(#[0-9a-f]{6})/gi)].map((match) => [match[1]!, match[2]!]));
}

function relativeLuminance(hex: string): number {
  const channels = hex.match(/[0-9a-f]{2}/gi)!.map((channel) => Number.parseInt(channel, 16) / 255)
    .map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return 0.2126 * channels[0]! + 0.7152 * channels[1]! + 0.0722 * channels[2]!;
}

function contrastRatio(foreground: string, background: string): number {
  const values = [relativeLuminance(foreground), relativeLuminance(background)].sort((a, b) => b - a);
  return (values[0]! + 0.05) / (values[1]! + 0.05);
}

describe("responsive layout safeguards", () => {
  it("keeps administration forms within tablet and mobile viewports", () => {
    expect(styles).toContain(".admin-content { min-width: 0;");
    expect(styles).toContain(".admin-form { grid-template-columns: repeat(2, minmax(0, 1fr)); }");
    expect(styles).toContain(".admin-form, .library-form { grid-template-columns: 1fr; }");
    expect(styles).toContain(".admin-list article, .setting-grid form, .library-row { grid-template-columns: minmax(0, 1fr);");
  });

  it("allows long statistics and model history to shrink on mobile", () => {
    expect(styles).toContain(".stat-grid div { min-width: 0;");
    expect(styles).toContain(".stat-grid small { overflow-wrap: anywhere;");
    expect(phase5).toContain(".history-list li { grid-template-columns: minmax(0, 1fr); min-width: 0; }");
  });

  it("ergänzt die versteckte Scrollleiste durch sichtbare Navigationstasten", () => {
    const main = readFileSync(new URL("./main.ts", import.meta.url), "utf8");
    expect(main).toContain("mountNavigationScroll(");
    expect(phase5).toContain(".nav-scroll-hint { display: flex;");
  });

  it("keeps navigation labels and secondary text above the normal-text contrast floor in both themes", () => {
    expect(phase5).toContain(".nav-group > p, .nav-group b, .nav-foot p { color: var(--text-muted); }");
    expect(styles).toMatch(/[.]app-nav [{][^}]*background: var[(]--surface[)]/);
    const light = themeTokens(":root {");
    const dark = themeTokens(':root[data-theme="dark"] {');
    for (const tokens of [light, dark]) {
      for (const foreground of ["text", "text-muted", "text-subtle", "accent"]) {
        for (const background of ["surface", "bg", "surface-2"]) {
          expect(contrastRatio(tokens[foreground]!, tokens[background]!), `${foreground} on ${background}`).toBeGreaterThanOrEqual(4.5);
        }
      }
      expect(contrastRatio(tokens["on-accent"]!, tokens.accent!)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps the operating-system dark theme identical to the explicit dark theme", () => {
    const system = theme.slice(theme.indexOf(':root:not([data-theme="light"]) {'), theme.indexOf(':root[data-theme="dark"]'));
    const explicit = theme.slice(theme.indexOf(':root[data-theme="dark"] {'));
    const declarations = (block: string) => [...block.matchAll(/--[a-z0-9-]+:[^;]+;/gi)].map((match) => match[0]);
    expect(declarations(system)).toEqual(declarations(explicit));
  });

  it("lässt lange Modellnamen und Kopfaktionen umbrechen", () => {
    expect(phase5).toContain(".page-crumbs { flex-wrap: wrap; }");
    expect(phase5).toContain(".page-crumbs > * { min-width: 0; max-width: 100%; overflow-wrap: anywhere; }");
    expect(phase5).toContain(".model-header { flex-wrap: wrap; }");
    expect(phase5).toContain(".topbar { height: auto; min-height: 56px; flex-wrap: wrap;");
  });
});
