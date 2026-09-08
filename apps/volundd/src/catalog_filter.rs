use crate::api_query::CatalogQuery;

const FORMATS: [&str; 9] = [
    "step", "iges", "brep", "stl", "3mf", "obj", "ply", "gltf", "glb",
];
const SORT_KEYS: [&str; 4] = ["path", "format", "size", "modified"];

#[derive(Debug, Eq, PartialEq)]
pub struct CatalogFilter {
    pub include_missing: bool,
    pub query: String,
    pub directory: String,
    pub format: Option<String>,
    pub sort: String,
    pub direction: String,
    pub limit: i64,
    pub offset: i64,
}

impl CatalogFilter {
    /// Validates and normalizes public catalog query parameters.
    ///
    /// # Errors
    ///
    /// Returns a user-facing validation message for unsupported values or an
    /// unsafe, non-normalized directory path.
    pub fn from_query(query: &CatalogQuery, limit: i64, offset: i64) -> Result<Self, String> {
        let search = query.query.as_deref().unwrap_or("").trim();
        if search.chars().count() > 200 {
            return Err("query must not exceed 200 characters".to_owned());
        }
        let directory = query.directory.as_deref().unwrap_or("").trim_matches('/');
        if directory.len() > 1024
            || directory.contains('\\')
            || directory
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
                && !directory.is_empty()
        {
            return Err("directory must be a normalized relative path".to_owned());
        }
        let format = query.format.as_deref().filter(|value| !value.is_empty());
        if format.is_some_and(|value| !FORMATS.contains(&value)) {
            return Err("unsupported format filter".to_owned());
        }
        let sort = query.sort.as_deref().unwrap_or("path");
        if !SORT_KEYS.contains(&sort) {
            return Err("sort must be path, format, size, or modified".to_owned());
        }
        let direction = query.direction.as_deref().unwrap_or("asc");
        if !matches!(direction, "asc" | "desc") {
            return Err("direction must be asc or desc".to_owned());
        }
        Ok(Self {
            include_missing: query.include_missing.unwrap_or(false),
            query: search.to_owned(),
            directory: directory.to_owned(),
            format: format.map(str::to_owned),
            sort: sort.to_owned(),
            direction: direction.to_owned(),
            limit,
            offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_filters_are_normalized_and_bounded() {
        let query = CatalogQuery {
            query: Some("  frame  ".to_owned()),
            directory: Some("/printers/voron/".to_owned()),
            format: Some("step".to_owned()),
            sort: Some("modified".to_owned()),
            direction: Some("desc".to_owned()),
            ..CatalogQuery::default()
        };
        let filter = CatalogFilter::from_query(&query, 50, 10).expect("valid filter");
        assert_eq!(filter.query, "frame");
        assert_eq!(filter.directory, "printers/voron");
        assert_eq!(filter.format.as_deref(), Some("step"));
        assert_eq!(filter.sort, "modified");
        assert_eq!(filter.direction, "desc");
    }

    #[test]
    fn catalog_filters_reject_paths_and_unbounded_values() {
        let invalid = ["../secret", "parts//frame", "parts\\frame"];
        for directory in invalid {
            let query = CatalogQuery {
                directory: Some(directory.to_owned()),
                ..CatalogQuery::default()
            };
            assert!(CatalogFilter::from_query(&query, 50, 0).is_err());
        }
        let query = CatalogQuery {
            format: Some("exe".to_owned()),
            ..CatalogQuery::default()
        };
        assert!(CatalogFilter::from_query(&query, 50, 0).is_err());
        let query = CatalogQuery {
            sort: Some("random()".to_owned()),
            ..CatalogQuery::default()
        };
        assert!(CatalogFilter::from_query(&query, 50, 0).is_err());
        let query = CatalogQuery {
            direction: Some("sideways".to_owned()),
            ..CatalogQuery::default()
        };
        assert!(CatalogFilter::from_query(&query, 50, 0).is_err());
        let query = CatalogQuery {
            query: Some("x".repeat(201)),
            ..CatalogQuery::default()
        };
        assert!(CatalogFilter::from_query(&query, 50, 0).is_err());
    }
}
