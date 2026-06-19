#![no_std]
use aidoku::{Result, Source, imports::net::Request, prelude::*};
use madara::{Impl, Madara, Params};

const BASE_URL: &str = "https://manhuaus.com";

// The site sits behind Cloudflare and serves a block page to non-browser
// User-Agents, so a browser UA is required for every request.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
	AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

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

	fn modify_request(&self, _params: &Params, request: Request) -> Result<Request> {
		Ok(request.header("User-Agent", USER_AGENT))
	}
}

register_source!(
	Madara<ManhuaUS>,
	DeepLinkHandler,
	Home,
	MigrationHandler,
	ImageRequestProvider
);
