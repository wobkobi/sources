#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, PageContext, Result, Source, Viewer,
	alloc::{String, Vec},
	imports::{html::Element, net::Request},
	prelude::*,
};
use chrono::NaiveDate;

const BASE_URL: &str = "https://www.mangahere.cc";
const MOBILE_URL: &str = "https://m.mangahere.cc";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
	AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

struct MangaHere;

fn request(url: String, referer: &str, cookie: &str) -> Result<Request> {
	Ok(Request::get(url)?
		.header("User-Agent", USER_AGENT)
		.header("Referer", referer)
		.header("Cookie", cookie))
}

fn strip_domain(url: &str) -> String {
	url.strip_prefix(BASE_URL)
		.or_else(|| url.strip_prefix(MOBILE_URL))
		.unwrap_or(url)
		.into()
}

fn parse_date(text: &str) -> Option<i64> {
	NaiveDate::parse_from_str(text.trim(), "%b %d,%Y")
		.ok()
		.and_then(|d| d.and_hms_opt(0, 0, 0))
		.map(|dt| dt.and_utc().timestamp())
}

fn parse_entry(element: &Element) -> Option<Manga> {
	let link = element.select_first("a")?;
	Some(Manga {
		key: strip_domain(&link.attr("abs:href")?),
		title: link.attr("title").unwrap_or_default(),
		cover: link.select_first("img").and_then(|img| img.attr("abs:src")),
		..Default::default()
	})
}

impl Source for MangaHere {
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
		let (url, selector) = if query.is_empty() {
			(
				format!("{BASE_URL}/directory/{page}.htm"),
				".manga-list-1-list li",
			)
		} else {
			let encoded = query.replace(' ', "+");
			(
				format!("{BASE_URL}/search?page={page}&title={encoded}"),
				".manga-list-4-list > li",
			)
		};

		let html = request(url, &referer, "isAdult=1")?.html()?;
		let entries = html
			.select(selector)
			.map(|els| els.filter_map(|el| parse_entry(&el)).collect())
			.unwrap_or_default();

		Ok(MangaPageResult {
			entries,
			has_next_page: html
				.select_first("div.pager-list-left a.active + a")
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
		let html = request(url.clone(), &format!("{BASE_URL}/"), "isAdult=1")?.html()?;

		if needs_details {
			manga.url = Some(url);
			manga.cover = html
				.select_first(".detail-info-cover-img")
				.and_then(|img| img.attr("abs:src"))
				.or(manga.cover.take());
			manga.authors = html
				.select(".detail-info-right-say a")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.tags = html
				.select(".detail-info-right-tag-list > a")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.description = html.select_first("p.fullcontent").and_then(|el| el.text());
			manga.status = html
				.select_first(".detail-info-right-title-tip")
				.and_then(|el| el.text())
				.map(|s| match s.to_lowercase().as_str() {
					t if t.contains("ongoing") => MangaStatus::Ongoing,
					t if t.contains("completed") => MangaStatus::Completed,
					_ => MangaStatus::Unknown,
				})
				.unwrap_or(MangaStatus::Unknown);
			manga.viewer = Viewer::RightToLeft;
			manga.content_rating = ContentRating::Safe;
		}

		if needs_chapters {
			manga.chapters = html.select("ul.detail-main-list > li a").map(|els| {
				els.filter_map(|el| {
					let texts: Vec<String> = el
						.select(".detail-main-list-main p")
						.map(|ps| ps.map(|p| p.text().unwrap_or_default()).collect())
						.unwrap_or_default();
					let name = texts.first().cloned().unwrap_or_default();
					Some(Chapter {
						key: strip_domain(&el.attr("abs:href")?),
						title: Some(name).filter(|t| !t.is_empty()),
						date_uploaded: texts.last().and_then(|d| parse_date(d)),
						url: el.attr("abs:href"),
						..Default::default()
					})
				})
				.collect()
			});
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// The mobile "roll" page returns every image at once when readway=2 is set,
		// avoiding the packed-JS image cipher used on the desktop reader.
		let mobile_path = chapter.key.replace("/manga/", "/roll_manga/");
		let html = request(
			format!("{MOBILE_URL}{mobile_path}"),
			&format!("{MOBILE_URL}/"),
			"isAdult=1; readway=2",
		)?
		.html()?;
		let pages = html
			.select("#viewer img")
			.map(|imgs| {
				imgs.filter_map(|img| {
					let page_url = img
						.attr("abs:data-original")
						.or_else(|| img.attr("abs:src"))?;
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

impl ImageRequestProvider for MangaHere {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{MOBILE_URL}/"))
			.header("User-Agent", USER_AGENT))
	}
}

impl DeepLinkHandler for MangaHere {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = strip_domain(&url);
		if !path.starts_with("/manga/") {
			return Ok(None);
		}
		if path.ends_with(".html") {
			Ok(Some(DeepLinkResult::Chapter {
				manga_key: String::new(),
				key: path,
			}))
		} else {
			Ok(Some(DeepLinkResult::Manga { key: path }))
		}
	}
}

register_source!(MangaHere, ImageRequestProvider, DeepLinkHandler);
