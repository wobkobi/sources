#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Manga, MangaPageResult,
	MangaStatus, MigrationHandler, Page, PageContent, Result, Source, UpdateStrategy, Viewer,
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

/// Turn a site URL or path into a key, swapping the title slug for the literal
/// `id` so `/manga/go-toubun-no-hanayome.18224/c1` becomes `/manga/id.18224/c1`.
/// The site serves both forms, and the numeric id survives title renames that
/// would otherwise orphan library entries.
fn url_to_key(url: &str) -> String {
	let path = url.strip_prefix(BASE_URL).unwrap_or(url);
	let Some(rest) = path.strip_prefix("/manga/") else {
		return path.into();
	};
	let (series, tail) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
	// rsplit so a slug containing dots (`dr.-stone.12345`) still yields the id.
	match series.rsplit_once('.') {
		Some((_, id)) if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) => {
			format!("/manga/id.{id}{}", tail.trim_end_matches('/'))
		}
		_ => path.into(),
	}
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
		key: url_to_key(&link.attr("abs:href")?),
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
		.map(|u| url_to_key(&u))?;
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
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let query = query.unwrap_or_default();
		// Text search ignores filters; the browse endpoint applies them.
		let url = if !query.is_empty() {
			let encoded = query.replace(' ', "+");
			format!("{BASE_URL}/page/{page}?search={encoded}&search_by=book_name")
		} else {
			let mut url = format!("{BASE_URL}/manga/page/{page}?filter=1");
			for filter in filters {
				match filter {
					FilterValue::MultiSelect {
						id,
						included,
						excluded,
					} if id == "genre" => {
						if !included.is_empty() {
							url.push_str(&format!("&include={}", included.join("_")));
						}
						if !excluded.is_empty() {
							url.push_str(&format!("&exclude={}", excluded.join("_")));
						}
					}
					FilterValue::Select { id, value } if !value.is_empty() => {
						url.push_str(&format!("&{id}={value}"));
					}
					_ => {}
				}
			}
			url
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
						key: url_to_key(&link.attr("abs:href")?),
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
		let path = path.split(['?', '#']).next().unwrap_or_default();
		// Manga: /manga/id.<id>   Chapter: /manga/id.<id>/c<n>
		let key = url_to_key(path);
		let Some(rest) = key.strip_prefix("/manga/id.") else {
			return Ok(None);
		};
		Ok(Some(match rest.split_once('/') {
			Some((id, _)) => DeepLinkResult::Chapter {
				manga_key: format!("/manga/id.{id}"),
				key,
			},
			None => DeepLinkResult::Manga { key },
		}))
	}
}

/// Rewrites keys from the legacy en.mangakatana source, which stored the bare
/// numeric id (`18224`) and chapter segment (`c1`), and from v3 of this one,
/// which kept the title slug. Already-migrated keys pass through unchanged.
impl MigrationHandler for MangaKatana {
	fn handle_manga_migration(&self, key: String) -> Result<String> {
		Ok(
			if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
				format!("/manga/id.{key}")
			} else {
				url_to_key(&key)
			},
		)
	}

	/// A legacy chapter key holds only `c<n>`, so the series comes from `manga_key`.
	fn handle_chapter_migration(&self, manga_key: String, chapter_key: String) -> Result<String> {
		if chapter_key.starts_with('/') {
			return Ok(url_to_key(&chapter_key));
		}
		let manga = self.handle_manga_migration(manga_key)?;
		Ok(format!("{manga}/{chapter_key}"))
	}
}

register_source!(MangaKatana, DeepLinkHandler, MigrationHandler);

#[cfg(test)]
mod test {
	use super::MangaKatana;
	use aidoku::{
		DeepLinkHandler, DeepLinkResult, MigrationHandler, Source, alloc::string::ToString,
		alloc::vec,
	};
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn key_migration_test() {
		let source = MangaKatana;
		let migrate = |key: &str| source.handle_manga_migration(key.to_string()).unwrap();
		// Legacy, v3 slug form, slug with a dot, and an already-migrated key.
		assert_eq!(migrate("18224"), "/manga/id.18224");
		assert_eq!(
			migrate("/manga/go-toubun-no-hanayome.18224"),
			"/manga/id.18224"
		);
		assert_eq!(migrate("/manga/dr.-stone.12345"), "/manga/id.12345");
		assert_eq!(migrate("/manga/id.18224"), "/manga/id.18224");

		let chapter = |manga: &str, key: &str| {
			source
				.handle_chapter_migration(manga.to_string(), key.to_string())
				.unwrap()
		};
		assert_eq!(chapter("18224", "c1"), "/manga/id.18224/c1");
		assert_eq!(chapter("18224", "c10.5"), "/manga/id.18224/c10.5");
		assert_eq!(
			chapter(
				"/manga/go-toubun-no-hanayome.18224",
				"/manga/go-toubun-no-hanayome.18224/c1"
			),
			"/manga/id.18224/c1"
		);
		assert_eq!(
			chapter("/manga/id.18224", "/manga/id.18224/c1"),
			"/manga/id.18224/c1"
		);
	}

	#[aidoku_test]
	fn deep_link_test() {
		let source = MangaKatana;
		let link = |url: &str| source.handle_deep_link(url.to_string()).unwrap();
		// A slug starting with "c" must not be mistaken for a chapter segment.
		assert_eq!(
			link("https://mangakatana.com/manga/chainsaw-man.21890"),
			Some(DeepLinkResult::Manga {
				key: "/manga/id.21890".to_string()
			})
		);
		assert_eq!(
			link("https://mangakatana.com/manga/chainsaw-man.21890/c12?sv=mk"),
			Some(DeepLinkResult::Chapter {
				manga_key: "/manga/id.21890".to_string(),
				key: "/manga/id.21890/c12".to_string()
			})
		);
		assert_eq!(link("https://mangakatana.com/latest"), None);
	}

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
