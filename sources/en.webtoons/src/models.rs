use aidoku::alloc::{String, Vec};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct EpisodeListResponse {
	pub result: EpisodeList,
}

#[derive(Deserialize)]
pub struct EpisodeList {
	#[serde(rename = "episodeList", default)]
	pub episode_list: Vec<Episode>,
}

#[derive(Deserialize)]
pub struct Episode {
	#[serde(rename = "episodeTitle")]
	pub episode_title: String,
	#[serde(rename = "viewerLink")]
	pub viewer_link: String,
	#[serde(rename = "exposureDateMillis", default)]
	pub exposure_date_millis: i64,
}
