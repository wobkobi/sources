#![no_std]
use aidoku::{
	Chapter, ContentRating, DeepLinkHandler, DeepLinkResult, FilterValue, Manga, MangaPageResult,
	MangaStatus, Page, PageContent, Result, Source, UpdateStrategy, Viewer,
	alloc::{String, Vec},
	imports::{html::Element, net::Request},
	prelude::*,
};

const BASE_URL: &str = "https://mangapill.com";

struct MangaPill;

fn request(url: String) -> Result<Request> {
	Ok(Request::get(url)?.header("Referer", &format!("{BASE_URL}/")))
}

/// Extract a chapter number from a name like "Chapter 179.5".
fn parse_chapter_number(name: &str) -> Option<f32> {
	name.split_whitespace()
		.filter_map(|token| token.parse::<f32>().ok())
		.next_back()
}

fn parse_entry(element: &Element) -> Option<Manga> {
	let link = element.select_first("a[href^='/manga/']")?;
	let key = link.attr("href")?;
	let title = element
		.select_first("div.line-clamp-2")
		.and_then(|el| el.text())
		.unwrap_or_default();
	let cover = element
		.select_first("img")
		.and_then(|img| img.attr("data-src").or_else(|| img.attr("abs:src")));
	Some(Manga {
		key,
		title,
		cover,
		..Default::default()
	})
}

impl Source for MangaPill {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let query = query.unwrap_or_default().replace(' ', "+");
		let url = format!("{BASE_URL}/search?page={page}&q={query}");
		let html = request(url)?.html()?;

		let entries = html
			.select(".grid > div:not([class])")
			.map(|els| els.filter_map(|el| parse_entry(&el)).collect::<Vec<_>>())
			.unwrap_or_default();

		Ok(MangaPageResult {
			entries,
			has_next_page: html.select_first("a.btn.btn-sm").is_some(),
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
			manga.cover = html
				.select_first("div.container div:first-child img")
				.and_then(|img| img.attr("data-src").or_else(|| img.attr("abs:src")))
				.or(manga.cover.take());
			manga.description = html
				.select_first("div.container p")
				.and_then(|el| el.text());
			manga.tags = html.select("a[href*=genre]").map(|els| {
				els.filter_map(|el| el.text())
					.filter(|t| !t.is_empty())
					.collect()
			});

			let status_text = html
				.select("div.flex.flex-col span")
				.map(|els| els.filter_map(|el| el.text()).collect::<Vec<_>>().join(" "))
				.unwrap_or_default()
				.to_lowercase();
			manga.status = if status_text.contains("publishing") {
				MangaStatus::Ongoing
			} else if status_text.contains("finished") {
				MangaStatus::Completed
			} else if status_text.contains("on hiatus") {
				MangaStatus::Hiatus
			} else if status_text.contains("discontinued") {
				MangaStatus::Cancelled
			} else {
				MangaStatus::Unknown
			};
			manga.update_strategy = match manga.status {
				MangaStatus::Completed | MangaStatus::Cancelled => UpdateStrategy::Never,
				_ => UpdateStrategy::Always,
			};
			manga.viewer = Viewer::RightToLeft;
			manga.content_rating = ContentRating::Safe;
		}

		if needs_chapters {
			manga.chapters = html.select("#chapters a[href^='/chapters/']").map(|els| {
				els.filter_map(|el| {
					let key = el.attr("href")?;
					let name = el.text().unwrap_or_default();
					Some(Chapter {
						chapter_number: parse_chapter_number(&name),
						key,
						title: Some(name).filter(|t| !t.is_empty()),
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
		let url = format!("{BASE_URL}{}", chapter.key);
		let html = request(url)?.html()?;
		let pages = html
			.select("picture img")
			.map(|imgs| {
				imgs.filter_map(|img| {
					let page_url = img.attr("data-src").or_else(|| img.attr("abs:src"))?;
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

impl DeepLinkHandler for MangaPill {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url.strip_prefix(BASE_URL) else {
			return Ok(None);
		};
		if path.starts_with("/manga/") {
			Ok(Some(DeepLinkResult::Manga { key: path.into() }))
		} else if path.starts_with("/chapters/") {
			Ok(Some(DeepLinkResult::Chapter {
				manga_key: String::new(),
				key: path.into(),
			}))
		} else {
			Ok(None)
		}
	}
}

register_source!(MangaPill, DeepLinkHandler);
