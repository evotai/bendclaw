use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

#[derive(Deserialize, Default)]
pub struct ListQuery {
    #[serde(default, deserialize_with = "de_opt_u32")]
    pub limit: Option<u32>,
    #[serde(default, deserialize_with = "de_opt_u32")]
    pub page: Option<u32>,
    pub sort_order: Option<SortOrder>,
}

impl ListQuery {
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(50).min(200)
    }
    pub fn offset(&self) -> u32 {
        let page = self.page.unwrap_or(1).max(1);
        (page - 1) * self.limit()
    }
    pub fn order(&self) -> &str {
        match self.sort_order {
            Some(SortOrder::Asc) => "ASC",
            _ => "DESC",
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Serialize)]
pub struct Paginated<T: Serialize> {
    pub data: Vec<T>,
    pub meta: PaginationMeta,
}

#[derive(Serialize)]
pub struct PaginationMeta {
    pub page: u32,
    pub limit: u32,
    pub total_count: u64,
    pub total_pages: u64,
}

impl<T: Serialize> Paginated<T> {
    pub fn new(data: Vec<T>, q: &ListQuery, total_count: u64) -> Self {
        let limit = q.limit() as u64;
        let total_pages = if limit > 0 {
            total_count.div_ceil(limit)
        } else {
            0
        };
        Self {
            data,
            meta: PaginationMeta {
                page: q.page.unwrap_or(1).max(1),
                limit: q.limit(),
                total_count,
                total_pages,
            },
        }
    }
}

pub async fn count_u64(pool: &crate::storage::pool::Pool, sql_str: &str) -> u64 {
    pool.query_row(sql_str)
        .await
        .ok()
        .flatten()
        .and_then(|r| crate::storage::sql::col(&r, 0).parse().ok())
        .unwrap_or(0)
}

fn de_opt_u32<'de, D>(deserializer: D) -> std::result::Result<Option<u32>, D::Error>
where D: Deserializer<'de> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrString {
        Num(u32),
        Str(String),
    }
    let value = Option::<NumOrString>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(NumOrString::Num(v)) => Ok(Some(v)),
        Some(NumOrString::Str(s)) => s.parse::<u32>().map(Some).map_err(serde::de::Error::custom),
    }
}
