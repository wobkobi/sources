#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Manga, MangaPageResult,
	MangaStatus, Page, PageContent, Result, Source, UpdateStrategy, Viewer,
	alloc::{String, Vec, vec},
	imports::{defaults::defaults_get, html::Document, html::Element, net::Request},
	prelude::*,
};
use chrono::NaiveDate;

const BASE_URL: &str = "https://mangakatana.com";

struct MangaKatana;

fn request(url: String) -> Result<Request> {
	Ok(Request::get(url)?.header("Referer", &format!("{BASE_URL}/")))
}

fn strip_domain(url: &str) -> String {
	url.strip_prefix(BASE_URL).unwrap_or(url).into()
}

/// Pull a chapter number out of a name like "Chapter 10.5" or "Vol.2 Chapter 7".
fn parse_chapter_number(name: &str) -> Option<f32> {
	let lower = name.to_lowercase();
	let after = lower.split("chapter").last().unwrap_or(&lower);
	let digits: String = after
		.trim_start_matches(|c: char| !c.is_ascii_digit())
		.chars()
		.take_while(|c| c.is_ascii_digit() || *c == '.')
		.collect();
	digits.parse::<f32>().ok()
}

fn parse_date(text: &str) -> Option<i64> {
	NaiveDate::parse_from_str(text.trim(), "%b-%d-%Y")
		.ok()
		.and_then(|d| d.and_hms_opt(0, 0, 0))
		.map(|dt| dt.and_utc().timestamp())
}

fn parse_list_entry(element: &Element) -> Option<Manga> {
	let link = element.select_first("div.text > h3 > a")?;
	Some(Manga {
		key: strip_domain(&link.attr("abs:href")?),
		title: link.own_text().unwrap_or_default(),
		cover: element
			.select_first("img")
			.and_then(|img| img.attr("abs:src")),
		..Default::default()
	})
}

/// Some searches resolve directly to a single manga's detail page, so build an
/// entry from the detail document in that case.
fn single_result(html: &Document) -> Option<Manga> {
	let title = html.select_first("h1.heading")?.text()?;
	let key = html
		.select_first("link[rel=canonical]")
		.and_then(|el| el.attr("href"))
		.or_else(|| {
			html.select_first("meta[property='og:url']")
				.and_then(|el| el.attr("content"))
		})
		.map(|u| strip_domain(&u))?;
	Some(Manga {
		key,
		title,
		cover: html
			.select_first("div.media div.cover img")
			.and_then(|img| img.attr("abs:src")),
		..Default::default()
	})
}

/// Extract page image urls from the inline script: `var <name>=['url', ...]`,
/// where <name> is the variable passed to `.attr('data-src', <name>[i])`. The
/// site's HTML parser drops script bodies from the DOM, so this runs against the
/// raw page source. Matching `data-src'` (single-quoted) targets the script's
/// string literal rather than lazy-load `data-src="..."` image attributes.
fn extract_pages(script: &str) -> Vec<String> {
	let Some(after) = script.split("data-src'").nth(1) else {
		return Vec::new();
	};
	let name: String = after
		.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_')
		.chars()
		.take_while(|c| c.is_alphanumeric() || *c == '_')
		.collect();
	if name.is_empty() {
		return Vec::new();
	}
	let Some(decl) = script.split(&format!("var {name}")).nth(1) else {
		return Vec::new();
	};
	let Some(open) = decl.find('[') else {
		return Vec::new();
	};
	let Some(close) = decl[open..].find(']') else {
		return Vec::new();
	};
	decl[open + 1..open + close]
		.split('\'')
		.filter(|s| s.starts_with("http"))
		.map(String::from)
		.collect()
}

impl Source for MangaKatana {
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
			format!("{BASE_URL}/manga/page/{page}")
		} else {
			let encoded = query.replace(' ', "+");
			format!("{BASE_URL}/page/{page}?search={encoded}&search_by=book_name")
		};
		let html = request(url)?.html()?;

		let entries = html
			.select("div#book_list > div.item")
			.map(|els| {
				els.filter_map(|el| parse_list_entry(&el))
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();

		if entries.is_empty()
			&& let Some(manga) = single_result(&html)
		{
			return Ok(MangaPageResult {
				entries: vec![manga],
				has_next_page: false,
			});
		}

		Ok(MangaPageResult {
			entries,
			has_next_page: html.select_first("a.next.page-numbers").is_some(),
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let url = format!("{BASE_URL}{}", manga.key);
		let html = request(url.clone())?.html()?;

		if needs_details {
			manga.url = Some(url);
			manga.title = html
				.select_first("h1.heading")
				.and_then(|el| el.text())
				.unwrap_or(manga.title);
			manga.cover = html
				.select_first("div.media div.cover img")
				.and_then(|img| img.attr("abs:src"))
				.or(manga.cover.take());
			manga.authors = html
				.select(".author")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.description = html.select_first(".summary > p").and_then(|el| el.text());
			manga.tags = html
				.select(".genres > a")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.status = html
				.select_first(".value.status")
				.and_then(|el| el.text())
				.map(|s| match s.to_lowercase().as_str() {
					t if t.contains("ongoing") => MangaStatus::Ongoing,
					t if t.contains("completed") => MangaStatus::Completed,
					_ => MangaStatus::Unknown,
				})
				.unwrap_or(MangaStatus::Unknown);
			manga.update_strategy = match manga.status {
				MangaStatus::Completed | MangaStatus::Cancelled => UpdateStrategy::Never,
				_ => UpdateStrategy::Always,
			};
			manga.viewer = Viewer::RightToLeft;
			manga.content_rating = ContentRating::Safe;
		}

		if needs_chapters {
			manga.chapters = html.select("tr:has(.chapter)").map(|els| {
				els.filter_map(|el| {
					let link = el.select_first("a")?;
					let name = link.text().unwrap_or_default();
					Some(Chapter {
						key: strip_domain(&link.attr("abs:href")?),
						chapter_number: parse_chapter_number(&name),
						title: Some(name).filter(|t| !t.is_empty()),
						date_uploaded: el
							.select_first(".update_time")
							.and_then(|el| el.text())
							.and_then(|d| parse_date(&d)),
						url: el.select_first("a").and_then(|a| a.attr("abs:href")),
						..Default::default()
					})
				})
				.collect()
			});
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// Users can switch the image server in settings (?sv=mk / ?sv=3).
		let server = defaults_get::<String>("server").unwrap_or_default();
		let suffix = if server.is_empty() {
			String::new()
		} else {
			format!("?sv={server}")
		};
		let url = format!("{BASE_URL}{}{suffix}", chapter.key);
		let body = request(url)?.send()?.get_string()?;

		let pages = extract_pages(&body)
			.into_iter()
			.map(|url| Page {
				content: PageContent::Url(url, None),
				..Default::default()
			})
			.collect();
		Ok(pages)
	}
}

impl DeepLinkHandler for MangaKatana {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		// Manga: /manga/<slug>.<id>   Chapter: /manga/<slug>.<id>/c<n>
		if path.starts_with("/manga/") {
			if let Some((manga_path, _)) = path.split_once("/c") {
				return Ok(Some(DeepLinkResult::Chapter {
					manga_key: manga_path.into(),
					key: path.into(),
				}));
			}
			return Ok(Some(DeepLinkResult::Manga { key: path.into() }));
		}
		Ok(None)
	}
}

register_source!(MangaKatana, DeepLinkHandler);

#[cfg(test)]
mod test {
	use super::MangaKatana;
	use aidoku::{Source, alloc::string::ToString, alloc::vec};
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn search_details_pages_test() {
		let source = MangaKatana;

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
