#![no_std]
use aidoku::{
	Chapter, HomeComponent, HomeComponentValue, HomeLayout, Manga, MangaWithChapter, Result,
	Source,
	alloc::Vec,
	helpers::string::StripPrefixOrSelf,
	imports::{
		html::{Document, Element},
		net::Request,
	},
	prelude::*,
};
use madara::{
	Impl, Madara, Params,
	helpers::{ElementImageAttr, find_first_f32, parse_chapter_date},
};

const BASE_URL: &str = "https://manhwatop.com";

/// The site sits behind Cloudflare, so leave the User-Agent to the app: it
/// sends the device's own WebView UA and solves any challenge in a WebView
/// with that same UA. Overriding it (e.g. with a desktop Chrome UA) makes the
/// "Verify you are human" check loop, because the UA no longer matches the
/// WebKit engine running the check.
struct Manhwatop;

impl Impl for Manhwatop {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: BASE_URL.into(),
			// The legacy admin-ajax `manga_get_chapters` action answers "0" here;
			// the site's own reader loads chapters from `{manga}/ajax/chapters`.
			use_new_chapter_endpoint: true,
			// Absolute dates read like "Oct 29, 2026" (short month, unpadded day).
			datetime_format: "MMM d, yyyy".into(),
			// The website does not flag the content type of entries.
			filter_non_manga_items: false,
			..Default::default()
		}
	}

	fn get_home(&self, params: &Params) -> Result<HomeLayout> {
		let html = self
			.modify_request(params, Request::get(BASE_URL)?)?
			.html()?;
		Ok(parse_home(&html, params))
	}
}

/// Build the home layout from the site's customised Madara theme.
///
/// The template's home selectors find nothing here: sections are
/// `.widget-manga-popular-display` widgets (e.g. "New Comics Update", "Top
/// Trending Today") rather than the popular slider/recent widgets, and the
/// latest-updates grid items lack the `manga` class. Sections are emitted in
/// page order.
fn parse_home(html: &Document, params: &Params) -> HomeLayout {
	let mut components = Vec::new();
	let Some(sections) = html.select(".widget-manga-popular-display, .main-col-inner") else {
		return HomeLayout { components };
	};
	for section in sections {
		if section.has_class("main-col-inner") {
			let entries: Vec<MangaWithChapter> = section
				.select(".page-listing-item .page-item-detail")
				.map(|els| els.filter_map(|el| parse_latest(&el, params)).collect())
				.unwrap_or_default();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: section
						.select_first(".c-blog__heading .h4")
						.and_then(|el| el.text()),
					subtitle: None,
					value: HomeComponentValue::MangaChapterList {
						page_size: Some(4),
						entries,
						listing: None,
					},
				});
			}
		} else {
			let entries: Vec<Manga> = section
				.select(".shark-item-wrap > div")
				.map(|els| els.filter_map(|el| parse_widget_item(&el)).collect())
				.unwrap_or_default();
			if !entries.is_empty() {
				components.push(HomeComponent {
					title: section
						.select_first(".c-blog__heading h4 > a")
						.and_then(|el| el.text()),
					subtitle: None,
					value: HomeComponentValue::Scroller {
						entries: entries.into_iter().map(|m| m.into()).collect(),
						listing: None,
					},
				});
			}
		}
	}
	HomeLayout { components }
}

/// A cover card in a popular-display widget.
fn parse_widget_item(el: &Element) -> Option<Manga> {
	let link = el
		.select_first(".popular-content a")
		.or_else(|| el.select_first(".popular-img a"))?;
	let url = link.attr("abs:href")?;
	Some(Manga {
		key: url.strip_prefix_or_self(BASE_URL).into(),
		title: link.attr("title").or_else(|| link.text())?,
		cover: el.select_first("img").and_then(|img| img.img_attr(false)),
		url: Some(url),
		..Default::default()
	})
}

/// A latest-updates grid item with its newest chapter. Keys are built the same
/// way as the template's chapter list so tapping the chapter opens it.
fn parse_latest(el: &Element, params: &Params) -> Option<MangaWithChapter> {
	let link = el.select_first(".post-title a")?;
	let url = link.attr("abs:href")?;
	let chapter_item = el.select_first(".chapter-item")?;
	let chapter_link = chapter_item.select_first("a")?;
	let chapter_url = chapter_link.attr("abs:href")?;
	let chapter_text = chapter_link.text()?;
	Some(MangaWithChapter {
		manga: Manga {
			key: url.strip_prefix_or_self(BASE_URL).into(),
			title: link.text()?,
			cover: el.select_first("img").and_then(|img| img.img_attr(false)),
			url: Some(url),
			..Default::default()
		},
		chapter: Chapter {
			key: chapter_url.strip_prefix_or_self(BASE_URL).into(),
			// Only keep a real name ("Chapter 12 - Title"), not a bare "Chapter 12".
			title: chapter_text
				.split_once('-')
				.map(|(_, title)| title.trim().into()),
			chapter_number: find_first_f32(&chapter_text),
			date_uploaded: chapter_item
				.select_first(".post-on")
				.and_then(|el| el.text())
				.map(|date| parse_chapter_date(params, &date)),
			url: Some(chapter_url),
			..Default::default()
		},
	})
}

register_source!(
	Madara<Manhwatop>,
	DeepLinkHandler,
	Home,
	MigrationHandler,
	ImageRequestProvider
);
