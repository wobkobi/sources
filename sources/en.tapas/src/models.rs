use aidoku::alloc::{String, Vec};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Deserialize)]
pub struct RankingResponse {
	pub data: WrapperContent,
	pub meta: Option<Meta>,
}

#[derive(Deserialize)]
pub struct WrapperContent {
	#[serde(default)]
	pub items: Vec<MangaDto>,
}

#[derive(Deserialize)]
pub struct Meta {
	pub pagination: Option<Pagination>,
}

#[derive(Deserialize, Default)]
pub struct Pagination {
	#[serde(default = "default_true")]
	pub last: bool,
	#[serde(default, rename = "has_next")]
	pub has_next: bool,
}

fn default_true() -> bool {
	true
}

#[derive(Deserialize)]
pub struct MangaDto {
	#[serde(rename = "seriesId")]
	pub series_id: i64,
	pub title: String,
	#[serde(default)]
	pub description: String,
	#[serde(rename = "assetProperty")]
	pub asset_property: Field,
}

#[derive(Deserialize)]
pub struct Field {
	#[serde(rename = "bookCoverImage", default)]
	pub book_cover_image: Map<String, Value>,
}

impl MangaDto {
	pub fn cover(&self) -> Option<String> {
		self.asset_property
			.book_cover_image
			.values()
			.find_map(|v| v.as_str())
			.map(|s| aidoku::alloc::format!("{s}.png"))
	}
}

#[derive(Deserialize)]
pub struct ChaptersResponse {
	pub data: ChapterListData,
}

#[derive(Deserialize)]
pub struct ChapterListData {
	#[serde(default)]
	pub pagination: Pagination,
	#[serde(default)]
	pub episodes: Vec<EpisodeDto>,
}

#[derive(Deserialize)]
pub struct EpisodeDto {
	pub id: i64,
	pub title: String,
	#[serde(rename = "publish_date")]
	pub publish_date: Option<String>,
	#[serde(default)]
	pub unlocked: bool,
	#[serde(default)]
	pub free: bool,
	#[serde(default)]
	pub scene: f32,
	#[serde(default)]
	pub scheduled: bool,
}
