import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, catalogApi, identityApi, jobsApi, libraryApi, mutationJson, operationsApi, policyApi, postFile, postJson, requestJson } from "./api";

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("catalog API client", () => {
  it.each(["retireProfile", "deleteSchedule"] as const)("binds %s to the confirmed revision", async (method) => {
    const fetchMock = vi.fn().mockResolvedValue(new Response("{}"));
    vi.stubGlobal("fetch", fetchMock);
    await policyApi[method]("object", 7, "exact confirmation");
    expect(fetchMock).toHaveBeenCalledWith(expect.any(String), expect.objectContaining({
      method: "DELETE", body: JSON.stringify({confirmation:"exact confirmation", expectedRevision:7}),
    }));
  });
  it("reads JSON and sends an explicit accept header", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{"status":"ok"}'));
    vi.stubGlobal("fetch", fetchMock);
    await expect(requestJson<{ status: string }>("/api/v1/health")).resolves.toEqual({ status: "ok" });
    expect(fetchMock).toHaveBeenCalledWith("/api/v1/health", { headers: { Accept: "application/json" } });
  });

  it("preserves API error messages and handles proxy errors", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(
      '{"error":{"message":"kaputt"}}', { status: 404 },
    )));
    await expect(requestJson("/missing")).rejects.toEqual(new ApiError(404, "kaputt"));
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("bad gateway", { status: 502 })));
    await expect(requestJson("/missing")).rejects.toEqual(new ApiError(502, "HTTP 502"));
  });

  it("ends a stalled JSON request with an actionable timeout error", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(new Promise<Response>(() => undefined)));
    let failure: unknown;
    void requestJson("/stalled").catch((error) => { failure = error; });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(failure).toEqual(new ApiError(0, "Die Anfrage hat zu lange gedauert. Prüfe die Verbindung und versuche es erneut."));
  });

  it("describes a stalled mutation as uncertain instead of inviting a blind retry", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn().mockReturnValue(new Promise<Response>(() => undefined)));
    let failure: unknown;
    void mutationJson("/stalled", "POST", {}).catch((error) => { failure = error; });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(failure).toEqual(new ApiError(0,
      "Die Antwort hat zu lange gedauert. Der Ausgang der Aktion ist unbekannt. Lade die Ansicht neu, bevor du sie wiederholst."));
  });

  it("encodes catalog identifiers and pagination", async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(
      new Response('{"items":[],"limit":100,"offset":20,"total":0}'),
    ));
    vi.stubGlobal("fetch", fetchMock);
    const options = {
      directory: "printer parts", query: "frame", format: "step", sort: "size" as const,
      direction: "desc" as const, offset: 20, limit: 100,
    };
    await catalogApi.files("CAD & Werk", options);
    expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/v1/roots/CAD%20%26%20Werk/files?limit=100&offset=20&sort=size&direction=desc&directory=printer+parts&q=frame&format=step");
    await catalogApi.folders("CAD & Werk", options);
    expect(fetchMock.mock.calls[1]?.[0]).toContain("/folders?");
    await catalogApi.previews("file/id");
    expect(fetchMock.mock.calls[2]?.[0]).toBe("/api/v1/files/file%2Fid/previews?limit=50");
    await catalogApi.models();
    expect(fetchMock.mock.calls[3]?.[0]).toBe("/api/v1/models");
    await catalogApi.collections();
    expect(fetchMock.mock.calls[4]?.[0]).toBe("/api/v1/collections?limit=100&offset=0");
    await catalogApi.enqueuePreview("file/id");
    expect(fetchMock.mock.calls[5]?.[0]).toBe("/api/v1/files/file%2Fid/previews");
    expect(fetchMock.mock.calls[5]?.[1]?.method).toBe("POST");
    expect(JSON.parse(fetchMock.mock.calls[5]?.[1]?.body as string)).toEqual({ profile: "web" });
    await catalogApi.enqueuePreview("file/id", "fine");
    expect(JSON.parse(fetchMock.mock.calls[6]?.[1]?.body as string)).toEqual({ profile: "fine" });
    expect(catalogApi.sourceContentUrl("file/id")).toBe("/api/v1/files/file%2Fid/content");
    await catalogApi.files("unicode", { ...options, directory: "Sonderzeichen", query: "Übergröße + 100%" });
    expect(fetchMock.mock.calls[7]?.[0]).toContain("directory=Sonderzeichen&q=%C3%9Cbergr%C3%B6%C3%9Fe+%2B+100%25");
  });

  it("passes administration list offsets to jobs and quarantines", async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(
      new Response('{"items":[],"limit":50,"offset":0,"total":0}'),
    ));
    vi.stubGlobal("fetch", fetchMock);
    await jobsApi.list({ kind: "scan", offset: 50 });
    await catalogApi.quarantines(100);
    expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/v1/jobs?limit=50&offset=50&kind=scan");
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/api/v1/quarantines?limit=100&offset=100");
  });

  it("posts managed moves as JSON and preserves API errors", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(
      '{"id":"file-1","previousPath":"old.step","path":"CAD/old.step"}',
    ));
    vi.stubGlobal("fetch", fetchMock);
    await expect(postJson("/move", { destinationDirectory: "CAD" })).resolves.toMatchObject({
      id: "file-1", path: "CAD/old.step",
    });
    expect(fetchMock).toHaveBeenCalledWith("/move", {
      method: "POST",
      headers: { Accept: "application/json", "Content-Type": "application/json" },
      body: '{"destinationDirectory":"CAD"}',
    });

    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(
      '{"error":{"message":"Zieldatei existiert bereits"}}', { status: 409 },
    )));
    await expect(postJson("/move", {})).rejects.toEqual(new ApiError(409, "Zieldatei existiert bereits"));
  });

  it("posts model creation and metadata-only import previews", async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(new Response('{"id":"draft-1","items":[]}')));
    vi.stubGlobal("fetch", fetchMock);
    await catalogApi.createModel({ name: "Voron", slug: "voron", kind: "assembly", primaryFileId: "file-1" });
    expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/v1/models");
    await catalogApi.previewImport("Voron.zip", [{ path: "main.step", byteSize: 42 }]);
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/api/v1/imports/preview");
    expect(JSON.parse(fetchMock.mock.calls[1]?.[1]?.body as string)).toEqual({
      sourceName: "Voron.zip", entries: [{ path: "main.step", byteSize: 42 }],
    });
    await catalogApi.configureImport("draft/1", {
      modelName: "Voron", kind: "assembly", libraryRootId: "root-1", description: "", authorName: null, tags: ["CoreXY"],
      collectionIds: ["collection-1"],
    });
    expect(fetchMock.mock.calls[2]?.[0]).toBe("/api/v1/imports/draft%2F1/metadata");
    await catalogApi.latestUploadedImport();
    expect(fetchMock.mock.calls[3]?.[0]).toBe("/api/v1/imports/latest-uploaded");
    await catalogApi.reviewImport("draft/1");
    expect(fetchMock.mock.calls[4]?.[0]).toBe("/api/v1/imports/draft%2F1/review");
    await catalogApi.createCollection({ name: "3D-Drucker", description: "" });
    expect(fetchMock.mock.calls[5]?.[0]).toBe("/api/v1/collections");
  });

  it("updates models and manages encoded component relationships", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response('{"id":"model/1"}'))
      .mockResolvedValueOnce(new Response('[]'))
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    await catalogApi.updateModel("model/1", {
      expectedRevision: 3, name: "Voron Legacy", description: "", kind: "project",
      licenseKind: "not-specified", licenseValue: null, authorName: null,
      tags: [], tagIds: ["tag-1"], collectionIds: ["collection-1"], primaryFileId: "file-1",
      viewerRotation: [-90, 0, 0],
    });
    await catalogApi.modelComponents("model/1");
    const model = { id: "model/1", revision: 3 } as never;
    await catalogApi.addModelComponent(model, "child/2");
    await catalogApi.removeModelComponent(model, "child/2");
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      "/api/v1/models/model%2F1",
      "/api/v1/models/model%2F1/components",
      "/api/v1/models/model%2F1/components",
      "/api/v1/models/model%2F1/components/child%2F2",
    ]);
    expect(fetchMock.mock.calls.map((call) => call[1]?.method)).toEqual(["PATCH", undefined, "POST", "DELETE"]);
  });

  it("uses revisioned model-owned thumbnail endpoints", async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce(new Response("[]"))
      .mockResolvedValueOnce(new Response('{"id":"model/1"}'))
      .mockResolvedValueOnce(new Response('{"id":"preview-1","status":"queued"}'));
    vi.stubGlobal("fetch", fetchMock);
    await catalogApi.thumbnailCandidates("model/1");
    await catalogApi.updateThumbnail({ id: "model/1", revision: 4 } as never, "source-file", "file/2");
    await catalogApi.regenerateThumbnail("model/1", 4, "fine");
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      "/api/v1/models/model%2F1/thumbnail-candidates",
      "/api/v1/models/model%2F1/thumbnail",
      "/api/v1/models/model%2F1/thumbnail/regenerate",
    ]);
    expect(JSON.parse(fetchMock.mock.calls[1]?.[1]?.body as string)).toEqual({
      expectedRevision: 4, kind: "source-file", candidateId: "file/2",
    });
    expect(JSON.parse(fetchMock.mock.calls[2]?.[1]?.body as string)).toEqual({ expectedRevision: 4, profile: "fine" });
  });

  it("uses revisioned model-owned problem endpoints", async () => {
    const fetchMock = vi.fn().mockImplementation(async () => new Response("[]"));
    vi.stubGlobal("fetch", fetchMock);
    await catalogApi.modelProblems("model/1");
    await catalogApi.updateModelProblems({ id: "model/1", revision: 7 } as never, ["preview:0"], "ignored");
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      "/api/v1/models/model%2F1/problems", "/api/v1/models/model%2F1/problems",
    ]);
    expect(fetchMock.mock.calls[1]?.[1]?.method).toBe("PUT");
    expect(JSON.parse(fetchMock.mock.calls[1]?.[1]?.body as string)).toEqual({ expectedRevision: 7, keys: ["preview:0"], status: "ignored" });
  });

  it("posts file bodies without JSON buffering", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{"status":"uploaded"}'));
    vi.stubGlobal("fetch", fetchMock);
    const body = new Blob(["cad-bytes"]);
    await postFile("/upload", body);
    expect(fetchMock).toHaveBeenCalledWith("/upload", {
      method: "POST",
      headers: { Accept: "application/json", "Content-Type": "application/octet-stream" },
      body,
    });
  });

  it("sends invitation and password lifecycle secrets only in mutation bodies", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    await identityApi.acceptInvitation("invite/token", "personal password");
    await identityApi.changeOwnPassword("temporary password", "personal password");
    await identityApi.resetPassword("user/id", "temporary password");
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      "/api/v1/invitations/accept",
      "/api/v1/account/password",
      "/api/v1/users/user%2Fid/password",
    ]);
    expect(fetchMock.mock.calls.map((call) => JSON.parse(call[1]?.body as string))).toEqual([
      { token: "invite/token", password: "personal password" },
      { currentPassword: "temporary password", newPassword: "personal password" },
      { password: "temporary password", mustChange: true },
    ]);
  });

  it("adds the double-submit CSRF token to state-changing requests", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("document", { cookie: "theme=dark; volund_csrf=verified-token" });
    await mutationJson<void>("/api/v1/session", "DELETE");
    expect(fetchMock.mock.calls[0]?.[1]?.headers).toMatchObject({ "X-CSRF-Token": "verified-token" });
  });

  it("uses encoded library administration endpoints", async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(new Response('{}')));
    vi.stubGlobal("fetch", fetchMock);
    await libraryApi.validatePath("/srv/cad");
    await libraryApi.create({ key: "cad", name: "CAD", path: "/srv/cad", confirmation: "ADD LIBRARY cad" });
    await libraryApi.update("cad/main", { expectedRevision: 4, enabled: false, confirmation: "UPDATE LIBRARY cad/main" });
    await libraryApi.scan("cad/main", true, "FULL SCAN cad/main");
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      "/api/v1/libraries/validate",
      "/api/v1/libraries",
      "/api/v1/libraries/cad%2Fmain",
      "/api/v1/libraries/cad%2Fmain/scans",
    ]);
  });

  it("loads the protected aggregate operations endpoint", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response('{}'));
    vi.stubGlobal("fetch", fetchMock);
    await operationsApi.health();
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/operations/health",
      { headers: { Accept: "application/json" } },
    );
  });

  it("downloads a bounded support archive with its safe server filename", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(new Blob(["tar"]), {
      headers: {
        "content-disposition": 'attachment; filename="volund-support-20260830.tar"',
        "content-type": "application/x-tar",
        "x-content-sha256": "a".repeat(64),
      },
    }));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("document", { cookie: "volund_csrf=verified-token" });
    const result = await operationsApi.supportBundle();
    expect(result.filename).toBe("volund-support-20260830.tar");
    expect(result.sha256).toBe("a".repeat(64));
    expect(fetchMock).toHaveBeenCalledWith("/api/v1/operations/support-bundle", {
      method: "POST",
      headers: { Accept: "application/x-tar", "X-CSRF-Token": "verified-token" },
    });
  });

  it("encodes central job filters and actions", async () => {
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(new Response('{"items":[]}')));
    vi.stubGlobal("fetch", fetchMock);
    const job = { id: "job/id", kind: "scan", status: "failed" } as Parameters<typeof jobsApi.retry>[0];
    await jobsApi.list({ kind: "conversion", status: "failed" });
    await jobsApi.get("scan", "job/id");
    await jobsApi.retry(job, "RETRY job/id");
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual([
      "/api/v1/jobs?limit=50&offset=0&kind=conversion&status=failed",
      "/api/v1/jobs/scan/job%2Fid",
      "/api/v1/jobs/scan/job%2Fid/retry",
    ]);
  });

  it("loads and revision-updates only the current user's preferences", async () => {
    const preferences = { previewAutoLoad: "selected", background: "dark", gridVisible: true,
      contrast: "balanced", renderStyle: "solid", problemMinimumSeverity: "warning", revision: 2 } as const;
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(new Response(JSON.stringify(preferences))));
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("document", { cookie: "volund_csrf=verified-token" });
    await identityApi.preferences();
    await identityApi.updatePreferences(preferences);
    expect(fetchMock.mock.calls.map((call) => call[0])).toEqual(["/api/v1/preferences", "/api/v1/preferences"]);
    expect(fetchMock.mock.calls[1]?.[1]?.method).toBe("PUT");
    expect(JSON.parse(String(fetchMock.mock.calls[1]?.[1]?.body))).toMatchObject({ expectedRevision: 2, background: "dark" });
  });
});
