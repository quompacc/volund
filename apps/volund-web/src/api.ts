import type { ModelProblem } from "./problem-types";
import { checkSessionResponse } from "./session-expiry";
import type { AuthorInput, AuthorSummary, CadFile, CatalogHistoryEvent, CatalogOptions, CollectionDetail, CollectionSummary, ConversionProfile, CurrentSession, Folder, Health, ImportCommit, ImportConfiguration, ImportDraft, ImportDraftLifecycle, ImportManifestEntry, ImportMetadataRequest, ImportReview, ImportStorage, ImportUpload, InvitationCreated, JobPage, LibraryPathValidation, LibraryRoot, LibraryScanResult, LifecycleAction, LifecyclePlan, LifecycleResult, ManagedJob, ManagedLibrary, ManagedUser, ModelComponent, ModelFile, ModelHistoryEvent, ModelSummary, MovedFile, OperationsHealth, OwnSession, Page, Preview, PreviewEnqueue, QuarantineSummary, RetentionPreview, RetentionResult, ScanSchedule, Setting, SetupStatus, SlicerHandoff, SlicerTarget, TagSummary, ThumbnailCandidate, UpdateModelFileRequest, UpdateModelRequest, UserPreferences } from "./types";

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

const JSON_REQUEST_TIMEOUT_MS = 30_000;
const READ_TIMEOUT_MESSAGE = "Die Anfrage hat zu lange gedauert. Prüfe die Verbindung und versuche es erneut.";
const MUTATION_TIMEOUT_MESSAGE = "Die Antwort hat zu lange gedauert. Der Ausgang der Aktion ist unbekannt. Lade die Ansicht neu, bevor du sie wiederholst.";

async function fetchWithJsonDeadline(path: string, init: RequestInit, timeoutMessage: string): Promise<Response> {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<Response>((_resolve, reject) => {
    timeout = setTimeout(() => reject(new ApiError(0, timeoutMessage)), JSON_REQUEST_TIMEOUT_MS);
  });
  try { return await Promise.race([fetch(path, init), deadline]); }
  finally { if (timeout !== undefined) clearTimeout(timeout); }
}

export async function requestJson<T>(path: string): Promise<T> {
  const response = await fetchWithJsonDeadline(path, { headers: { Accept: "application/json" } }, READ_TIMEOUT_MESSAGE);
  checkSessionResponse(response.status);
  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const body = (await response.json()) as { error?: { message?: string } };
      message = body.error?.message ?? message;
    } catch {
      // A proxy-generated response does not have to be JSON.
    }
    throw new ApiError(response.status, message);
  }
  return (await response.json()) as T;
}

export async function postJson<T>(path: string, body: unknown): Promise<T> {
  return mutationJson<T>(path, "POST", body);
}

export async function mutationJson<T>(path: string, method: "POST" | "PUT" | "PATCH" | "DELETE", body?: unknown, headers: Record<string, string> = {}): Promise<T> {
  const csrf = csrfToken();
  const response = await fetchWithJsonDeadline(path, {
    method,
    headers: { Accept: "application/json", ...(body === undefined ? {} : { "Content-Type": "application/json" }), ...(csrf ? { "X-CSRF-Token": csrf } : {}), ...headers },
    body: body === undefined ? undefined : JSON.stringify(body),
  }, MUTATION_TIMEOUT_MESSAGE);
  checkSessionResponse(response.status);
  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const payload = (await response.json()) as { error?: { message?: string } };
      message = payload.error?.message ?? message;
    } catch {
      // A proxy-generated response does not have to be JSON.
    }
    throw new ApiError(response.status, message);
  }
  return response.status === 204 ? undefined as T : (await response.json()) as T;
}

export async function postDownload(path: string): Promise<{ blob: Blob; filename: string; sha256: string | null }> {
  const csrf = csrfToken();
  const response = await fetch(path, {
    method: "POST",
    headers: { Accept: "application/x-tar", ...(csrf ? { "X-CSRF-Token": csrf } : {}) },
  });
  checkSessionResponse(response.status);
  if (!response.ok) throw new ApiError(response.status, `HTTP ${response.status}`);
  const disposition = response.headers.get("content-disposition") ?? "";
  const filename = /filename="([A-Za-z0-9._-]+)"/.exec(disposition)?.[1] ?? "volund-support.tar";
  return { blob: await response.blob(), filename, sha256: response.headers.get("x-content-sha256") };
}

function csrfToken(): string | undefined {
  if (typeof document === "undefined") return undefined;
  for (const part of document.cookie.split(";")) {
    const [name, value] = part.trim().split("=", 2);
    if (name === "volund_csrf" || name === "__Host-volund_csrf") return value;
  }
  return undefined;
}

export async function postFile<T>(path: string, file: Blob): Promise<T> {
  const csrf = csrfToken();
  const response = await fetch(path, {
    method: "POST",
    headers: { Accept: "application/json", "Content-Type": "application/octet-stream", ...(csrf ? { "X-CSRF-Token": csrf } : {}) },
    body: file,
  });
  checkSessionResponse(response.status);
  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const payload = (await response.json()) as { error?: { message?: string } };
      message = payload.error?.message ?? message;
    } catch {
      // A proxy-generated response does not have to be JSON.
    }
    throw new ApiError(response.status, message);
  }
  return (await response.json()) as T;
}

export function catalogUrl(rootKey: string, resource: "files" | "folders", options: CatalogOptions): string {
  const parameters = new URLSearchParams({
    limit: String(options.limit),
    offset: String(options.offset),
    sort: options.sort,
    direction: options.direction,
  });
  if (options.directory) parameters.set("directory", options.directory);
  if (options.query) parameters.set("q", options.query);
  if (options.format) parameters.set("format", options.format);
  return `/api/v1/roots/${encodeURIComponent(rootKey)}/${resource}?${parameters}`;
}

export const catalogApi = {
  health: (): Promise<Health> => requestJson("/api/v1/health"),
  roots: (): Promise<LibraryRoot[]> => requestJson("/api/v1/roots"),
  files: (rootKey: string, options: CatalogOptions): Promise<Page<CadFile>> =>
    requestJson(catalogUrl(rootKey, "files", options)),
  folders: (rootKey: string, options: CatalogOptions): Promise<Page<Folder>> =>
    requestJson(catalogUrl(rootKey, "folders", options)),
  previews: (fileId: string): Promise<Page<Preview>> =>
    requestJson(`/api/v1/files/${encodeURIComponent(fileId)}/previews?limit=50`),
  enqueuePreview: (fileId: string, profile: "web" | "fine" = "web"): Promise<PreviewEnqueue> =>
    postJson(`/api/v1/files/${encodeURIComponent(fileId)}/previews`, { profile }),
  sourceContentUrl: (fileId: string): string => `/api/v1/files/${encodeURIComponent(fileId)}/content`,
  sourceDownloadUrl: (fileId: string): string => `/api/v1/files/${encodeURIComponent(fileId)}/content?download=true`,
  moveFile: (fileId: string, destinationDirectory: string): Promise<MovedFile> =>
    postJson(`/api/v1/files/${encodeURIComponent(fileId)}/move`, { destinationDirectory }),
  models: (): Promise<ModelSummary[]> => requestJson("/api/v1/models"),
  previewLifecycle: (action: LifecycleAction, targetId: string, expectedRevision: number, parentId: string | null = null): Promise<LifecyclePlan> =>
    postJson("/api/v1/lifecycle/preview", { action, targetId, parentId, expectedRevision }),
  applyLifecycle: (plan: LifecyclePlan, confirmation: string): Promise<LifecycleResult> =>
    postJson(`/api/v1/lifecycle/plans/${encodeURIComponent(plan.id)}/apply`, { confirmation }),
  history: (targetType: CatalogHistoryEvent["targetType"], targetId: string, offset = 0): Promise<Page<CatalogHistoryEvent>> =>
    requestJson(`/api/v1/history?targetType=${encodeURIComponent(targetType)}&targetId=${encodeURIComponent(targetId)}&limit=50&offset=${offset}`),
  quarantines: (offset = 0): Promise<Page<QuarantineSummary>> => requestJson(`/api/v1/quarantines?limit=100&offset=${offset}`),
  model: (modelId: string): Promise<ModelSummary> =>
    requestJson(`/api/v1/models/${encodeURIComponent(modelId)}`),
  modelFiles: (modelId: string, offset = 0): Promise<Page<ModelFile>> =>
    requestJson(`/api/v1/models/${encodeURIComponent(modelId)}/files?limit=200&offset=${offset}`),
  updateModelFile: (modelId: string, fileId: string, request: UpdateModelFileRequest): Promise<ModelFile> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(modelId)}/files/${encodeURIComponent(fileId)}`, "PATCH", request),
  slicerTargets: (): Promise<SlicerTarget[]> => requestJson("/api/v1/slicer-targets"),
  createSlicerHandoff: (modelId: string, fileId: string, targetId: string): Promise<SlicerHandoff> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(modelId)}/files/${encodeURIComponent(fileId)}/slicer-handoff`, "POST", { targetId }),
  modelHistory: (modelId: string, offset = 0): Promise<Page<ModelHistoryEvent>> =>
    requestJson(`/api/v1/models/${encodeURIComponent(modelId)}/history?limit=50&offset=${offset}`),
  modelProblems: (modelId: string): Promise<ModelProblem[]> =>
    requestJson(`/api/v1/models/${encodeURIComponent(modelId)}/problems`),
  updateModelProblems: (model: ModelSummary, keys: string[], status: ModelProblem["status"]): Promise<ModelProblem[]> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(model.id)}/problems`, "PUT", { expectedRevision: model.revision, keys, status }),
  thumbnailCandidates: (modelId: string): Promise<ThumbnailCandidate[]> =>
    requestJson(`/api/v1/models/${encodeURIComponent(modelId)}/thumbnail-candidates`),
  updateThumbnail: (model: ModelSummary, kind: ModelSummary["thumbnail"]["kind"], candidateId: string | null): Promise<ModelSummary> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(model.id)}/thumbnail`, "PUT",
      { expectedRevision: model.revision, kind, candidateId }),
  regenerateThumbnail: (modelId: string, expectedRevision: number, profile: "web" | "fine"): Promise<PreviewEnqueue> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(modelId)}/thumbnail/regenerate`, "POST",
      { expectedRevision, profile }),
  updateModel: (modelId: string, request: UpdateModelRequest): Promise<ModelSummary> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(modelId)}`, "PATCH", request),
  updateModelPrimary: (model: ModelSummary, primaryFileId: string): Promise<ModelSummary> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(model.id)}/primary`, "PUT",
      { expectedRevision: model.revision, primaryFileId }),
  modelComponents: (modelId: string): Promise<ModelComponent[]> =>
    requestJson(`/api/v1/models/${encodeURIComponent(modelId)}/components`),
  addModelComponent: (model: ModelSummary, childModelId: string): Promise<void> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(model.id)}/components`, "POST",
      { expectedRevision: model.revision, childModelId }),
  removeModelComponent: (model: ModelSummary, childModelId: string): Promise<void> =>
    mutationJson(`/api/v1/models/${encodeURIComponent(model.id)}/components/${encodeURIComponent(childModelId)}`, "DELETE",
      { expectedRevision: model.revision }),
  collectionPage: (offset = 0): Promise<Page<CollectionSummary>> =>
    requestJson(`/api/v1/collections?limit=100&offset=${offset}`),
  collections: async (): Promise<CollectionSummary[]> => (await catalogApi.collectionPage()).items,
  collection: (id: string): Promise<CollectionDetail> =>
    requestJson(`/api/v1/collections/${encodeURIComponent(id)}`),
  createCollection: (request: { name: string; description: string }): Promise<CollectionSummary> =>
    postJson("/api/v1/collections", request),
  updateCollection: (collection: CollectionSummary, request: { name: string; description: string }): Promise<CollectionDetail> =>
    mutationJson(`/api/v1/collections/${encodeURIComponent(collection.id)}`, "PUT",
      { ...request, expectedRevision: collection.revision }),
  setCollectionMember: (collection: CollectionSummary, modelId: string, present: boolean): Promise<CollectionDetail> =>
    mutationJson(`/api/v1/collections/${encodeURIComponent(collection.id)}/models/${encodeURIComponent(modelId)}`,
      present ? "PUT" : "DELETE", { expectedRevision: collection.revision }),
  removeCollection: (collection: CollectionSummary, confirmation: string): Promise<CollectionDetail> =>
    mutationJson(`/api/v1/collections/${encodeURIComponent(collection.id)}`, "DELETE",
      { expectedRevision: collection.revision, confirmation }),
  tags: (includeInactive = false, offset = 0): Promise<Page<TagSummary>> =>
    requestJson(`/api/v1/tags?limit=100&offset=${offset}&includeInactive=${includeInactive}`),
  createTag: (name: string): Promise<TagSummary> => postJson("/api/v1/tags", { name }),
  updateTag: (tag: TagSummary, name: string): Promise<TagSummary> =>
    mutationJson(`/api/v1/tags/${encodeURIComponent(tag.id)}`, "PUT",
      { expectedRevision: tag.revision, name }),
  mergeTag: (source: TagSummary, targetTagId: string, confirmation: string): Promise<TagSummary> =>
    postJson(`/api/v1/tags/${encodeURIComponent(source.id)}/merge`,
      { expectedRevision: source.revision, targetTagId, confirmation }),
  removeTag: (tag: TagSummary, confirmation: string): Promise<TagSummary> =>
    mutationJson(`/api/v1/tags/${encodeURIComponent(tag.id)}`, "DELETE",
      { expectedRevision: tag.revision, confirmation }),
  authors: (includeMerged = false): Promise<Page<AuthorSummary>> =>
    requestJson(`/api/v1/authors?limit=100&offset=0&includeMerged=${includeMerged}`),
  author: (authorId: string): Promise<AuthorSummary> =>
    requestJson(`/api/v1/authors/${encodeURIComponent(authorId)}`),
  createAuthor: (request: AuthorInput): Promise<AuthorSummary> =>
    postJson("/api/v1/authors", request),
  updateAuthor: (author: AuthorSummary, request: AuthorInput): Promise<AuthorSummary> =>
    mutationJson(`/api/v1/authors/${encodeURIComponent(author.id)}`, "PUT",
      { ...request, expectedRevision: author.revision }),
  mergeAuthor: (sourceId: string, targetAuthorId: string, confirmation: string): Promise<AuthorSummary> =>
    postJson(`/api/v1/authors/${encodeURIComponent(sourceId)}/merge`, { targetAuthorId, confirmation }),
  createModel: (request: { name: string; slug: string; kind: string; primaryFileId: string }): Promise<ModelSummary> =>
    postJson("/api/v1/models", request),
  previewImport: (sourceName: string, entries: ImportManifestEntry[]): Promise<ImportDraft> =>
    postJson("/api/v1/imports/preview", { sourceName, entries }),
  configureImport: (draftId: string, request: ImportMetadataRequest): Promise<ImportConfiguration> =>
    postJson(`/api/v1/imports/${encodeURIComponent(draftId)}/metadata`, request),
  uploadImportItem: (draftId: string, itemId: string, file: Blob): Promise<ImportUpload> =>
    postFile(`/api/v1/imports/${encodeURIComponent(draftId)}/items/${encodeURIComponent(itemId)}/content`, file),
  latestUploadedImport: (): Promise<ImportDraft | null> => requestJson("/api/v1/imports/latest-uploaded"),
  reviewImport: (draftId: string): Promise<ImportReview> =>
    postJson(`/api/v1/imports/${encodeURIComponent(draftId)}/review`, {}),
  commitImport: (draftId: string): Promise<ImportCommit> =>
    postJson(`/api/v1/imports/${encodeURIComponent(draftId)}/commit`, {}),
  importDrafts: (status = "", offset = 0): Promise<Page<ImportDraftLifecycle>> =>
    requestJson(`/api/v1/imports?limit=50&offset=${offset}${status ? `&status=${encodeURIComponent(status)}` : ""}`),
  importStorage: (): Promise<ImportStorage> => requestJson("/api/v1/imports/storage"),
  importDraft: (draftId: string): Promise<ImportDraftLifecycle> =>
    requestJson(`/api/v1/imports/${encodeURIComponent(draftId)}`),
  importDraftManifest: (draftId: string): Promise<ImportDraft> =>
    requestJson(`/api/v1/imports/${encodeURIComponent(draftId)}/manifest`),
  renameImport: (draftId: string, displayName: string): Promise<ImportDraftLifecycle> =>
    mutationJson(`/api/v1/imports/${encodeURIComponent(draftId)}`, "PATCH", { displayName }),
  cancelImport: (draftId: string): Promise<{ id: string; status: "cancelled"; stagingCleaned: boolean }> =>
    postJson(`/api/v1/imports/${encodeURIComponent(draftId)}/cancel`, { confirmation: `CANCEL IMPORT ${draftId}` }),
  resolveImportItem: (draftId: string, itemId: string, action: "create" | "reuse" | "relocate" | "skip", targetPath: string | null): Promise<void> =>
    postJson(`/api/v1/imports/${encodeURIComponent(draftId)}/items/${encodeURIComponent(itemId)}/resolution`, { action, targetPath }),
};

export const identityApi = {
  setupStatus: (): Promise<SetupStatus> => requestJson("/api/v1/setup"),
  setupOwner: (token: string, body: { email: string; displayName: string; password: string }): Promise<ManagedUser> =>
    mutationJson("/api/v1/setup/owner", "POST", body, { "X-Volund-Setup-Token": token }),
  login: (email: string, password: string): Promise<unknown> =>
    mutationJson("/api/v1/sessions", "POST", { email, password }),
  current: (): Promise<CurrentSession> => requestJson("/api/v1/session"),
  logout: (): Promise<void> => mutationJson("/api/v1/session", "DELETE"),
  sessions: (): Promise<OwnSession[]> => requestJson("/api/v1/sessions"),
  revokeSession: (id: string): Promise<void> => mutationJson(`/api/v1/sessions/${encodeURIComponent(id)}`, "DELETE"),
  users: (): Promise<ManagedUser[]> => requestJson("/api/v1/users"),
  createUser: (body: { email: string; displayName: string; role: string; password: string }): Promise<ManagedUser> =>
    mutationJson("/api/v1/users", "POST", body),
  inviteUser: (body: { email: string; displayName: string; role: string }): Promise<InvitationCreated> =>
    mutationJson("/api/v1/users/invitations", "POST", body),
  acceptInvitation: (token: string, password: string): Promise<void> =>
    mutationJson("/api/v1/invitations/accept", "POST", { token, password }),
  changeOwnPassword: (currentPassword: string, newPassword: string): Promise<void> =>
    mutationJson("/api/v1/account/password", "PUT", { currentPassword, newPassword }),
  updateUser: (id: string, body: { displayName?: string; role?: string; status?: string }): Promise<ManagedUser> =>
    mutationJson(`/api/v1/users/${encodeURIComponent(id)}`, "PATCH", body),
  resetPassword: (id: string, password: string): Promise<void> =>
    mutationJson(`/api/v1/users/${encodeURIComponent(id)}/password`, "POST", { password, mustChange: true }),
  settings: (): Promise<Setting[]> => requestJson("/api/v1/settings"),
  preferences: (): Promise<UserPreferences> => requestJson("/api/v1/preferences"),
  updatePreferences: (preferences: UserPreferences): Promise<UserPreferences> =>
    mutationJson("/api/v1/preferences", "PUT", {
      previewAutoLoad: preferences.previewAutoLoad, background: preferences.background,
      gridVisible: preferences.gridVisible, contrast: preferences.contrast,
      renderStyle: preferences.renderStyle, problemMinimumSeverity: preferences.problemMinimumSeverity,
      expectedRevision: preferences.revision,
    }),
  updateSetting: (setting: Setting, value: unknown, confirmation?: string): Promise<Setting> =>
    mutationJson(`/api/v1/settings/${encodeURIComponent(setting.key)}`, "PUT", { value, expectedRevision: setting.revision, confirmation }),
};

export const libraryApi = {
  list: (): Promise<ManagedLibrary[]> => requestJson("/api/v1/libraries"),
  validatePath: (path: string): Promise<LibraryPathValidation> =>
    postJson("/api/v1/libraries/validate", { path }),
  create: (body: { key: string; name: string; path: string; confirmation: string }): Promise<ManagedLibrary> =>
    postJson("/api/v1/libraries", body),
  update: (key: string, body: { expectedRevision: number; name?: string; enabled?: boolean; confirmation: string }): Promise<ManagedLibrary> =>
    mutationJson(`/api/v1/libraries/${encodeURIComponent(key)}`, "PATCH", body),
  scan: (key: string, full: boolean, confirmation?: string): Promise<LibraryScanResult> =>
    postJson(`/api/v1/libraries/${encodeURIComponent(key)}/scans`, { full, confirmation }),
};

export const operationsApi = {
  health: (): Promise<OperationsHealth> => requestJson("/api/v1/operations/health"),
  supportBundle: (): Promise<{ blob: Blob; filename: string; sha256: string | null }> => postDownload("/api/v1/operations/support-bundle"),
};

export const policyApi = {
  profiles: (): Promise<ConversionProfile[]> => requestJson("/api/v1/conversion-profiles"),
  createProfile: (body: Omit<ConversionProfile, "id" | "builtIn" | "revision">, confirmation: string): Promise<ConversionProfile> => postJson("/api/v1/conversion-profiles", { ...body, confirmation }),
  updateProfile: (profile: ConversionProfile, body: Omit<ConversionProfile, "id" | "builtIn" | "revision">, confirmation: string): Promise<ConversionProfile> => mutationJson(`/api/v1/conversion-profiles/${encodeURIComponent(profile.id)}`, "PUT", { ...body, expectedRevision: profile.revision, confirmation }),
  retireProfile: (id: string, expectedRevision: number, confirmation: string): Promise<ConversionProfile> => mutationJson(`/api/v1/conversion-profiles/${encodeURIComponent(id)}`, "DELETE", { confirmation, expectedRevision }),
  schedules: (): Promise<ScanSchedule[]> => requestJson("/api/v1/scan-schedules"),
  createSchedule: (body: Omit<ScanSchedule, "id" | "libraryName" | "nextRunAtUnixMs" | "lastScheduledAtUnixMs" | "lastOutcome" | "lastScanId" | "lastScanStatus" | "revision">, confirmation: string): Promise<ScanSchedule> => postJson("/api/v1/scan-schedules", { ...body, confirmation }),
  updateSchedule: (schedule: ScanSchedule, body: Omit<ScanSchedule, "id" | "libraryName" | "nextRunAtUnixMs" | "lastScheduledAtUnixMs" | "lastOutcome" | "lastScanId" | "lastScanStatus" | "revision">, confirmation: string): Promise<ScanSchedule> => mutationJson(`/api/v1/scan-schedules/${encodeURIComponent(schedule.id)}`, "PUT", { ...body, expectedRevision: schedule.revision, confirmation }),
  deleteSchedule: (id: string, expectedRevision: number, confirmation: string): Promise<void> => mutationJson(`/api/v1/scan-schedules/${encodeURIComponent(id)}`, "DELETE", { confirmation, expectedRevision }),
  retentionPreview: (): Promise<RetentionPreview> => requestJson("/api/v1/retention/preview"),
  runRetention: (confirmation: string): Promise<RetentionResult> => postJson("/api/v1/retention/runs", { confirmation }),
};

export const jobsApi = {
  list: (filters: { kind?: string; status?: string; offset?: number } = {}): Promise<JobPage> => {
    const query = new URLSearchParams({ limit: "50", offset: String(filters.offset ?? 0) });
    if (filters.kind) query.set("kind", filters.kind);
    if (filters.status) query.set("status", filters.status);
    return requestJson(`/api/v1/jobs?${query}`);
  },
  get: (kind: string, id: string): Promise<ManagedJob> =>
    requestJson(`/api/v1/jobs/${encodeURIComponent(kind)}/${encodeURIComponent(id)}`),
  cancel: (job: ManagedJob, confirmation: string): Promise<ManagedJob> =>
    postJson(`/api/v1/jobs/${encodeURIComponent(job.kind)}/${encodeURIComponent(job.id)}/cancel`, { confirmation }),
  retry: (job: ManagedJob, confirmation: string): Promise<ManagedJob> =>
    postJson(`/api/v1/jobs/${encodeURIComponent(job.kind)}/${encodeURIComponent(job.id)}/retry`, { confirmation }),
};
