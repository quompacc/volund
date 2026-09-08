# VÖLUND product contract

Status: Baseline for product planning  
Last reviewed: 2026-08-29

## Purpose

VÖLUND is a native, self-hosted engineering vault for complete CAD projects.
It keeps parts, assemblies, projects, original CAD files, printable meshes,
drawings, documents, images, tables, archives, and their metadata together for
the entire useful life of the project.

VÖLUND is not merely a file index, importer, or 3D viewer. It is the beginning
of a sovereign engineering toolchain: administration, ingestion, cataloguing,
inspection, organization, collaboration, lifecycle management, export, and
recovery belong to the product.

The product is currently an early technical foundation and design prototype.
Existing capabilities must not be presented as a complete user workflow until
the applicable acceptance criteria in this contract and the roadmap are met.

## Product promise

A self-hosting owner can install VÖLUND, configure storage, create users, import
or index a complete engineering project, organize it without changing original
bytes, inspect every associated file, maintain its metadata and relationships,
control access, and export or restore all owned data without dependence on
VÖLUND.

## Non-negotiable principles

1. **Sovereignty.** Originals and user-created metadata have a documented,
   machine-readable exit path.
2. **Original safety.** Indexing, viewing, search, and preview generation never
   alter authoritative files. Explicit mutations are narrow, audited, and
   recoverable where possible.
3. **Stable identity.** Logical objects and file identities survive controlled
   path changes.
4. **Truthful UX.** A prototype, partial workflow, queued operation, failure,
   and completed capability are visibly distinguishable.
5. **Native operation.** Production runs natively on Debian with PostgreSQL and
   systemd; application containers are not a runtime dependency.
6. **Bounded processing.** Uploads, archive handling, conversion, indexing, and
   background work enforce explicit resource limits.
7. **Accessible administration.** Routine configuration and recovery do not
   require direct database edits.
8. **Forward compatibility.** Persisted schemas evolve through immutable,
   forward-only migrations.
9. **Observable behavior.** Important operations expose status, diagnostics,
   history, and actionable failures.
10. **Tested workflows.** A feature is not complete because its table or API
    exists; its full user journey must satisfy acceptance tests.

## Product boundaries

### In scope

- Single-instance and small-team self-hosting.
- Local users, roles, sessions, and instance administration.
- Optional external identity integration after local authentication is stable.
- Multiple libraries and controlled storage roots.
- Parts, assemblies, projects, revisions, relationships, and complete filesets.
- CAD, mesh, document, image, table, archive, and sidecar file handling.
- Metadata, authors, tags, collections, licenses, thumbnails, and provenance.
- Native preview generation and browser-based inspection.
- Search, filtering, saved views, activity, export, backup, and restore.
- Stable, versioned APIs for supported integrations and automation.

### Not an initial goal

- Replacing a parametric CAD authoring application.
- Editing STEP/BREP geometry in the browser.
- Public multi-tenant SaaS operation.
- A production dependency on Docker, FreeCAD, or a proprietary cloud service.
- Full enterprise PLM, ERP, purchasing, or manufacturing execution in the first
  product generations.

These exclusions do not prevent later integrations or deliberately scoped PLM
capabilities.

## Roles

Roles describe product behavior, not yet-implemented database entities.

### Instance owner

- Completes initial setup and recovery.
- Controls instance identity, authentication policy, storage, backups, updates,
  and global defaults.
- Can appoint administrators but cannot remove the last viable owner.

### Administrator

- Manages users, invitations, roles, libraries, conversion settings, jobs, and
  instance-wide metadata vocabularies.
- Can inspect audit and health information.

### Editor

- Imports and maintains permitted projects, parts, assemblies, files, metadata,
  relationships, thumbnails, and collections.
- Cannot change instance security or storage policy without a higher role.

### Viewer

- Searches, views, downloads, and exports content explicitly available to the
  viewer.
- Cannot mutate catalog or instance state.

### Service identity

- Uses revocable, scoped credentials for documented automation endpoints.
- Is never represented by a shared human password.

## Core domain language

- **Instance:** one independently operated VÖLUND installation.
- **Library:** a configured authoritative storage boundary.
- **Source file:** a stable observation of an original file in a library.
- **Content object:** content-addressed identity shared by identical bytes.
- **Model:** a user-facing part, assembly, or project.
- **Fileset:** all source files associated with a model, with semantic roles.
- **Revision:** an immutable or explicitly finalized state of a model.
- **Author:** a reusable creator or source identity, not merely free text.
- **Collection:** a user-managed grouping independent of storage paths.
- **Tag:** reusable lightweight classification.
- **Thumbnail:** a chosen image or generated preview representing a model.
- **Derived artifact:** rebuildable output such as GLB, image preview, manifest,
  or diagnostics.
- **Import draft:** a resumable, reviewable proposal before publication.
- **Audit event:** durable record of a security- or data-relevant action.

## Required end-to-end journeys

### First-run setup

1. The owner reaches a clearly identified setup flow on an uninitialized
   instance.
2. The owner creates the first account and configures instance identity,
   language, time zone, storage, and backup expectations.
3. VÖLUND validates paths, permissions, database state, converter availability,
   and writable capacity before declaring the instance ready.
4. Setup is idempotent and cannot silently create a second owner.

### User and access administration

1. An authorized administrator lists active, invited, disabled, and locked
   users.
2. The administrator creates or invites a user, assigns a role, changes access,
   and revokes sessions.
3. Authentication, authorization failure, and security-relevant changes are
   audited without recording secrets.
4. Every mutating API independently enforces authorization.

### Instance settings

1. Administrators can view effective settings and their origin.
2. Editable settings are validated before persistence.
3. Settings that require a restart or migration say so before submission.
4. Sensitive values are masked and never returned through ordinary APIs.
5. Defaults exist for import layout, previews, locale, thumbnails, retention,
   and background processing.

### Library onboarding and indexing

1. An administrator adds a library through a validated workflow.
2. A dry run reports reachability, permissions, filesystem characteristics,
   estimated scope, and conflicts.
3. Scan progress and failures are visible; cancellation and safe retry are
   defined.
4. Indexing leaves originals unchanged and records missing paths without
   deleting historical identity.

### Import and publication

1. An editor selects files, directories, or supported archives.
2. VÖLUND proposes a model, fileset, author, tags, collections, primary CAD,
   thumbnail candidates, and destination without publishing originals.
3. Upload is resumable and progress is understandable.
4. Review summarizes creates, reuse, relocation, conflicts, and space impact.
5. One confirmation publishes files and metadata atomically or leaves a
   recoverable draft with a clear failure.
6. Completion opens the created or updated model.

### Model maintenance

1. A model page shows every associated file with role, path, type, size,
   availability, preview state, and safe actions.
2. Authorized editors can change name, description, type, author, tags,
   collections, license, primary file, and thumbnail after import.
3. Authors, collections, and tags can be created and managed from their own
   catalog pages as well as selected contextually.
4. A thumbnail can use an associated image, a generated CAD preview, or a
   deliberate default; broken sources fall back visibly.
5. Changes are validated, authorized, atomic, and audited.

### Inspection and discovery

1. Users can search models and files by meaningful metadata and content facts.
2. Filters, sorting, pagination, and saved views remain bounded on large
   libraries.
3. Supported models open in the appropriate 3D or document/image viewer.
4. Assemblies expose hierarchy, repeated instances, names, transforms, colors,
   and diagnostics when present.

### Export, backup, and recovery

1. A model can be exported with originals, metadata, relationships, and a
   documented manifest.
2. An instance metadata export is machine-readable and versioned.
3. Backup health and last successful completion are visible.
4. Restore is documented and tested on an empty replacement instance.
5. Derived artifacts may be omitted and rebuilt without loss of originals or
   user metadata.

## Settings domains

- Instance: name, base URL, locale, time zone, branding.
- Security: authentication mode, session policy, invitations, password policy,
  trusted proxy behavior, and later external identity providers.
- Users and roles: membership, status, role assignments, scoped access.
- Libraries: paths, capacity warnings, read/write policy, scan schedule.
- Imports: layout rules, collision policy, size limits, retention, defaults.
- Conversion: worker paths, profiles, timeouts, concurrency, resource limits.
- Previews and thumbnails: preferred source, generated styles, fallbacks.
- Metadata: kinds, licenses, tag policy, author and collection defaults.
- Notifications: job failures, capacity, backup, security events.
- Backup and export: targets, schedules, retention, verification.
- Maintenance: logs, jobs, diagnostics, updates, data integrity checks.

## Security and privacy baseline

- No anonymous mutation endpoints.
- Passwords use a current memory-hard password hash and are never logged.
- Sessions are revocable, time-bounded, protected against fixation, and use
  secure cookie attributes when deployed behind TLS.
- State-changing browser requests have CSRF protection appropriate to the
  authentication design.
- API tokens are hashed at rest, scoped, expiring where practical, and shown
  only once.
- Authorization is deny-by-default and tested at route and domain boundaries.
- File streaming resolves catalog identities, canonicalizes paths, and refuses
  escapes from configured roots.
- Audit records identify actor, action, target, result, and timestamp without
  leaking credentials or original file contents.

## Definition of a complete feature

A capability is **complete** only when all applicable items are true:

- The user journey and permission rules are documented.
- Empty, loading, success, validation, conflict, retry, and failure states exist.
- Backend, browser, migration, and filesystem boundaries are explicit.
- Public API changes update the versioned contract.
- Automated tests cover happy path, validation, authorization, and regression.
- Large inputs are bounded and representative production data is exercised.
- Accessibility includes keyboard operation, labels, focus, and readable status.
- Operations expose diagnostics without leaking sensitive or host-only paths.
- Upgrade, backup, rollback constraints, and data compatibility are understood.
- User-facing documentation is updated.
- The capability is verified on the native Debian target.

Database tables, placeholder pages, mock data, or isolated endpoints count as
**foundation** or **partial**, never as a completed feature.

## Contract governance

- `docs/PRODUCT_CONTRACT.md` defines what the product must become.
- This contract defines scope and maturity; release history is in `CHANGELOG.md`.
- ADRs define consequential technical decisions, not product completion.
- The changelog records delivered behavior, not planned behavior.
- Each implementation milestone must link to roadmap acceptance criteria.
- Scope changes update this contract before or with implementation.
- Product status is reviewed after every milestone; optimistic status changes
  without evidence are not allowed.
