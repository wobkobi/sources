#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Manga, MangaPageResult, MangaStatus, Page, PageContent, PageContext, Result, Source, Viewer,
	alloc::{String, Vec},
	imports::{html::Element, net::Request},
	prelude::*,
};
use chrono::NaiveDate;

const BASE_URL: &str = "https://fanfox.net";
const MOBILE_URL: &str = "https://m.fanfox.net";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
	AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

struct FanFox;

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

impl Source for FanFox {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let referer = format!("{BASE_URL}/");
		let query = query.unwrap_or_default();

		let mut included: Vec<String> = Vec::new();
		let mut excluded: Vec<String> = Vec::new();
		let mut type_value = String::new();
		for filter in filters {
			match filter {
				FilterValue::MultiSelect {
					id,
					included: inc,
					excluded: exc,
				} if id == "genre" => {
					included = inc;
					excluded = exc;
				}
				FilterValue::Select { id, value } if id == "type" => type_value = value,
				_ => {}
			}
		}
		let filters_active = !included.is_empty()
			|| !excluded.is_empty()
			|| (!type_value.is_empty() && type_value != "0");

		let (url, selector) = if query.is_empty() && !filters_active {
			let suffix = if page != 1 {
				format!("{page}.html")
			} else {
				String::new()
			};
			(
				format!("{BASE_URL}/directory/{suffix}"),
				"ul.manga-list-1-list li",
			)
		} else {
			let encoded = query.replace(' ', "+");
			let t = if type_value.is_empty() {
				"0"
			} else {
				&type_value
			};
			let genres = included.join(",");
			let nogenres = excluded.join(",");
			(
				format!(
					"{BASE_URL}/search?page={page}&title={encoded}&type={t}&genres={genres}&nogenres={nogenres}&sort=&stype=1"
				),
				"ul.manga-list-4-list li",
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
				.select_first(".pager-list-left a.active + a + a")
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
				.select(".detail-info-right-tag-list a")
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
			manga.chapters = html.select("ul.detail-main-list li a").map(|els| {
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
		// The mobile "roll" page returns every image at once when readway=2 is set.
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

impl ImageRequestProvider for FanFox {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Referer", &format!("{MOBILE_URL}/"))
			.header("User-Agent", USER_AGENT))
	}
}

impl DeepLinkHandler for FanFox {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = strip_domain(&url);
		if !path.starts_with("/manga/") {
			return Ok(None);
		}
		// A chapter url ends in an .html page; a series url is just /manga/<slug>/.
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

register_source!(FanFox, ImageRequestProvider, DeepLinkHandler);

#[cfg(test)]
mod test {
	use super::FanFox;
	use aidoku::{Source, alloc::string::ToString, alloc::vec};
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn search_details_pages_test() {
		let source = FanFox;

		let results = source
			.get_search_manga_list(Some("naruto".to_string()), 1, vec![])
			.expect("search failed");
		assert!(!results.entries.is_empty(), "search returned no entries");

		let entry = results.entries.into_iter().next().unwrap();
		let manga = source
			.get_manga_update(entry, true, true)
			.expect("manga update failed");
		assert!(!manga.title.is_empty(), "empty title");
		let chapters = manga.chapters.clone().expect("no chapters");
		assert!(!chapters.is_empty(), "no chapters listed");

		let chapter = chapters.into_iter().next_back().unwrap();
		let pages = source.get_page_list(manga, chapter).expect("pages failed");
		assert!(!pages.is_empty(), "no pages");
	}
}
