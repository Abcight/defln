//! Live regression test for e621 post 2270206.
//!
//! The application used to keep retrying this post's media indefinitely and
//! eventually displayed a completely black image. Keep this test live against
//! the archive so changes to the API record, sample URL, or full media URL
//! make the failure visible instead of silently changing the reproduction.
//!
//! This post also locks in e621's thumbnail behavior: animated GIFs may have
//! `sample.has == false` while still exposing an image at `preview.url`.

use serde::Deserialize;

const POST_ID: u64 = 2_270_206;
const POST_URL: &str = "https://e621.net/posts/2270206.json";

#[derive(Debug, Deserialize)]
struct PostResponse {
	post: PostFixture,
}

#[derive(Debug, Deserialize)]
struct PostFixture {
	id: u64,
	file: FileFixture,
	preview: PreviewFixture,
	sample: SampleFixture,
}

#[derive(Debug, Deserialize)]
struct PreviewFixture {
	width: u64,
	height: u64,
	url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileFixture {
	ext: String,
	url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SampleFixture {
	has: bool,
	url: Option<String>,
}

pub(crate) async fn run(client: &mut super::LiveE621Client) -> anyhow::Result<()> {
	let response = client.get(POST_URL).await?;
	let response: PostResponse = response.json().await?;
	let post = response.post;

	assert_eq!(post.id, POST_ID);
	assert!(
		post.file.url.is_some(),
		"post 2270206 no longer has a full media URL; remove this regression if the post was deleted"
	);
	assert!(
		post.preview.width > 0 && post.preview.height > 0 && post.preview.url.is_some(),
		"playable post 2270206 no longer exposes a usable preview thumbnail"
	);

	let mut media_urls = Vec::new();
	if let Some(url) = post.preview.url.clone() {
		media_urls.push(("preview", url));
	}
	if post.sample.has
		&& let Some(url) = post.sample.url
	{
		media_urls.push(("sample", url));
	}
	if let Some(url) = post.file.url {
		media_urls.push(("full", url));
	}
	assert!(
		!media_urls.is_empty(),
		"post 2270206 has no sample or full media URL; remove this regression if the post was deleted"
	);

	for (kind, url) in media_urls {
		let bytes = client.get(&url).await?.bytes().await?;
		if kind == "sample" || kind == "preview" {
			crate::media::MediaPane::decode_media(&bytes).map_err(|error| {
				anyhow::anyhow!(
					"post 2270206 sample media cannot be decoded as an image (ext={}): {error}",
					post.file.ext
				)
			})?;
		} else {
			assert_eq!(
				crate::types::MediaKind::from_extension(&post.file.ext),
				Some(crate::types::MediaKind::Playable),
				"post 2270206 full media must be routed to the playable backend"
			);
			assert!(!bytes.is_empty(), "post 2270206 full media is empty");
		}
	}

	Ok(())
}
