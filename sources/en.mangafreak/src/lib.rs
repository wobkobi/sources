#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Manga, MangaPageResult,
	MangaStatus, Page, PageContent, Result, Source, UpdateStrategy, Viewer,
	alloc::{String, Vec, vec},
	imports::{html::Element, net::Request},
	prelude::*,
};
use chrono::NaiveDate;

const BASE_URL: &str = "https://ww2.mangafreak.me";

struct Mangafreak;

fn request(url: String) -> Result<Request> {
	Ok(Request::get(url)?.header("Referer", &format!("{BASE_URL}/")))
}

fn parse_chapter_number(name: &str) -> Option<f32> {
	let digits: String = name
		.chars()
		.skip_while(|c| !c.is_ascii_digit())
		.take_while(|c| c.is_ascii_digit() || *c == '.')
		.collect();
	digits.parse::<f32>().ok()
}

fn parse_date(text: &str) -> Option<i64> {
	NaiveDate::parse_from_str(text.trim(), "%Y/%m/%d")
		.ok()
		.and_then(|d| d.and_hms_opt(0, 0, 0))
		.map(|dt| dt.and_utc().timestamp())
}

fn parse_entry(element: &Element, link_selector: &str) -> Option<Manga> {
	let link = element.select_first(link_selector)?;
	Some(Manga {
		key: link.attr("href")?,
		title: link.text().unwrap_or_default(),
		cover: element
			.select_first("img")
			.and_then(|img| img.attr("abs:src")),
		..Default::default()
	})
}

impl Source for Mangafreak {
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
			// No text query: browse the full genre ranking list (paginated).
			let html = request(format!("{BASE_URL}/Genre/All/{page}"))?.html()?;
			let entries = html
				.select("div.ranking_item")
				.map(|els| {
					els.filter_map(|el| parse_entry(&el, "a"))
						.collect::<Vec<_>>()
				})
				.unwrap_or_default();
			return Ok(MangaPageResult {
				entries,
				has_next_page: html.select_first("a.next_p").is_some(),
			});
		}

		let encoded = query.replace(' ', "_");
		let html = request(format!("{BASE_URL}/Find/{encoded}"))?.html()?;
		let entries = html
			.select("div.manga_search_item, div.mangaka_search_item")
			.map(|els| {
				els.filter_map(|el| parse_entry(&el, "h3 a, h5 a"))
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();

		Ok(MangaPageResult {
			entries,
			has_next_page: false,
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
				.select_first("div.manga_series_data h5")
				.and_then(|el| el.text())
				.unwrap_or(manga.title);
			manga.cover = html
				.select_first("div.manga_series_image img")
				.and_then(|img| img.attr("abs:src"))
				.or(manga.cover.take());

			// The data block lists value divs in a fixed order:
			// [0] status flag, [1] released, [2] status, [3] author, [4] artist.
			let data: Vec<String> = html
				.select("div.manga_series_data > div")
				.map(|els| els.map(|el| el.text().unwrap_or_default()).collect())
				.unwrap_or_default();
			manga.status = match data.get(2).map(|s| s.trim()) {
				Some("ON-GOING") => MangaStatus::Ongoing,
				Some("COMPLETED") => MangaStatus::Completed,
				_ => MangaStatus::Unknown,
			};
			manga.authors = data
				.get(3)
				.filter(|s| !s.trim().is_empty())
				.map(|s| vec![s.trim().into()]);
			manga.artists = data
				.get(4)
				.filter(|s| !s.trim().is_empty())
				.map(|s| vec![s.trim().into()]);
			manga.tags = html
				.select("div.series_sub_genre_list a")
				.map(|els| els.filter_map(|el| el.text()).collect());
			manga.description = html
				.select_first("div.manga_series_description p")
				.and_then(|el| el.text());
			manga.update_strategy = match manga.status {
				MangaStatus::Completed | MangaStatus::Cancelled => UpdateStrategy::Never,
				_ => UpdateStrategy::Always,
			};
			manga.viewer = Viewer::RightToLeft;
			manga.content_rating = ContentRating::Safe;
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = html
				.select("div.manga_series_list tr:has(a)")
				.map(|els| {
					els.filter_map(|el| {
						let link = el.select_first("a")?;
						let cells: Vec<String> = el
							.select("td")
							.map(|tds| tds.map(|td| td.text().unwrap_or_default()).collect())
							.unwrap_or_default();
						let name = cells.first().cloned().unwrap_or_default();
						Some(Chapter {
							key: link.attr("href")?,
							chapter_number: parse_chapter_number(&name),
							title: Some(name).filter(|t| !t.is_empty()),
							date_uploaded: cells.get(1).and_then(|d| parse_date(d)),
							url: link.attr("abs:href"),
							..Default::default()
						})
					})
					.collect()
				})
				.unwrap_or_default();
			// The list is oldest-last on the page; present newest first.
			chapters.reverse();
			manga.chapters = Some(chapters);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let url = format!("{BASE_URL}{}", chapter.key);
		let html = request(url)?.html()?;
		let pages = html
			.select("img#gohere[src]")
			.map(|imgs| {
				imgs.filter_map(|img| {
					let page_url = img.attr("abs:src")?;
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

impl DeepLinkHandler for Mangafreak {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		if path.starts_with("/Manga/") {
			Ok(Some(DeepLinkResult::Manga { key: path.into() }))
		} else if path.starts_with("/Read") {
			Ok(Some(DeepLinkResult::Chapter {
				manga_key: String::new(),
				key: path.into(),
			}))
		} else {
			Ok(None)
		}
	}
}

register_source!(Mangafreak, DeepLinkHandler);

#[cfg(test)]
mod test {
	use super::Mangafreak;
	use aidoku::{Source, alloc::string::ToString, alloc::vec};
	use aidoku_test::aidoku_test;

	#[aidoku_test]
	fn search_details_pages_test() {
		let source = Mangafreak;

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

		let chapter = chapters.into_iter().next().unwrap();
		let pages = source.get_page_list(manga, chapter).expect("pages failed");
		assert!(!pages.is_empty(), "no pages");
	}
}
