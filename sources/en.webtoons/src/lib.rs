#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, PageContext, Result, Source, Viewer,
	alloc::{String, Vec, vec},
	imports::{defaults::defaults_get, html::Element, net::Request},
	prelude::*,
};
use serde::de::DeserializeOwned;

mod models;
use models::*;

const BASE_URL: &str = "https://www.webtoons.com";
const MOBILE_URL: &str = "https://m.webtoons.com";
const LANG: &str = "en";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
	AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const COOKIE: &str = "ageGatePass=true; locale=en; needGDPR=false";

struct Webtoons;

fn request(url: String, referer: &str) -> Result<Request> {
	Ok(Request::get(url)?
		.header("User-Agent", USER_AGENT)
		.header("Referer", referer)
		.header("Cookie", COOKIE))
}

fn fetch_json<T: DeserializeOwned>(url: String) -> Result<T> {
	request(url, &format!("{MOBILE_URL}/"))?
		.send()?
		.get_json::<T>()
}

fn strip_domain(url: &str) -> String {
	url.strip_prefix(BASE_URL)
		.or_else(|| url.strip_prefix(MOBILE_URL))
		.unwrap_or(url)
		.into()
}

fn query_param(url: &str, key: &str) -> Option<String> {
	url.split(&format!("{key}="))
		.nth(1)
		.map(|s| s.split('&').next().unwrap_or(s).into())
}

fn parse_number(text: &str) -> Option<f32> {
	let digits: String = text
		.chars()
		.skip_while(|c| !c.is_ascii_digit())
		.take_while(|c| c.is_ascii_digit() || *c == '.')
		.collect();
	digits.parse::<f32>().ok()
}

fn manga_from_element(element: &Element) -> Option<Manga> {
	let key = strip_domain(&element.attr("abs:href")?);
	Some(Manga {
		title: element
			.select_first(".title")
			.and_then(|t| t.text())
			.unwrap_or_default(),
		cover: element
			.select_first("img")
			.and_then(|img| img.attr("abs:src")),
		key,
		..Default::default()
	})
}

impl Source for Webtoons {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let referer = format!("{BASE_URL}/");
		let query = query.unwrap_or_default();
		if query.is_empty() {
			let html = request(format!("{BASE_URL}/{LANG}/ranking/trending"), &referer)?.html()?;
			let entries = html
				.select(".webtoon_list li a")
				.map(|els| els.filter_map(|el| manga_from_element(&el)).collect())
				.unwrap_or_default();
			return Ok(MangaPageResult {
				entries,
				has_next_page: false,
			});
		}

		let encoded = query.replace(' ', "+");
		let html = request(
			format!("{BASE_URL}/{LANG}/search?keyword={encoded}&page={page}"),
			&referer,
		)?
		.html()?;
		let entries = html
			.select(".webtoon_list li a")
			.map(|els| els.filter_map(|el| manga_from_element(&el)).collect())
			.unwrap_or_default();

		Ok(MangaPageResult {
			entries,
			has_next_page: html
				.select_first("a.pagination[aria-current=true] + a")
				.is_some(),
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{BASE_URL}{}", manga.key);
		if needs_details {
			let html = request(url.clone(), &format!("{BASE_URL}/"))?.html()?;
			manga.url = Some(url.clone());
			manga.title = html
				.select_first("h1.subj, h3.subj")
				.and_then(|el| el.text())
				.unwrap_or(manga.title);
			manga.cover = html
				.select_first("head meta[property='og:image']")
				.and_then(|el| el.attr("content"))
				.or(manga.cover.take());
			manga.authors = html
				.select_first(".detail_header .info .author")
				.and_then(|el| el.own_text())
				.map(|a| vec![a]);
			manga.tags = html
				.select(".detail_header .info .genre")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.description = html
				.select_first("#_asideDetail p.summary")
				.and_then(|el| el.text());
			manga.status = html
				.select_first("#_asideDetail p.day_info")
				.and_then(|el| el.text())
				.map(|s| {
					let s = s.to_uppercase();
					if s.contains("UP") || s.contains("EVERY") {
						MangaStatus::Ongoing
					} else if s.contains("END") || s.contains("COMPLETED") {
						MangaStatus::Completed
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
		let max_quality = defaults_get::<bool>("maxQuality").unwrap_or(false);
		let url = format!("{BASE_URL}{}", chapter.key);
		let html = request(url, &format!("{BASE_URL}/"))?.html()?;
		let pages = html
			.select("div#_imageList > img")
			.map(|imgs| {
				imgs.filter_map(|img| {
					let mut page_url = img.attr("data-url").or_else(|| img.attr("abs:src"))?;
					// Images default to ?type=q90; dropping it serves full resolution.
					if max_quality && let Some(idx) = page_url.find("?type=q90") {
						page_url.truncate(idx);
					}
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

impl Webtoons {
	fn fetch_chapters(&self, manga_key: &str) -> Result<Vec<Chapter>> {
		let title_id = query_param(manga_key, "title_no")
			.or_else(|| query_param(manga_key, "titleNo"))
			.unwrap_or_default();
		let kind = if manga_key.contains("/canvas/") {
			"canvas"
		} else {
			"webtoon"
		};
		let url = format!("{MOBILE_URL}/api/v1/{kind}/{title_id}/episodes?pageSize=99999");
		let response: EpisodeListResponse = fetch_json(url)?;

		let mut chapters: Vec<Chapter> = response
			.result
			.episode_list
			.into_iter()
			.enumerate()
			.map(|(index, ep)| Chapter {
				key: strip_domain(&ep.viewer_link),
				chapter_number: parse_number(&ep.episode_title).or(Some((index + 1) as f32)),
				title: Some(ep.episode_title).filter(|t| !t.is_empty()),
				date_uploaded: Some(ep.exposure_date_millis / 1000).filter(|d| *d > 0),
				url: Some(format!("{BASE_URL}{}", strip_domain(&ep.viewer_link))),
				..Default::default()
			})
			.collect();
		// API returns oldest-first; present newest-first.
		chapters.reverse();
		Ok(chapters)
	}
}

impl ImageRequestProvider for Webtoons {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		// Webtoon's image CDN rejects requests without a matching Referer.
		Ok(Request::get(url)?
			.header("Referer", &format!("{BASE_URL}/"))
			.header("User-Agent", USER_AGENT))
	}
}

impl DeepLinkHandler for Webtoons {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = strip_domain(&url);
		if path.contains("/viewer") {
			Ok(Some(DeepLinkResult::Chapter {
				manga_key: String::new(),
				key: path,
			}))
		} else if path.contains("title_no=") || path.contains("/list") {
			Ok(Some(DeepLinkResult::Manga { key: path }))
		} else {
			Ok(None)
		}
	}
}

register_source!(Webtoons, ImageRequestProvider, DeepLinkHandler);

#[cfg(test)]
mod test {
	use super::Webtoons;
	use aidoku::{Source, alloc::string::ToString, alloc::vec};
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn search_details_chapters_test() {
		let source = Webtoons;

		let results = source
			.get_search_manga_list(Some("tower of god".to_string()), 1, vec![])
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
