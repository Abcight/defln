//! Site-specific API adapters that produce the application's `api::Post`.
//!
//! Keep this module separate from `api` so the rest of the application keeps
//! consuming the e621-shaped model regardless of the selected booru.

use crate::api::{
	E621Client, File, Flags, Post, Preview, Relationships, Sample, Score, Tags,
};
use crate::config::{BooruCredentials, DapiCredentials};
use serde_json::{Map, Value};

const RULE34_DAPI_URL: &str = "https://api.rule34.xxx/index.php";
const GELBOORU_DAPI_URL: &str = "https://gelbooru.com/index.php";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BooruSource {
	E621,
	Rule34,
	Gelbooru,
}

impl BooruSource {
	pub(crate) const ALL: [Self; 3] = [Self::E621, Self::Rule34, Self::Gelbooru];

	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::E621 => "e621",
			Self::Rule34 => "Rule34",
			Self::Gelbooru => "Gelbooru",
		}
	}

	pub(crate) fn uses_dapi(self) -> bool {
		!matches!(self, Self::E621)
	}
}

/// The API boundary used by the gateway.
///
/// Every implementation returns `api::Post`, leaving the browser, media
/// loader, and views independent of the remote site's wire format.
pub struct BooruClient {
	inner: Client,
}

enum Client {
	E621(E621Client),
	Dapi(DapiClient),
}

impl BooruClient {
	pub fn new(source: BooruSource, credentials: &BooruCredentials) -> Self {
		let inner = match source {
			BooruSource::E621 => Client::E621(E621Client::new()),
			BooruSource::Rule34 => Client::Dapi(DapiClient::new(
				RULE34_DAPI_URL,
				1_000,
				credentials.rule34.clone(),
			)),
			BooruSource::Gelbooru => Client::Dapi(DapiClient::new(
				GELBOORU_DAPI_URL,
				100,
				credentials.gelbooru.clone(),
			)),
		};
		Self { inner }
	}

	pub async fn get_post(&self, id: u64) -> anyhow::Result<Post> {
		match &self.inner {
			Client::E621(client) => client.get_post(id).await,
			Client::Dapi(client) => client.get_post(id).await,
		}
	}

	pub async fn search_posts(
		&self,
		tags: &str,
		limit: u32,
		page: u32,
	) -> anyhow::Result<Vec<Post>> {
		match &self.inner {
			Client::E621(client) => client.search_posts(tags, limit, page).await,
			Client::Dapi(client) => client.search_posts(tags, limit, page).await,
		}
	}
}

struct DapiClient {
	client: reqwest::Client,
	url: &'static str,
	max_limit: u32,
	credentials: Option<DapiCredentials>,
}

impl DapiClient {
	fn new(url: &'static str, max_limit: u32, credentials: DapiCredentials) -> Self {
		Self {
			client: crate::platform::api_client(),
			url,
			max_limit,
			credentials: credentials.is_complete().then_some(credentials),
		}
	}

	async fn get_post(&self, id: u64) -> anyhow::Result<Post> {
		let posts = self.request_posts(&[("id", id.to_string())]).await?;
		let post = posts
			.into_iter()
			.next()
			.ok_or_else(|| anyhow::anyhow!("Post {id} was not found"))?;
		anyhow::ensure!(post.id == id, "Server returned a different post");
		Ok(post)
	}

	async fn search_posts(
		&self,
		tags: &str,
		limit: u32,
		page: u32,
	) -> anyhow::Result<Vec<Post>> {
		let page = page.saturating_sub(1);
		let posts = self
			.request_posts(&[
				("tags", tags.to_owned()),
				("limit", limit.min(self.max_limit).to_string()),
				("pid", page.to_string()),
			])
			.await?;
		Ok(posts
			.into_iter()
			.filter(|post| post.file.url.is_some())
			.collect())
	}

	async fn request_posts(
		&self,
		parameters: &[(&str, String)],
	) -> anyhow::Result<Vec<Post>> {
		let mut query = vec![
			("page", "dapi".to_owned()),
			("s", "post".to_owned()),
			("q", "index".to_owned()),
			("json", "1".to_owned()),
		];
		if let Some(credentials) = &self.credentials {
			query.push(("user_id", credentials.user_id.clone()));
			query.push(("api_key", credentials.api_key.clone()));
		}
		query.extend_from_slice(parameters);

		let response = self
			.client
			.get(self.url)
			.query(&query)
			.send()
			.await?
			.error_for_status()?;
		let value = response.json::<Value>().await?;
		parse_dapi_posts(value)
	}
}

fn parse_dapi_posts(value: Value) -> anyhow::Result<Vec<Post>> {
	let posts = match value {
		Value::Array(posts) => posts,
		Value::Object(mut response) => match response.remove("post") {
			Some(Value::Array(posts)) => posts,
			Some(post) => vec![post],
			None => anyhow::bail!(
				"DAPI request rejected: {}",
				dapi_error_message(&response)
			),
		},
		Value::String(message) => anyhow::bail!("DAPI request rejected: {message}"),
		_ => anyhow::bail!("DAPI response was not a post list"),
	};

	posts.into_iter().map(dapi_post_to_post).collect()
}

fn dapi_error_message(response: &Map<String, Value>) -> String {
	["message", "reason", "error"]
		.into_iter()
		.find_map(|key| text(response, key))
		.unwrap_or_else(|| "the server did not return posts".to_owned())
}

fn dapi_post_to_post(value: Value) -> anyhow::Result<Post> {
	let object = value
		.as_object()
		.ok_or_else(|| anyhow::anyhow!("DAPI post was not an object"))?;
	let id =
		number(object, "id").ok_or_else(|| anyhow::anyhow!("DAPI post has no id"))?;
	let file_url = text(object, "file_url");
	let file_name = text(object, "image");
	let extension = file_url
		.as_deref()
		.and_then(file_extension)
		.or_else(|| file_name.as_deref().and_then(file_extension))
		.unwrap_or_default()
		.to_owned();
	let preview_url = text(object, "preview_url");
	let sample_url = text(object, "sample_url");
	let sample_is_full_media = sample_url.as_deref() == file_url.as_deref();
	let media_url = (!sample_is_full_media).then_some(file_url).flatten();

	Ok(Post {
		id,
		created_at: text(object, "created_at").unwrap_or_default(),
		updated_at: text(object, "change")
			.or_else(|| text(object, "created_at"))
			.unwrap_or_default(),
		file: File {
			width: number(object, "width").unwrap_or_default(),
			height: number(object, "height").unwrap_or_default(),
			ext: extension,
			size: number(object, "file_size").unwrap_or_default(),
			md5: text(object, "md5").unwrap_or_default(),
			url: media_url,
		},
		preview: Preview {
			width: number(object, "preview_width").unwrap_or_default(),
			height: number(object, "preview_height").unwrap_or_default(),
			url: preview_url,
		},
		sample: Sample {
			// Rule34 sometimes reports the original GIF as its sample. Such posts
			// are excluded from search results below instead of attempting the
			// same unsafe file twice.
			has: sample_url.is_some(),
			height: number(object, "sample_height").unwrap_or_default(),
			width: number(object, "sample_width").unwrap_or_default(),
			url: sample_url,
		},
		score: Score {
			total: signed_number(object, "score").unwrap_or_default(),
			..Score::default()
		},
		tags: Tags {
			general: text(object, "tags")
				.unwrap_or_default()
				.split_whitespace()
				.map(str::to_owned)
				.collect(),
			..Tags::default()
		},
		change_seq: number(object, "change").unwrap_or_default(),
		flags: Flags {
			deleted: text(object, "status").as_deref() == Some("deleted"),
			..Flags::default()
		},
		rating: text(object, "rating").unwrap_or_default(),
		fav_count: number(object, "fav_count").unwrap_or_default(),
		sources: text(object, "source")
			.unwrap_or_default()
			.split_whitespace()
			.map(str::to_owned)
			.collect(),
		relationships: Relationships {
			parent_id: number(object, "parent_id").filter(|id| *id != 0),
			..Relationships::default()
		},
		uploader_id: number(object, "creator_id").unwrap_or_default(),
		comment_count: number(object, "comment_count").unwrap_or_default(),
		..Post::default()
	})
}

fn text(object: &Map<String, Value>, key: &str) -> Option<String> {
	match object.get(key)? {
		Value::String(value) if !value.is_empty() => Some(value.clone()),
		Value::Number(value) => Some(value.to_string()),
		_ => None,
	}
}

fn number(object: &Map<String, Value>, key: &str) -> Option<u64> {
	match object.get(key)? {
		Value::Number(value) => value.as_u64(),
		Value::String(value) => value.parse().ok(),
		_ => None,
	}
}

fn signed_number(object: &Map<String, Value>, key: &str) -> Option<i64> {
	match object.get(key)? {
		Value::Number(value) => value.as_i64(),
		Value::String(value) => value.parse().ok(),
		_ => None,
	}
}

fn file_extension(url: &str) -> Option<&str> {
	url.split('?')
		.next()
		.and_then(|path| path.rsplit_once('.').map(|(_, extension)| extension))
		.filter(|extension| !extension.is_empty())
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn dapi_posts_are_retrofitted_to_the_application_model() {
		let posts = parse_dapi_posts(json!([{
			"id": "42",
			"created_at": "2026-01-02 03:04:05",
			"change": "123",
			"width": "1920",
			"height": 1080,
			"md5": "abc",
			"file_url": "https://img.example.test/images/post.webp",
			"preview_url": "https://img.example.test/preview/post.jpg",
			"preview_width": "150",
			"preview_height": "84",
			"sample": "1",
			"sample_url": "https://img.example.test/samples/post.jpg",
			"sample_width": "850",
			"sample_height": "478",
			"score": "99",
			"tags": "blue_eyes original_character",
			"rating": "e",
			"source": "https://source.example.test/post",
			"parent_id": "12",
			"creator_id": "7",
			"comment_count": "3"
		}]))
		.expect("fixture is valid");

		assert_eq!(posts.len(), 1);
		let post = &posts[0];
		assert_eq!(post.id, 42);
		assert_eq!(post.file.ext, "webp");
		assert_eq!(
			post.file.url.as_deref(),
			Some("https://img.example.test/images/post.webp")
		);
		assert_eq!(post.preview.width, 150);
		assert!(post.sample.has);
		assert_eq!(post.score.total, 99);
		assert_eq!(post.tags.general, ["blue_eyes", "original_character"]);
		assert_eq!(post.relationships.parent_id, Some(12));
	}

	#[test]
	fn wrapped_dapi_posts_and_zero_parent_ids_are_supported() {
		let posts = parse_dapi_posts(json!({
			"post": [{
				"id": 7,
				"file_url": "https://img.example.test/post.png?download=1",
				"parent_id": "0"
			}]
		}))
		.expect("fixture is valid");

		assert_eq!(posts[0].file.ext, "png");
		assert_eq!(posts[0].relationships.parent_id, None);
	}

	#[test]
	fn dapi_authentication_errors_are_preserved() {
		let error = parse_dapi_posts(json!("Missing authentication"))
			.expect_err("authentication response is not a post list");

		assert_eq!(
			error.to_string(),
			"DAPI request rejected: Missing authentication"
		);
	}

	#[test]
	fn dapi_full_media_is_excluded_when_used_as_its_own_sample() {
		let posts = parse_dapi_posts(json!([{
			"id": 42,
			"file_url": "https://img.example.test/post.gif",
			"sample": "1",
			"sample_url": "https://img.example.test/post.gif",
			"preview_url": "https://img.example.test/preview.jpg"
		}]))
		.expect("fixture is valid");

		assert!(posts[0].sample.has);
		assert_eq!(
			posts[0].sample.url.as_deref(),
			Some("https://img.example.test/post.gif")
		);
		assert_eq!(posts[0].file.url, None);
		assert_eq!(
			posts[0].preview.url.as_deref(),
			Some("https://img.example.test/preview.jpg")
		);
	}

	#[test]
	fn every_booru_source_has_a_display_label() {
		assert_eq!(
			BooruSource::ALL.map(BooruSource::label),
			["e621", "Rule34", "Gelbooru"]
		);
	}
}
