import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const styles = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
const phase5 = readFileSync(new URL("./phase5.css", import.meta.url), "utf8");

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

  it("keeps desktop navigation labels and values above the normal-text contrast floor", () => {
    const labelColor = phase5.match(/[.]nav-group > p, [. ]nav-group b, [. ]nav-foot p [{] color: (#[0-9a-f]{6})/i)?.[1];
    const navigationBackground = styles.match(/[.]app-nav [{][^}]*background: (#[0-9a-f]{6})/i)?.[1];
    expect(labelColor).toBeDefined();
    expect(navigationBackground).toBeDefined();
    expect(contrastRatio(labelColor!, navigationBackground!)).toBeGreaterThanOrEqual(4.5);
  });

  it("lässt lange Modellnamen und Kopfaktionen umbrechen", () => {
    expect(phase5).toContain(".page-crumbs { flex-wrap: wrap; }");
    expect(phase5).toContain(".page-crumbs > * { min-width: 0; max-width: 100%; overflow-wrap: anywhere; }");
    expect(phase5).toContain(".model-header { flex-wrap: wrap; }");
    expect(phase5).toContain(".topbar { height: auto; min-height: 62px; flex-wrap: wrap;");
  });
});
