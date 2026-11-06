import { describe, expect, it } from "vitest";
import { accountStatusLabel, componentLabel, describeWeekdays, roleLabel, runStatusLabel, scheduleOutcomeLabel, weekdayMask } from "./admin-labels";

describe("administration labels", () => {
  it("translates run, schedule, role, account and component states", () => {
    expect(runStatusLabel("completed")).toBe("Abgeschlossen");
    expect(runStatusLabel("timed-out")).toBe("Zeitlimit");
    expect(runStatusLabel("future-state")).toBe("future-state");
    expect(scheduleOutcomeLabel("enqueued")).toBe("eingereiht");
    expect(scheduleOutcomeLabel(null)).toBe("noch nicht ausgeführt");
    expect(roleLabel("owner")).toBe("Owner");
    expect(roleLabel("viewer")).toBe("Viewer");
    expect(accountStatusLabel("active")).toBe("aktiv");
    expect(accountStatusLabel("locked")).toBe("gesperrt");
    expect(componentLabel("scheduler")).toBe("Zeitplaner");
    expect(componentLabel("unknown-worker")).toBe("unknown-worker");
  });

  it("builds scheduler weekday masks with Monday as bit 0", () => {
    expect(weekdayMask([0, 1, 2, 3, 4])).toBe(31);
    expect(weekdayMask([0, 1, 2, 3, 4, 5, 6])).toBe(127);
    expect(weekdayMask([6, 6])).toBe(64);
    expect(weekdayMask([7, -1, 1.5, Number.NaN])).toBe(0);
  });

  it("describes weekday masks for people", () => {
    expect(describeWeekdays(127)).toBe("täglich");
    expect(describeWeekdays(31)).toBe("Mo–Fr");
    expect(describeWeekdays(96)).toBe("Sa–So");
    expect(describeWeekdays(0b0010101)).toBe("Mo, Mi, Fr");
    expect(describeWeekdays(0)).toBe("an keinem Tag");
  });
});
