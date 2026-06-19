use aidoku::alloc::{String, Vec};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct MdCover {
	pub b2key: Option<String>,
}

/// An entry as returned by the search/listing endpoints.
#[derive(Deserialize)]
pub struct SearchComic {
	pub slug: String,
	pub title: String,
	#[serde(default)]
	pub md_covers: Vec<MdCover>,
}

#[derive(Deserialize)]
pub struct ComicDetailResponse {
	pub comic: Comic,
	#[serde(default)]
	pub authors: Vec<NameItem>,
	#[serde(default)]
	pub artists: Vec<NameItem>,
}

#[derive(Deserialize)]
pub struct NameItem {
	pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct Comic {
	pub hid: String,
	pub slug: Option<String>,
	pub title: String,
	pub country: Option<String>,
	pub status: Option<i32>,
	pub desc: Option<String>,
	pub content_rating: Option<String>,
	#[serde(default)]
	pub md_covers: Vec<MdCover>,
	#[serde(default)]
	pub md_comic_md_genres: Vec<GenreWrap>,
}

#[derive(Deserialize)]
pub struct GenreWrap {
	pub md_genres: Option<Genre>,
}

#[derive(Deserialize)]
pub struct Genre {
	pub name: Option<String>,
}

#[derive(Deserialize)]
pub struct ChaptersResponse {
	#[serde(default)]
	pub chapters: Vec<ChapterItem>,
	pub total: Option<i32>,
}

#[derive(Deserialize)]
pub struct ChapterItem {
	pub hid: String,
	pub chap: Option<String>,
	pub vol: Option<String>,
	pub title: Option<String>,
	#[serde(default)]
	pub group_name: Vec<String>,
	pub created_at: Option<String>,
}

#[derive(Deserialize)]
pub struct ChapterDetailResponse {
	pub chapter: ChapterDetail,
}

#[derive(Deserialize)]
pub struct ChapterDetail {
	#[serde(default)]
	pub md_images: Vec<MdImage>,
}

#[derive(Deserialize)]
pub struct MdImage {
	pub b2key: Option<String>,
}
