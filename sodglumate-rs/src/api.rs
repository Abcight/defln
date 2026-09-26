use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Post {
	pub id: u64,
	pub created_at: String,
	pub updated_at: String,
	pub file: File,
	pub preview: Preview,
	pub sample: Sample,
	pub score: Score,
	pub tags: Tags,
	pub locked_tags: Vec<String>,
	pub change_seq: u64,
	pub flags: Flags,
	pub rating: String,
	pub fav_count: u64,
	pub sources: Vec<String>,
	pub pools: Vec<u64>,
	pub relationships: Relationships,
	pub approver_id: Option<u64>,
	pub uploader_id: u64,
	pub description: String,
	pub comment_count: u64,
	pub is_favorited: bool,
	pub has_notes: bool,
	pub duration: Option<f64>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct File {
	pub width: u64,
	pub height: u64,
	pub ext: String,
	pub size: u64,
	pub md5: String,
	pub url: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preview {
	pub width: u64,
	pub height: u64,
	pub url: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
	pub has: bool,
	pub height: u64,
	pub width: u64,
	pub url: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
	pub up: i64,
	pub down: i64,
	pub total: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tags {
	pub general: Vec<String>,
	pub species: Vec<String>,
	pub character: Vec<String>,
	pub copyright: Vec<String>,
	pub artist: Vec<String>,
	pub invalid: Vec<String>,
	pub meta: Vec<String>,
	pub lore: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Flags {
	pub pending: bool,
	pub flagged: bool,
	pub note_locked: bool,
	pub status_locked: bool,
	pub rating_locked: bool,
	pub deleted: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationships {
	pub parent_id: Option<u64>,
	pub has_children: bool,
	pub has_active_children: bool,
	pub children: Vec<u64>,
}

#[derive(Debug, Deserialize)]
pub struct PostsResponse {
	pub posts: Vec<Post>,
}

pub struct E621Client {
	client: reqwest::Client,
}

impl E621Client {
	pub fn new() -> Self {
		Self {
			client: crate::platform::api_client(),
		}
	}

	pub async fn get_post(&self, id: u64) -> anyhow::Result<Post> {
		#[derive(Deserialize)]
		struct PostResponse {
			post: Post,
		}
		let response = self
			.client
			.get(crate::platform::e621_post_url(id))
			.send()
			.await?
			.error_for_status()?
			.json::<PostResponse>()
			.await?;
		anyhow::ensure!(response.post.id == id, "Server returned a different post");
		Ok(response.post)
	}

	pub async fn search_posts(
		&self,
		tags: &str,
		limit: u32,
		page: u32,
	) -> anyhow::Result<Vec<Post>> {
		let url = crate::platform::e621_posts_url();
		log::info!(
			"Searching posts with tags: '{}', limit: {}, page: {}",
			tags,
			limit,
			page
		);

		let query = [
			("tags", tags),
			("limit", &limit.to_string()),
			("page", &page.to_string()),
		];

		let response = self.client.get(url).query(&query).send().await?;

		let status = response.status();
		log::info!("Search response status: {}", status);

		if !status.is_success() {
			let error_text = response
				.text()
				.await
				.unwrap_or_else(|_| "<failed to read error text>".into());
			log::error!("Search failed. Status: {}, Body: {}", status, error_text);
			anyhow::bail!("Request failed with status: {}", status);
		}

		let resp_json: PostsResponse = response.json().await?;
		let original_len = resp_json.posts.len();
		let valid_posts: Vec<Post> = resp_json
			.posts
			.into_iter()
			.filter(|p| p.file.url.is_some())
			.collect();

		log::info!(
			"Found {} valid posts (out of {})",
			valid_posts.len(),
			original_len
		);

		Ok(valid_posts)
	}
}
