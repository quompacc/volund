use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub include_missing: Option<bool>,
    #[serde(rename = "q")]
    pub query: Option<String>,
    pub directory: Option<String>,
    pub format: Option<String>,
    pub sort: Option<String>,
    pub direction: Option<String>,
}
