#![no_std]
use aidoku::{Source, prelude::*};
use madara::{Impl, Madara, Params};

const BASE_URL: &str = "https://manhuaus.com";

/// The site sits behind a Cloudflare challenge, so leave the User-Agent to the
/// app: it sends the device's own WebView UA and solves the challenge in a
/// WebView with that same UA. Overriding it (e.g. with a desktop Chrome UA)
/// makes the "Verify you are human" check loop, because the UA no longer
/// matches the WebKit engine running the check.
struct ManhuaUS;

impl Impl for ManhuaUS {
	fn new() -> Self {
		Self
	}

	fn params(&self) -> Params {
		Params {
			base_url: BASE_URL.into(),
			use_new_chapter_endpoint: true,
			// The website does not flag the content type of entries.
			filter_non_manga_items: false,
			..Default::default()
		}
	}
}

register_source!(
	Madara<ManhuaUS>,
	DeepLinkHandler,
	Home,
	MigrationHandler,
	ImageRequestProvider
);
