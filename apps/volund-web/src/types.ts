export interface Health {
  status: "ok";
  version: string;
}

export interface SetupStatus {
  initialized: boolean;
  bootstrapAvailable: boolean;
}

export interface CurrentSession {
  sessionId: string;
  userId: string;
  email: string;
  displayName: string;
  role: "owner" | "administrator" | "editor" | "viewer";
  mustChangePassword: boolean;
}

export interface OwnSession {
  id: string;
  current: boolean;
  createdAtUnixMs: number;
  lastSeenAtUnixMs: number;
  idleExpiresAtUnixMs: number;
  absoluteExpiresAtUnixMs: number;
  clientAddress: string | null;
  userAgent: string | null;
}

export interface ManagedUser {
  id: string;
  email: string;
  displayName: string;
  role: CurrentSession["role"];
  status: "active" | "disabled" | "invited" | "locked";
  createdAtUnixMs: number;
  lastLoginAtUnixMs: number | null;
  lockedUntilUnixMs: number | null;
  mustChangePassword: boolean;
}

export interface InvitationCreated {
  user: ManagedUser;
  activationToken: string;
  expiresAtUnixMs: number;
}

export interface Setting {
  key: string;
  domain: string;
  valueType: "string" | "enum" | "integer" | "boolean" | "secret";
  value: unknown;
  origin: "default" | "persisted" | "environment";
  editable: boolean;
  sensitive: boolean;
  configured?: boolean;
  revision: number;
  effect: "immediate" | "restart" | "migration";
  constraints: { allowed?: string[]; minimum?: number; maximum?: number; unit?: string; minLength?: number; maxLength?: number; operatorManaged?: boolean; compiled?: boolean };
}

export interface UserPreferences {
  previewAutoLoad: "manual" | "selected" | "visible";
  background: "dark" | "light" | "system";
  gridVisible: boolean;
  contrast: "balanced" | "high";
  renderStyle: "solid" | "wireframe";
  problemMinimumSeverity: "info" | "warning" | "error";
  revision: number;
}

export interface ConversionProfile {
  id: string; name: string; nativePreset: "web" | "fine";
  linearDeflection: number | null; angularDeflection: number | null;
  enabled: boolean; builtIn: boolean; revision: number;
}

export interface ScanSchedule {
  id: string; libraryId: string; libraryName: string; name: string;
  localTime: string; timeZone: string; weekdayMask: number; fullScan: boolean;
  enabled: boolean; nextRunAtUnixMs: number | null; lastScheduledAtUnixMs: number | null;
  lastOutcome: "enqueued" | "coalesced" | "blocked" | null; lastScanId: string | null; revision: number;
  lastScanStatus: string | null;
}

export interface RetentionPreview {
  artifactRuns: number; artifactFiles: number; artifactBytes: number;
  diagnostics: number; reasonCodes: string[];
}

export interface RetentionResult extends Omit<RetentionPreview, "diagnostics" | "reasonCodes"> {
  id: string; status: "completed" | "partial" | "failed";
  diagnosticsCleared: number; resultCodes: string[];
}

export interface LibraryRoot {
  id: string;
  key: string;
  name: string;
  readOnly: boolean;
  fileCount: number;
  missingFileCount: number;
  latestScanStatus: string | null;
}

export interface ManagedLibrary extends LibraryRoot {
  revision: number;
  filesystemPath: string;
  enabled: boolean;
  latestScanStartedAtUnixMs: number | null;
  updatedAtUnixMs: number;
  storage: StorageObservation;
}

export type OperationalState = "healthy" | "degraded" | "blocked";

export interface StorageObservation {
  state: OperationalState;
  checkedAtUnixMs: number;
  reachable: boolean;
  directory: boolean;
  readable: boolean;
  writable: boolean;
  totalBytes: number | null;
  availableBytes: number | null;
  usedPercent: number | null;
  filesystemType: string | null;
  reasons: { code: string; state: OperationalState }[];
}

export interface OperationsHealth {
  state: OperationalState;
  checkedAtUnixMs: number;
  version: string;
  database: OperationsDatabaseStatus;
  workers: OperationalComponent[];
  backup: OperationalComponent;
  libraries: ManagedLibrary[];
  recentScans: RecentOperationalScan[];
}

export interface OperationsDatabaseStatus {
  state: OperationalState;
  serverVersion: number;
  schemaTables: number;
  expectedSchemaTables: number;
  appliedMigrations: number;
  expectedMigrations: number;
}

export interface OperationalComponent {
  key: string;
  state: OperationalState;
  lastOutcome: "success" | "failed" | null;
  lastSucceededAtUnixMs: number | null;
  lastFailedAtUnixMs: number | null;
  expectedIntervalSeconds: number;
  reasons: string[];
}

export interface RecentOperationalScan {
  id: string;
  libraryKey: string;
  status: string;
  full: boolean;
  requestedAtUnixMs: number;
  finishedAtUnixMs: number | null;
  hasError: boolean;
}

export interface JobPage {
  items: ManagedJob[];
  limit: number;
  offset: number;
  total: number;
}

export interface ManagedJob {
  id: string;
  kind: "scan" | "conversion";
  status: string;
  title: string;
  context: string;
  profile: string | null;
  attempt: number;
  retryOfId: string | null;
  cancellationRequestedAtUnixMs: number | null;
  requestedAtUnixMs: number;
  startedAtUnixMs: number | null;
  finishedAtUnixMs: number | null;
  progressCurrent: number;
  progressTotal: number | null;
  diagnostic: string | null;
  canRetry: boolean;
  canCancel: boolean;
}

export interface LibraryPathValidation {
  requestedPath: string;
  canonicalPath: string;
  exists: boolean;
  directory: boolean;
  readable: boolean;
  writablePermission: boolean;
}

export interface LibraryScanResult {
  id: string;
  status: "queued" | "running";
  full: boolean;
}

export interface CadFile {
  id: string;
  path: string;
  sha256: string;
  byteSize: number;
  format: string | null;
  modifiedAtUnixMs: number;
  missing: boolean;
}

export interface MovedFile {
  id: string;
  previousPath: string;
  path: string;
}

export type LifecycleAction = "model-file.unlink" | "model.remove" | "source.quarantine" |
  "source.recover" | "source.purge" | "author.remove" | "tag.remove" | "collection.remove";

export interface LifecyclePlan {
  id: string;
  action: LifecycleAction;
  targetType: "model-file" | "model" | "source-file" | "author" | "tag" | "collection";
  targetId: string;
  parentId: string | null;
  expectedRevision: number;
  confirmation: string;
  impact: Record<string, unknown>;
  expiresAtUnixMs: number;
}

export interface LifecycleResult {
  action: LifecycleAction;
  targetId: string;
  outcome: "applied";
  revision: number;
}

export interface CatalogHistoryEvent {
  id: string;
  actorId: string | null;
  actorName: string | null;
  timestampUnixMs: number;
  action: string;
  targetType: "model" | "model-file" | "source-file" | "author" | "tag" | "collection";
  targetId: string | null;
  targetName: string | null;
  outcome: "success" | "denied" | "failure";
  revision: number | null;
  summary: Record<string, unknown>;
}

export interface QuarantineSummary {
  sourceId: string;
  relativePath: string;
  libraryName: string;
  sha256: string;
  byteSize: number;
  revision: number;
  retentionUntilUnixMs: number;
  retentionExpired: boolean;
}

export interface ModelSummary {
  id: string;
  slug: string;
  name: string;
  description: string;
  kind: "part" | "assembly" | "project";
  licenseKind: "not-specified" | "spdx" | "custom";
  licenseValue: string | null;
  authorName: string | null;
  primaryFileId: string | null;
  fileCount: number;
  formats: string[];
  updatedAtUnixMs: number;
  tags: string[];
  tagIds: string[];
  collections: string[];
  viewerRotation: [number, number, number];
  revision: number;
  thumbnail: ThumbnailState;
}

export interface ThumbnailState {
  kind: "default" | "source-file" | "derived-artifact";
  candidateId: string | null;
  url: string | null;
  status: "default" | "ready" | "generated" | "fallback";
}

export interface ThumbnailCandidate {
  id: string;
  kind: "source-file" | "derived-artifact";
  label: string;
  url: string;
  mediaType: "image/png" | "image/jpeg" | "image/gif" | "image/webp";
  byteSize: number;
}

export interface ModelHistoryEvent {
  id: string;
  actorDisplayName: string | null;
  action: string;
  outcome: "success" | "denied" | "failure";
  occurredAtUnixMs: number;
  change: Record<string, unknown>;
}

export interface ModelComponent {
  id: string;
  name: string;
  kind: "part" | "assembly" | "project";
  fileCount: number;
}

export interface UpdateModelRequest {
  expectedRevision: number;
  name: string;
  description: string;
  kind: "part" | "assembly" | "project";
  licenseKind: "not-specified" | "spdx" | "custom";
  licenseValue: string | null;
  authorName: string | null;
  tags: string[];
  tagIds: string[];
  collectionIds: string[];
  primaryFileId: string | null;
  viewerRotation: [number, number, number];
}

export interface ModelFile {
  id: string;
  path: string;
  rootKey: string;
  rootName: string;
  role: "master-cad" | "cad" | "printable-mesh" | "document" | "image" | "archive" | "other";
  primary: boolean;
  format: string | null;
  byteSize: number;
  modifiedAtUnixMs: number;
  missing: boolean;
  revision: number;
  caption: string;
  description: string;
  notes: string;
  printable: boolean;
  printed: boolean;
  preSupported: boolean;
  upAxis: "x" | "y" | "z" | null;
  supportHint: string;
  orientation: [number, number, number];
  lifecycleState: "available" | "quarantined" | "purged";
  lifecycleRevision: number;
}

export interface UpdateModelFileRequest {
  expectedRevision: number;
  caption: string;
  description: string;
  notes: string;
  printable: boolean;
  printed: boolean;
  preSupported: boolean;
  upAxis: "x" | "y" | "z" | null;
  supportHint: string;
  orientation: [number, number, number];
}

export interface SlicerTarget { id: string; name: string; scheme: string; }
export interface SlicerHandoff {
  targetId: string;
  launchUrl: string;
  downloadUrl: string;
  expiresAtUnixMs: number;
}

export interface CollectionSummary {
  id: string;
  slug: string;
  name: string;
  description: string;
  modelCount: number;
  revision: number;
  active: boolean;
  updatedAtUnixMs: number;
}

export interface CollectionDetail extends CollectionSummary { modelIds: string[]; }

export interface TagSummary {
  id: string;
  name: string;
  active: boolean;
  mergedIntoId: string | null;
  modelCount: number;
  revision: number;
  updatedAtUnixMs: number;
}

export interface AuthorSummary {
  id: string;
  name: string;
  website: string | null;
  provenanceSource: "unknown" | "import" | "user" | "website";
  provenanceNote: string | null;
  active: boolean;
  mergedIntoId: string | null;
  modelCount: number;
  revision: number;
  updatedAtUnixMs: number;
}

export interface AuthorInput {
  name: string;
  website: string | null;
  provenanceSource: AuthorSummary["provenanceSource"];
  provenanceNote: string | null;
}

export interface ImportManifestEntry {
  path: string;
  byteSize: number;
}

export interface ImportDraftItem {
  id: string;
  originalPath: string;
  byteSize: number;
  category: "cad" | "mesh" | "document" | "image" | "archive" | "other";
  suggestedRelativePath: string;
  isPrimaryCandidate: boolean;
  uploadStatus: "pending" | "uploading" | "uploaded";
}

export interface ImportDraft {
  id: string;
  sourceName: string;
  suggestedModelName: string;
  suggestedSlug: string;
  totalFiles: number;
  totalBytes: number;
  items: ImportDraftItem[];
}

export interface ImportMetadataRequest {
  modelName: string;
  kind: "part" | "assembly" | "project";
  libraryRootId: string;
  description: string;
  authorName: string | null;
  tags: string[];
  collectionIds: string[];
  targetAction?: "create" | "update" | "extend";
  targetModelId?: string | null;
  expectedModelRevision?: number | null;
  licenseKind?: "not-specified" | "spdx" | "custom";
  licenseValue?: string | null;
  primaryItemId?: string | null;
  thumbnailItemId?: string | null;
}

export interface ImportConfiguration extends ImportMetadataRequest {
  id: string;
  slug: string;
  readyForUpload: boolean;
}

export type ImportLifecycleStatus = "draft" | "uploading" | "uploaded" | "review_ready" | "reviewed" | "committing" | "committed" | "failed" | "cancelled" | "expired";
export interface ImportDraftLifecycle {
  id: string; displayName: string; status: ImportLifecycleStatus;
  actorId: string | null; actorName: string | null;
  totalFiles: number; uploadedFiles: number; totalBytes: number; uploadedBytes: number;
  createdAtUnixMs: number; updatedAtUnixMs: number; expiresAtUnixMs: number;
  targetAction: "create" | "update" | "extend" | null; targetModelId: string | null;
  lastErrorCode: string | null; resultModelId: string | null;
  canRetry: boolean; canCancel: boolean;
}
export interface ImportStorage {
  reservedBytes: number; uploadedBytes: number; reclaimableBytes: number; capacityBytes: number;
}

export interface ImportUpload {
  draftId: string;
  itemId: string;
  byteSize: number;
  sha256: string;
  status: "uploaded";
  alreadyUploaded: boolean;
}

export interface ImportReviewItem {
  id: string;
  originalPath: string;
  category: string;
  byteSize: number;
  sha256: string;
  action: "create" | "reuse" | "relocate" | "conflict" | "skip";
  targetPath: string;
  existingFileId: string | null;
  existingPath: string | null;
  isPrimary: boolean;
}

export interface ImportReview {
  draftId: string;
  modelName: string;
  modelAction: "create" | "update" | "extend";
  existingModelId: string | null;
  rootKey: string;
  rootName: string;
  baseDirectory: string;
  createFiles: number;
  reuseFiles: number;
  relocateFiles: number;
  conflicts: number;
  skipFiles: number;
  newBytes: number;
  savedBytes: number;
  items: ImportReviewItem[];
}

export interface ImportCommit {
  draftId: string;
  modelId: string;
  modelName: string;
  totalFiles: number;
  createdFiles: number;
  reusedFiles: number;
  relocatedFiles: number;
  status: "committed";
}

export interface Folder {
  name: string;
  path: string;
  fileCount: number;
}

export type CatalogSort = "path" | "format" | "size" | "modified";
export type SortDirection = "asc" | "desc";

export interface CatalogOptions {
  directory: string;
  query: string;
  format: string;
  sort: CatalogSort;
  direction: SortDirection;
  offset: number;
  limit: number;
}

export interface Artifact {
  kind: "preview-glb" | "thumbnail-raster" | "assembly-manifest" | "diagnostics" | "result";
  url: string;
  sha256: string;
  byteSize: number;
  mediaType: string;
}

export interface Preview {
  id: string;
  profile: "web" | "fine";
  status: string;
  converterVersion: string;
  requestedAtUnixMs: number;
  finishedAtUnixMs: number | null;
  artifacts: Artifact[];
}

export interface PreviewEnqueue {
  id: string;
  status: "queued" | "running" | "ready";
}

export interface Page<T> {
  items: T[];
  limit: number;
  offset: number;
  total: number;
}
