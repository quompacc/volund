//! Declarative route inventory: the authorization policy of every API route.
//! The middleware in `api_auth` enforces it; tests check it against the router.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutePolicy {
    Public,
    Authenticated,
    CatalogWrite,
    UserAdmin,
    SettingsAdmin,
    LibraryAdmin,
    MetadataAdmin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteRule {
    pub method: &'static str,
    pub path: &'static str,
    pub policy: RoutePolicy,
}

pub const ROUTE_POLICIES: &[RouteRule] = &[
    rule("GET", "/api/v1/health", RoutePolicy::Public),
    rule("GET", "/api/v1/setup", RoutePolicy::Public),
    rule("POST", "/api/v1/setup/owner", RoutePolicy::Public),
    rule("POST", "/api/v1/invitations/accept", RoutePolicy::Public),
    rule("POST", "/api/v1/sessions", RoutePolicy::Public),
    rule("GET", "/api/v1/slicer-download/{}", RoutePolicy::Public),
    rule("GET", "/api/v1/sessions", RoutePolicy::Authenticated),
    rule("DELETE", "/api/v1/sessions/{}", RoutePolicy::Authenticated),
    rule(
        "PUT",
        "/api/v1/account/password",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/session", RoutePolicy::Authenticated),
    rule("DELETE", "/api/v1/session", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/users", RoutePolicy::UserAdmin),
    rule("POST", "/api/v1/users", RoutePolicy::UserAdmin),
    rule("POST", "/api/v1/users/invitations", RoutePolicy::UserAdmin),
    rule("PATCH", "/api/v1/users/{}", RoutePolicy::UserAdmin),
    rule("POST", "/api/v1/users/{}/password", RoutePolicy::UserAdmin),
    rule("GET", "/api/v1/settings", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/preferences", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/preferences", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/operations/health",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/operations/support-bundle",
        RoutePolicy::LibraryAdmin,
    ),
    rule("GET", "/api/v1/jobs", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/exports/metadata",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "GET",
        "/api/v1/conversion-profiles",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/conversion-profiles",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "PUT",
        "/api/v1/conversion-profiles/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "DELETE",
        "/api/v1/conversion-profiles/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule("GET", "/api/v1/scan-schedules", RoutePolicy::LibraryAdmin),
    rule("POST", "/api/v1/scan-schedules", RoutePolicy::LibraryAdmin),
    rule(
        "PUT",
        "/api/v1/scan-schedules/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "DELETE",
        "/api/v1/scan-schedules/{}",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "GET",
        "/api/v1/retention/preview",
        RoutePolicy::LibraryAdmin,
    ),
    rule("POST", "/api/v1/retention/runs", RoutePolicy::LibraryAdmin),
    rule("GET", "/api/v1/jobs/{}/{}", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/jobs/{}/{}/cancel",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/jobs/{}/{}/retry",
        RoutePolicy::LibraryAdmin,
    ),
    rule("PUT", "/api/v1/settings/{}", RoutePolicy::SettingsAdmin),
    rule("GET", "/api/v1/models", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/models/search", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/saved-views", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/problems", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/saved-views", RoutePolicy::Authenticated),
    rule(
        "DELETE",
        "/api/v1/saved-views/{}",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/models/{}", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/models/{}/files", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/models/{}/export",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/slicer-targets", RoutePolicy::Authenticated),
    rule(
        "PATCH",
        "/api/v1/models/{}/files/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/models/{}/files/{}/slicer-handoff",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/models/{}/history",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/history", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/models/{}/problems",
        RoutePolicy::Authenticated,
    ),
    rule(
        "PUT",
        "/api/v1/models/{}/problems",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/models/{}/thumbnail-candidates",
        RoutePolicy::Authenticated,
    ),
    rule(
        "PUT",
        "/api/v1/models/{}/thumbnail",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/models/{}/thumbnail/regenerate",
        RoutePolicy::CatalogWrite,
    ),
    rule("POST", "/api/v1/models", RoutePolicy::CatalogWrite),
    rule(
        "POST",
        "/api/v1/models/bulk-edit",
        RoutePolicy::CatalogWrite,
    ),
    rule("PATCH", "/api/v1/models/{}", RoutePolicy::CatalogWrite),
    rule(
        "PUT",
        "/api/v1/models/{}/primary",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/lifecycle/preview",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/quarantines", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/lifecycle/plans/{}/apply",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/artifacts/{}/content",
        RoutePolicy::Authenticated,
    ),
    rule(
        "GET",
        "/api/v1/models/{}/components",
        RoutePolicy::Authenticated,
    ),
    rule(
        "POST",
        "/api/v1/models/{}/components",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "DELETE",
        "/api/v1/models/{}/components/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/collections", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/collections", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/collections/{}", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/collections/{}", RoutePolicy::MetadataAdmin),
    rule(
        "DELETE",
        "/api/v1/collections/{}",
        RoutePolicy::MetadataAdmin,
    ),
    rule(
        "PUT",
        "/api/v1/collections/{}/models/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "DELETE",
        "/api/v1/collections/{}/models/{}",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/tags", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/tags", RoutePolicy::MetadataAdmin),
    rule("GET", "/api/v1/tags/{}", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/tags/{}", RoutePolicy::MetadataAdmin),
    rule("DELETE", "/api/v1/tags/{}", RoutePolicy::MetadataAdmin),
    rule("POST", "/api/v1/tags/{}/merge", RoutePolicy::MetadataAdmin),
    rule("GET", "/api/v1/authors", RoutePolicy::Authenticated),
    rule("POST", "/api/v1/authors", RoutePolicy::MetadataAdmin),
    rule("GET", "/api/v1/authors/{}", RoutePolicy::Authenticated),
    rule("PUT", "/api/v1/authors/{}", RoutePolicy::MetadataAdmin),
    rule(
        "POST",
        "/api/v1/authors/{}/merge",
        RoutePolicy::MetadataAdmin,
    ),
    rule("POST", "/api/v1/imports/preview", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/imports", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/imports/storage", RoutePolicy::CatalogWrite),
    rule("GET", "/api/v1/imports/{}", RoutePolicy::CatalogWrite),
    rule(
        "GET",
        "/api/v1/imports/{}/manifest",
        RoutePolicy::CatalogWrite,
    ),
    rule("PATCH", "/api/v1/imports/{}", RoutePolicy::CatalogWrite),
    rule(
        "POST",
        "/api/v1/imports/{}/cancel",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/imports/latest-uploaded",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/metadata",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/review",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/commit",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/items/{}/content",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "POST",
        "/api/v1/imports/{}/items/{}/resolution",
        RoutePolicy::CatalogWrite,
    ),
    rule("GET", "/api/v1/roots", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/roots/{}/files", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/roots/{}/folders",
        RoutePolicy::Authenticated,
    ),
    rule("GET", "/api/v1/roots/{}/scans", RoutePolicy::Authenticated),
    rule("GET", "/api/v1/libraries", RoutePolicy::LibraryAdmin),
    rule("POST", "/api/v1/libraries", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/libraries/validate",
        RoutePolicy::LibraryAdmin,
    ),
    rule(
        "POST",
        "/api/v1/libraries/dry-run",
        RoutePolicy::LibraryAdmin,
    ),
    rule("PATCH", "/api/v1/libraries/{}", RoutePolicy::LibraryAdmin),
    rule(
        "POST",
        "/api/v1/libraries/{}/scans",
        RoutePolicy::LibraryAdmin,
    ),
    rule("GET", "/api/v1/content/{}", RoutePolicy::Authenticated),
    rule(
        "GET",
        "/api/v1/files/{}/previews",
        RoutePolicy::Authenticated,
    ),
    rule(
        "POST",
        "/api/v1/files/{}/previews",
        RoutePolicy::CatalogWrite,
    ),
    rule(
        "GET",
        "/api/v1/files/{}/content",
        RoutePolicy::Authenticated,
    ),
    rule("POST", "/api/v1/files/{}/move", RoutePolicy::CatalogWrite),
    rule(
        "GET",
        "/api/v1/previews/{}/artifacts/{}",
        RoutePolicy::Authenticated,
    ),
];

const fn rule(method: &'static str, path: &'static str, policy: RoutePolicy) -> RouteRule {
    RouteRule {
        method,
        path,
        policy,
    }
}
