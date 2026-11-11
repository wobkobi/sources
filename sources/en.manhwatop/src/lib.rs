#![no_std]
use aidoku::{Source, prelude::*};
use madara::{Impl, Madara, Params};

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
			// The website does not flag the content type of entries.
			filter_non_manga_items: false,
			..Default::default()
		}
	}
}

register_source!(
	Madara<Manhwatop>,
	DeepLinkHandler,
	Home,
	MigrationHandler,
	ImageRequestProvider
);
