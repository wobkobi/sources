#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Manga, MangaPageResult,
	MangaStatus, Page, PageContent, Result, Source, Viewer,
	alloc::{String, Vec},
	imports::{defaults::defaults_get, net::Request},
	prelude::*,
};
use chrono::DateTime;
use serde::de::DeserializeOwned;

mod models;
use models::*;

const BASE_URL: &str = "https://tapas.io";
const API_URL: &str = "https://story-api.tapas.io";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
	AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

struct Tapas;

fn request(url: String) -> Result<Request> {
	Ok(Request::get(url)?
		.header("User-Agent", USER_AGENT)
		.header("Referer", "https://m.tapas.io"))
}

fn fetch_json<T: DeserializeOwned>(url: String) -> Result<T> {
	request(url)?.send()?.get_json::<T>()
}

impl Source for Tapas {
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
		if query.is_empty() {
			let url = format!(
				"{API_URL}/cosmos/api/v1/landing/ranking?category_type=COMIC&subtab_id=17&size=25&page={}",
				page - 1
			);
			let response: RankingResponse = fetch_json(url)?;
			let has_next_page = !response
				.meta
				.and_then(|m| m.pagination)
				.map(|p| p.last)
				.unwrap_or(true);
			let entries = response
				.data
				.items
				.into_iter()
				.map(|item| Manga {
					key: format!("/series/{}", item.series_id),
					cover: item.cover(),
					title: item.title,
					description: Some(item.description).filter(|d| !d.is_empty()),
					..Default::default()
				})
				.collect();
			return Ok(MangaPageResult {
				entries,
				has_next_page,
			});
		}

		let encoded = query.replace(' ', "+");
		let url = format!("{BASE_URL}/search?pageNumber={page}&q={encoded}&t=COMICS");
		let html = request(url)?.html()?;
		let entries = html
			.select(".search-item-wrap")
			.map(|els| {
				els.filter_map(|el| {
					let link = el.select_first(".item__thumb a, .title-section .title a")?;
					let id = link.attr("data-series-id")?;
					let title = el
						.select_first(".item__thumb img")
						.and_then(|img| img.attr("alt"))
						.or_else(|| {
							el.select_first(".title-section .title a")
								.and_then(|a| a.text())
						})
						.unwrap_or_default();
					Some(Manga {
						key: format!("/series/{id}"),
						title,
						cover: el
							.select_first(".item__thumb img, .thumb-wrap img")
							.and_then(|img| img.attr("abs:src")),
						..Default::default()
					})
				})
				.collect::<Vec<_>>()
			})
			.unwrap_or_default();

		Ok(MangaPageResult {
			entries,
			has_next_page: html
				.select_first("a[class*=paging__button--next]")
				.is_some(),
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details {
			let url = format!("{BASE_URL}{}/info", manga.key);
			let html = request(url)?.html()?;
			manga.url = Some(format!("{BASE_URL}{}", manga.key));
			manga.title = html
				.select_first(".info__right .title")
				.and_then(|el| el.text())
				.unwrap_or(manga.title);
			manga.cover = html
				.select_first(".thumb.js-thumbnail img")
				.and_then(|img| img.attr("abs:src"))
				.or(manga.cover.take());
			manga.description = html
				.select_first(".description__body")
				.and_then(|el| el.text());
			manga.authors = html
				.select(".creator-section .name")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.tags = html
				.select(".genre-btn")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.status = html
				.select_first(".schedule-label")
				.and_then(|el| el.text())
				.map(|s| {
					let s = s.to_lowercase();
					if s.contains("completed") {
						MangaStatus::Completed
					} else if s.contains("update") {
						MangaStatus::Ongoing
					} else {
						MangaStatus::Unknown
					}
				})
				.unwrap_or(MangaStatus::Unknown);
			manga.viewer = Viewer::Webtoon;
			manga.content_rating = ContentRating::Safe;
		}

		if needs_chapters {
			manga.chapters = Some(self.fetch_chapters(&manga.key)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{BASE_URL}{}", chapter.key);
		let html = request(url)?.html()?;
		let pages = html
			.select("img.content__img")
			.map(|imgs| {
				imgs.filter_map(|img| {
					let page_url = img.attr("abs:data-src").or_else(|| img.attr("abs:src"))?;
					Some(Page {
						content: PageContent::Url(page_url, None),
						..Default::default()
					})
				})
				.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		Ok(pages)
	}
}

impl Tapas {
	/// The episode list is a paginated JSON API; collect every page and keep the
	/// freely readable episodes (skip paywalled and unpublished/scheduled ones).
	fn fetch_chapters(&self, series_key: &str) -> Result<Vec<Chapter>> {
		let show_locked = defaults_get::<bool>("showLocked").unwrap_or(false);
		let show_scheduled = defaults_get::<bool>("showScheduled").unwrap_or(false);
		let mut chapters: Vec<Chapter> = Vec::new();
		let mut page = 1;
		loop {
			let url = format!(
				"{BASE_URL}{series_key}/episodes?page={page}&sort=NEWEST&since=0&large=true&last_access=0"
			);
			let response: ChaptersResponse = fetch_json(url)?;
			for ep in &response.data.episodes {
				let readable = ep.unlocked || ep.free;
				if (ep.scheduled && !show_scheduled) || (!readable && !show_locked) {
					continue;
				}
				// Mark paywalled episodes so they're distinguishable in the list.
				let title = if readable {
					ep.title.clone()
				} else {
					format!("🔒 {}", ep.title)
				};
				chapters.push(Chapter {
					key: format!("/episode/{}", ep.id),
					title: Some(title).filter(|t| !t.is_empty()),
					chapter_number: Some(ep.scene),
					date_uploaded: ep
						.publish_date
						.as_deref()
						.and_then(|d| DateTime::parse_from_rfc3339(d).ok())
						.map(|d| d.timestamp()),
					url: Some(format!("{BASE_URL}/episode/{}", ep.id)),
					..Default::default()
				});
			}
			if !response.data.pagination.has_next || page >= 100 {
				break;
			}
			page += 1;
		}
		Ok(chapters)
	}
}

impl DeepLinkHandler for Tapas {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		if let Some(rest) = path.strip_prefix("/series/") {
			let id = rest.split(['/', '?']).next().unwrap_or(rest);
			return Ok(Some(DeepLinkResult::Manga {
				key: format!("/series/{id}"),
			}));
		}
		if path.starts_with("/episode/") {
			return Ok(Some(DeepLinkResult::Chapter {
				manga_key: String::new(),
				key: path.into(),
			}));
		}
		Ok(None)
	}
}

register_source!(Tapas, DeepLinkHandler);

#[cfg(test)]
mod test {
	use super::Tapas;
	use aidoku::{Source, alloc::string::ToString, alloc::vec};
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn search_details_chapters_test() {
		let source = Tapas;

		let results = source
			.get_search_manga_list(Some("love".to_string()), 1, vec![])
			.expect("search failed");
		assert!(!results.entries.is_empty(), "search returned no entries");

		let entry = results.entries.into_iter().next().unwrap();
		let manga = source
			.get_manga_update(entry, true, true)
			.expect("manga update failed");
		assert!(!manga.title.is_empty(), "empty title");
		let chapters = manga.chapters.expect("no chapters field");
		assert!(!chapters.is_empty(), "no chapters listed");
	}
}
