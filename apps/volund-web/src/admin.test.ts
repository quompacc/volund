import { describe, expect, it } from "vitest";
import { activationUrl, availableRoles, formatSessionTime, formatSettingValue, settingControl, settingInputValue, userStatusAction } from "./admin";
import type { Setting } from "./types";

function setting(overrides: Partial<Setting>): Setting {
  return {
    key: "instance.name",
    domain: "instance",
    valueType: "string",
    value: "VÖLUND",
    origin: "default",
    editable: true,
    sensitive: false,
    revision: 0,
    effect: "immediate",
    constraints: {},
    ...overrides,
  };
}

describe("administration setting controls", () => {
  it("renders persisted settings as editable revision-controlled forms", () => {
    const markup = settingControl(setting({ origin: "persisted", revision: 4 }));
    expect(markup).toContain("data-setting='");
    expect(markup).toContain("Name der Instanz");
    expect(markup).toContain("Revision 4");
    expect(markup).toContain("Speichern");
  });

  it("renders operator values as non-editable and never renders secret material", () => {
    const diagnostic = settingControl(setting({
      key: "database.connectionOverride",
      valueType: "secret",
      value: null,
      origin: "environment",
      editable: false,
      sensitive: true,
      configured: true,
      effect: "restart",
      constraints: { operatorManaged: true },
    }));
    expect(diagnostic).not.toContain("<form");
    expect(diagnostic).toContain("Konfiguriert");
    expect(diagnostic).toContain("Neustart erforderlich");
    expect(diagnostic).toContain("durch Betreiber verwaltet");
    expect(diagnostic).not.toContain("postgresql://");
  });

  it("renders and converts integer and boolean values without string coercion", () => {
    const integer = setting({
      key: "security.sessionIdleMinutes", valueType: "integer", value: 480,
      constraints: { minimum: 5, maximum: 1440 },
    });
    expect(settingControl(integer)).toContain('type="number"');
    expect(settingControl(integer)).toContain('min="5"');
    expect(settingInputValue(integer, "30")).toBe(30);
    const boolean = setting({ key: "example.boolean", valueType: "boolean", value: false });
    expect(settingControl(boolean)).toContain('<option value="false" selected>');
    expect(settingInputValue(boolean, "true")).toBe(true);
  });

  it("distinguishes compiled limits from operator-managed settings", () => {
    const limit = setting({
      key: "limits.importMaxFiles", valueType: "integer", value: 10_000,
      editable: false, constraints: { compiled: true },
    });
    expect(settingControl(limit)).toContain("durch Anwendung festgelegt");
    expect(settingControl(limit)).not.toContain("durch Betreiber verwaltet");
    expect(formatSettingValue(setting({ valueType: "integer", value: 1_048_576, constraints: { unit: "bytes" } }))).toContain("MB");
  });

  it("formats session activity in the configured locale and time zone", () => {
    const timestamp = Date.UTC(2026, 7, 29, 12, 0, 0);
    expect(formatSessionTime(timestamp, "de-DE", "Europe/Berlin")).toBe(
      new Intl.DateTimeFormat("de-DE", {
        dateStyle: "medium", timeStyle: "short", timeZone: "Europe/Berlin",
      }).format(timestamp),
    );
  });

  it("builds encoded one-time activation links and explicit account actions", () => {
    expect(activationUrl("token/value", "https://vault.example:8443")).toBe("https://vault.example:8443/?invite=token%2Fvalue");
    expect(userStatusAction("active")).toEqual({ next: "disabled", label: "Deaktivieren" });
    expect(userStatusAction("locked")).toEqual({ next: "active", label: "Entsperren" });
    expect(userStatusAction("invited")).toEqual({ next: "disabled", label: "Einladung sperren" });
    expect(availableRoles("administrator")).toEqual(["viewer", "editor", "administrator"]);
    expect(availableRoles("administrator")).not.toContain("owner");
    expect(availableRoles("owner")).toContain("owner");
  });
});
