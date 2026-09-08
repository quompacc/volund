// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import { policyApi } from "./api";
import { bindPolicyActions, policyPanels } from "./policy-view";

describe("P2.4 policy administration",()=>{
  it("renders empty, blocked, retained, and non-color outcome states accessibly",()=>{
    const markup=policyPanels([],[],{artifactRuns:2,artifactFiles:8,artifactBytes:1024,diagnostics:3,reasonCodes:["artifact_age_or_count_limit"]},[],"Europe/Berlin");
    expect(markup).toContain("Keine Profile vorhanden");
    expect(markup).toContain("Keine aktive Bibliothek");
    expect(markup).toContain("2 Artefaktläufe, 8 Dateien und 3 Diagnoseeinträge");
    expect(markup).toContain("Originale und Katalogdaten bleiben erhalten");
  });

  it("shows durable profile and schedule state with labeled controls",()=>{
    const markup=policyPanels([{id:"profile",name:"Fein",nativePreset:"fine",linearDeflection:null,angularDeflection:null,enabled:true,builtIn:false,revision:2}],[{id:"schedule",libraryId:"library",libraryName:"CAD",name:"Nacht",localTime:"02:30",timeZone:"Europe/Berlin",weekdayMask:127,fullScan:false,enabled:true,nextRunAtUnixMs:1,lastScheduledAtUnixMs:1,lastOutcome:"coalesced",lastScanId:"scan",lastScanStatus:"completed",revision:3}],{artifactRuns:0,artifactFiles:0,artifactBytes:0,diagnostics:0,reasonCodes:[]},[{id:"library",key:"cad",name:"CAD",readOnly:true,fileCount:0,missingFileCount:0,latestScanStatus:null,revision: 1, filesystemPath:"/redacted",enabled:true,latestScanStartedAtUnixMs:null,updatedAtUnixMs:1,storage:{state:"healthy",checkedAtUnixMs:1,reachable:true,directory:true,readable:true,writable:false,totalBytes:1,availableBytes:1,usedPercent:0,filesystemType:"ext4",reasons:[]}}],"Europe/Berlin");
    expect(markup).toContain("Fein"); expect(markup).toContain("coalesced"); expect(markup).toContain("IANA-Zeitzone");
    const host = document.createElement("main");
    host.innerHTML = markup;
    expect(host.querySelector<HTMLButtonElement>("[data-retire-profile]")!.dataset.policyRevision).toBe("2");
    expect(host.querySelector<HTMLButtonElement>("[data-delete-schedule]")!.dataset.policyRevision).toBe("3");
  });


  afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });

  function setup() {
    const host = document.createElement("main");
    host.innerHTML = policyPanels([], [], {artifactRuns:0,artifactFiles:0,artifactBytes:0,diagnostics:0,reasonCodes:[]}, [], "Europe/Berlin");
    host.querySelector<HTMLInputElement>("#create-profile [name=name]")!.value = "safe";
    document.body.append(host);
    const refresh = vi.fn().mockResolvedValue(undefined);
    bindPolicyActions(host, refresh);
    return {host, refresh};
  }

  function confirm(text: string) {
    const input = document.querySelector<HTMLInputElement>("dialog input")!;
    input.value = text;
    input.dispatchEvent(new Event("input"));
    document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
  }

  it("blocks wrong text and cancels without applying a profile", async () => {
    const {host, refresh} = setup();
    const create = vi.spyOn(policyApi, "createProfile");
    host.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable:true}));
    confirm("wrong");
    expect(document.querySelector("dialog")).not.toBeNull();
    expect(create).not.toHaveBeenCalled();
    document.querySelector("dialog")!.dispatchEvent(new Event("cancel", {cancelable:true}));
    await vi.waitFor(() => expect(host.textContent).toContain("Aktion abgebrochen"));
    expect(refresh).not.toHaveBeenCalled();
    expect(host.dataset.policyPending).toBeUndefined();
  });

  it("creates a profile with optional parameters and suppresses duplicate submission", async () => {
    const {host, refresh} = setup();
    const create = vi.spyOn(policyApi, "createProfile").mockResolvedValue({id:"profile",name:"safe",nativePreset:"web",linearDeflection:0.2,angularDeflection:null,enabled:true,builtIn:false,revision:1});
    host.querySelector<HTMLInputElement>("[name=linearDeflection]")!.value = "0.2";
    const form = host.querySelector("form")!;
    form.dispatchEvent(new Event("submit", {cancelable:true}));
    form.dispatchEvent(new Event("submit", {cancelable:true}));
    expect(document.querySelectorAll("dialog")).toHaveLength(1);
    confirm("APPLY PROFILE new");
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledTimes(1));
    expect(create).toHaveBeenCalledExactlyOnceWith({name:"safe",nativePreset:"web",linearDeflection:0.2,angularDeflection:null,enabled:true}, "APPLY PROFILE new");
  });

  it("shows API errors inline and permits another attempt", async () => {
    const {host} = setup();
    vi.spyOn(policyApi, "createProfile").mockRejectedValue(new Error("Revision veraltet"));
    host.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable:true}));
    confirm("APPLY PROFILE new");
    await vi.waitFor(() => expect(host.querySelector('[role=status]')?.textContent).toBe("Revision veraltet"));
    expect(host.dataset.policyPending).toBeUndefined();
    host.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable:true}));
    expect(document.querySelector("dialog")).not.toBeNull();
    document.querySelector("dialog")!.dispatchEvent(new Event("cancel", {cancelable:true}));
    await vi.waitFor(() => expect(host.dataset.policyPending).toBeUndefined());
  });

  it.each(["profile", "schedule"] as const)("preserves revision and parameters when toggling a %s", async (kind) => {
    const profile = {id:"profile",name:"safe",nativePreset:"web",linearDeflection:null,angularDeflection:null,enabled:true,builtIn:false,revision:7};
    const schedule = {id:"schedule",libraryId:"library",name:"night",localTime:"02:00",timeZone:"Europe/Berlin",weekdayMask:127,fullScan:false,enabled:true,revision:9};
    const value = kind === "profile" ? profile : schedule;
    const host = document.createElement("main");
    const button = document.createElement("button");
    button.setAttribute(`data-toggle-${kind}`, JSON.stringify(value));
    host.append(button); document.body.append(host);
    const action = vi.spyOn(policyApi, kind === "profile" ? "updateProfile" : "updateSchedule").mockResolvedValue(undefined as never);
    const refresh = vi.fn().mockResolvedValue(undefined);
    bindPolicyActions(host, refresh);
    button.click();
    confirm(`APPLY ${kind.toUpperCase()} ${value.id}`);
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    expect(action).toHaveBeenCalledWith(value, expect.objectContaining({name:value.name,enabled:false}), `APPLY ${kind.toUpperCase()} ${value.id}`);
  });

  it("submits all schedule fields after exact confirmation", async () => {
    const host = document.createElement("main");
    host.innerHTML = '<form id="create-schedule"><input name="libraryId" value="library"><input name="name" value="night"><input name="localTime" value="03:15"><input name="timeZone" value="Europe/Berlin"><input name="weekdayMask" value="31"><input name="fullScan" type="checkbox" checked></form>';
    document.body.append(host);
    const action = vi.spyOn(policyApi, "createSchedule").mockResolvedValue(undefined as never);
    const refresh = vi.fn().mockResolvedValue(undefined);
    bindPolicyActions(host, refresh);
    host.querySelector("form")!.dispatchEvent(new Event("submit", {cancelable:true}));
    confirm("APPLY SCHEDULE new");
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    expect(action).toHaveBeenCalledWith({libraryId:"library",name:"night",localTime:"03:15",timeZone:"Europe/Berlin",weekdayMask:31,fullScan:true,enabled:true}, "APPLY SCHEDULE new");
  });

  it.each(["retireProfile", "deleteSchedule"] as const)("does not retry %s after a revision conflict", async (method) => {
    const host = document.createElement("main");
    const profile = method === "retireProfile";
    host.innerHTML = `<p id="admin-message" role="status"></p><button ${profile ? "data-retire-profile" : "data-delete-schedule"}="object" data-policy-revision="7">Aktion</button>`;
    document.body.append(host);
    const action = vi.spyOn(policyApi, method).mockRejectedValue(new Error("Revision changed; reload before confirming"));
    const refresh = vi.fn();
    bindPolicyActions(host, refresh);
    host.querySelector("button")!.click();
    confirm(`${profile ? "RETIRE" : "DELETE SCHEDULE"} object`);
    await vi.waitFor(() => expect(host.textContent).toContain("Revision changed"));
    expect(action).toHaveBeenCalledTimes(1);
    expect(refresh).not.toHaveBeenCalled();
    expect(host.dataset.policyPending).toBeUndefined();
  });

  it.each([
    ["data-retire-profile", "profile", "RETIRE profile", "retireProfile"],
    ["data-delete-schedule", "schedule", "DELETE SCHEDULE schedule", "deleteSchedule"],
    ["id", "run-retention", "PURGE DERIVED ARTIFACTS", "runRetention"],
  ] as const)("confirms %s in the app before calling the API", async (attribute, value, expected, method) => {
    const host = document.createElement("main");
    host.innerHTML = `<p id="admin-message" role="status"></p><button ${attribute}="${value}" data-policy-revision="7">Aktion</button>`;
    document.body.append(host);
    const action = vi.spyOn(policyApi, method).mockResolvedValue(undefined as never);
    const refresh = vi.fn().mockResolvedValue(undefined);
    bindPolicyActions(host, refresh);
    host.querySelector("button")!.click();
    expect(action).not.toHaveBeenCalled();
    host.querySelector("button")!.dataset.policyRevision = "8";
    confirm(expected);
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    expect(action).toHaveBeenCalledWith(...(method === "runRetention" ? [expected] : [value, 7, expected]));
  });
});
