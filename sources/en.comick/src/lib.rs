#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Manga, MangaPageResult,
	MangaStatus, Page, PageContent, Result, Source, UpdateStrategy, Viewer,
	alloc::{String, Vec},
	imports::{net::Request, std::send_partial_result},
	prelude::*,
};
use chrono::DateTime;
use serde::de::DeserializeOwned;

mod models;
use models::*;

// Comick separates its public JSON API, website and image CDN across hosts, and
// has historically migrated the API/website domains (.fun > .io > .dev).
const API_URL: &str = "https://api.comick.dev";
const WEB_URL: &str = "https://comick.io";
const IMAGE_URL: &str = "https://meo.comick.pictures";
const LANGUAGE: &str = "en";
const SEARCH_LIMIT: i32 = 30;
const CHAPTER_LIMIT: i32 = 100;
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
	AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

struct Comick;

fn fetch_json<T: DeserializeOwned>(url: String) -> Result<T> {
	Request::get(url)?
		.header("User-Agent", USER_AGENT)
		// The API serves Brotli-compressed responses by default; request an
		// uncompressed body so the response is parseable plain JSON.
		.header("Accept-Encoding", "identity")
		.send()?
		.get_json::<T>()
}

fn cover_url(covers: &[MdCover]) -> Option<String> {
	covers
		.iter()
		.find_map(|c| c.b2key.as_deref())
		.map(|key| format!("{IMAGE_URL}/{key}"))
}

fn parse_number(value: &Option<String>) -> Option<f32> {
	value.as_ref().and_then(|v| v.parse::<f32>().ok())
}

impl Source for Comick {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let query = query.unwrap_or_default();
		let url = if query.is_empty() {
			format!(
				"{API_URL}/v1.0/search?sort=follow&page={page}&limit={SEARCH_LIMIT}&tachiyomi=true"
			)
		} else {
			let encoded = query.replace(' ', "%20");
			format!(
				"{API_URL}/v1.0/search?q={encoded}&page={page}&limit={SEARCH_LIMIT}&tachiyomi=true"
			)
		};

		let comics: Vec<SearchComic> = fetch_json(url)?;
		let has_next_page = comics.len() as i32 == SEARCH_LIMIT;
		let entries = comics
			.into_iter()
			.map(|c| Manga {
				key: c.slug,
				title: c.title,
				cover: cover_url(&c.md_covers),
				..Default::default()
			})
			.collect();

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let detail: ComicDetailResponse =
			fetch_json(format!("{API_URL}/comic/{}?tachiyomi=true", manga.key))?;
		let comic = &detail.comic;
		let slug = comic.slug.clone().unwrap_or_else(|| manga.key.clone());

		if needs_details {
			manga.title = comic.title.clone();
			manga.cover = cover_url(&comic.md_covers).or(manga.cover.take());
			manga.url = Some(format!("{WEB_URL}/comic/{slug}"));
			manga.description = comic.desc.clone();
			manga.authors = Some(
				detail
					.authors
					.iter()
					.filter_map(|a| a.name.clone())
					.collect(),
			);
			manga.artists = Some(
				detail
					.artists
					.iter()
					.filter_map(|a| a.name.clone())
					.collect(),
			);
			manga.tags = Some(
				comic
					.md_comic_md_genres
					.iter()
					.filter_map(|g| g.md_genres.as_ref().and_then(|x| x.name.clone()))
					.collect(),
			);
			manga.status = match comic.status {
				Some(1) => MangaStatus::Ongoing,
				Some(2) => MangaStatus::Completed,
				Some(3) => MangaStatus::Cancelled,
				Some(4) => MangaStatus::Hiatus,
				_ => MangaStatus::Unknown,
			};
			manga.content_rating = match comic.content_rating.as_deref() {
				Some("suggestive") => ContentRating::Suggestive,
				Some("erotica") => ContentRating::NSFW,
				_ => ContentRating::Safe,
			};
			manga.viewer = match comic.country.as_deref() {
				Some("jp") => Viewer::RightToLeft,
				Some("kr") | Some("cn") => Viewer::Webtoon,
				_ => Viewer::Unknown,
			};
			manga.update_strategy = match manga.status {
				MangaStatus::Completed | MangaStatus::Cancelled => UpdateStrategy::Never,
				_ => UpdateStrategy::Always,
			};

			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			manga.chapters = Some(self.fetch_chapters(&comic.hid, &slug)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let detail: ChapterDetailResponse =
			fetch_json(format!("{API_URL}/chapter/{}?tachiyomi=true", chapter.key))?;
		let pages = detail
			.chapter
			.md_images
			.into_iter()
			.filter_map(|img| img.b2key)
			.map(|key| Page {
				content: PageContent::Url(format!("{IMAGE_URL}/{key}"), None),
				..Default::default()
			})
			.collect();
		Ok(pages)
	}
}

impl Comick {
	/// Paginate the chapter list endpoint (one language) until every chapter has
	/// been collected. Comick lists each scanlation group's upload separately.
	fn fetch_chapters(&self, comic_hid: &str, slug: &str) -> Result<Vec<Chapter>> {
		let mut chapters: Vec<Chapter> = Vec::new();
		let mut page = 1;
		loop {
			let response: ChaptersResponse = fetch_json(format!(
				"{API_URL}/comic/{comic_hid}/chapters?lang={LANGUAGE}&page={page}&limit={CHAPTER_LIMIT}&tachiyomi=true"
			))?;
			let total = response.total.unwrap_or(0);
			let count = response.chapters.len();
			if count == 0 {
				break;
			}

			for item in response.chapters {
				let scanlators: Vec<String> = item
					.group_name
					.into_iter()
					.filter(|g| !g.is_empty())
					.collect();
				chapters.push(Chapter {
					key: item.hid.clone(),
					title: item.title.filter(|t| !t.is_empty()),
					chapter_number: parse_number(&item.chap),
					volume_number: parse_number(&item.vol),
					date_uploaded: item
						.created_at
						.as_deref()
						.and_then(|d| DateTime::parse_from_rfc3339(d).ok())
						.map(|d| d.timestamp()),
					scanlators: if scanlators.is_empty() {
						None
					} else {
						Some(scanlators)
					},
					url: Some(format!("{WEB_URL}/comic/{slug}/{}", item.hid)),
					language: Some(LANGUAGE.into()),
					..Default::default()
				});
			}

			if chapters.len() as i32 >= total || page >= 50 {
				break;
			}
			page += 1;
		}
		Ok(chapters)
	}
}

impl DeepLinkHandler for Comick {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		// e.g. https://comick.io/comic/<slug> or .../comic/<slug>/<chapterHid>-chapter-...
		let Some(rest) = url.split("/comic/").nth(1) else {
			return Ok(None);
		};
		let mut parts = rest.split('/');
		let Some(slug) = parts.next().filter(|s| !s.is_empty()) else {
			return Ok(None);
		};
		if let Some(chapter_seg) = parts.next().filter(|s| !s.is_empty()) {
			let hid = chapter_seg.split('-').next().unwrap_or(chapter_seg);
			return Ok(Some(DeepLinkResult::Chapter {
				manga_key: slug.into(),
				key: hid.into(),
			}));
		}
		Ok(Some(DeepLinkResult::Manga { key: slug.into() }))
	}
}

register_source!(Comick, DeepLinkHandler);
