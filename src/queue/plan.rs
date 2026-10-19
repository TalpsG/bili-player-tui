//! Resolve video-level selections before committing a queue mutation.
use super::track::Track;
use crate::{
    bilibili::{
        api::BilibiliClient,
        video::{VideoPage, get_video_pages},
    },
    error::BilibiliError,
};
use std::collections::HashMap;

pub struct ResolvedPlan {
    pub tracks: Vec<Track>,
    pub start: usize,
    pub pages: HashMap<String, Vec<VideoPage>>,
}

/// Pure expansion used by both warm and cold resolution paths. Explicit parts are leaves.
pub fn materialize(
    tracks: &[Track],
    start: usize,
    page: Option<u32>,
    pages: &HashMap<String, Vec<VideoPage>>,
) -> Result<(Vec<Track>, usize), BilibiliError> {
    let mut output = Vec::new();
    let mut selected = 0;
    for (i, track) in tracks.iter().enumerate() {
        if track.page.is_some() {
            if i == start {
                selected = output.len();
            }
            output.push(track.clone());
            continue;
        }
        let parts = pages
            .get(&track.bvid)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| BilibiliError::Parse(format!("No parts for {}", track.bvid)))?;
        let target = page.unwrap_or(parts[0].page);
        if i == start && !parts.iter().any(|p| p.page == target) {
            return Err(BilibiliError::Parse(format!(
                "Missing P{target} for {}",
                track.bvid
            )));
        }
        // Pagelist always contains P1, including ordinary single-page videos.
        // Resolve playback metadata without turning such a video into a part:
        // page/part also determine row structure and persisted item identity.
        if parts.len() == 1 {
            if i == start {
                selected = output.len();
            }
            let mut resolved = track.clone();
            resolved.cid = parts[0].cid;
            resolved.duration = parts[0].duration();
            resolved.source = None;
            output.push(resolved);
            continue;
        }
        for part in parts {
            if i == start && part.page == target {
                selected = output.len();
            }
            output.push(part.to_track(track));
        }
    }
    Ok((output, selected))
}

/// At most four requests run concurrently; materialization retains playlist order.
pub async fn resolve(
    client: BilibiliClient,
    tracks: Vec<Track>,
    start: usize,
    page: Option<u32>,
    pages: HashMap<String, Vec<VideoPage>>,
) -> Result<ResolvedPlan, BilibiliError> {
    resolve_with(tracks, start, page, pages, move |bvid| {
        let client = client.clone();
        async move { get_video_pages(&client, &bvid).await }
    })
    .await
}

async fn resolve_with<F, Fut>(
    tracks: Vec<Track>,
    start: usize,
    page: Option<u32>,
    mut pages: HashMap<String, Vec<VideoPage>>,
    fetch: F,
) -> Result<ResolvedPlan, BilibiliError>
where
    F: Fn(String) -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = Result<Vec<VideoPage>, BilibiliError>> + Send + 'static,
{
    let mut missing: Vec<String> = tracks
        .iter()
        .filter(|t| t.page.is_none() && !pages.contains_key(&t.bvid))
        .map(|t| t.bvid.clone())
        .collect();
    missing.sort();
    missing.dedup();
    let mut pending = missing.into_iter();
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        while tasks.len() < 4 {
            let Some(bvid) = pending.next() else {
                break;
            };
            let fetch = fetch.clone();
            tasks.spawn(async move {
                let result = fetch(bvid.clone()).await;
                (bvid, result)
            });
        }
        let Some(result) = tasks.join_next().await else {
            break;
        };
        let (bvid, result) =
            result.map_err(|e| BilibiliError::Parse(format!("Part loading stopped: {e}")))?;
        pages.insert(bvid, result?);
    }
    let (tracks, start) = materialize(&tracks, start, page, &pages)?;
    Ok(ResolvedPlan {
        tracks,
        start,
        pages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(bvid: &str) -> Track {
        Track {
            bvid: bvid.into(),
            cid: 0,
            title: bvid.into(),
            author: "up".into(),
            duration: std::time::Duration::ZERO,
            cover_url: None,
            part: None,
            page: None,
            source: None,
        }
    }
    fn parts() -> Vec<VideoPage> {
        (1..=3)
            .map(|page| VideoPage {
                cid: page as u64 + 10,
                page,
                part: format!("P{page}"),
                duration_secs: 60,
            })
            .collect()
    }
    #[tokio::test]
    async fn single_page_video_keeps_video_identity_with_cold_and_warm_cache() {
        let base = track("single");
        let single = vec![parts().remove(0)];
        let mut outputs = Vec::new();
        for cache in [
            HashMap::new(),
            HashMap::from([("single".into(), single.clone())]),
        ] {
            let fetched = single.clone();
            let result = resolve_with(vec![base.clone()], 0, None, cache, move |_| {
                let fetched = fetched.clone();
                async move { Ok(fetched) }
            })
            .await
            .unwrap();
            assert_eq!(result.start, 0);
            assert_eq!(result.tracks.len(), 1);
            let resolved = &result.tracks[0];
            assert_eq!(resolved.page, None);
            assert_eq!(resolved.part, None);
            assert_eq!(resolved.display_title(), base.title);
            assert_eq!(resolved.cid, single[0].cid);
            assert_eq!(resolved.duration, single[0].duration());
            assert!(resolved.same_item(&base));
            let (again, start) = materialize(&result.tracks, 0, None, &result.pages).unwrap();
            assert_eq!(again, result.tracks);
            assert_eq!(start, 0);
            outputs.push(result.tracks);
        }
        assert_eq!(outputs[0], outputs[1]);
    }

    #[test]
    fn mixed_videos_expand_only_real_multipart_and_preserve_saved_parts() {
        let single = track("single");
        let multi = track("multi");
        let saved = parts()[1].to_track(&track("saved"));
        let cache = HashMap::from([
            (single.bvid.clone(), vec![parts().remove(0)]),
            (multi.bvid.clone(), parts()),
            (saved.bvid.clone(), parts()),
        ]);
        let (resolved, start) =
            materialize(&[single.clone(), multi, saved.clone()], 1, Some(2), &cache).unwrap();
        assert_eq!(resolved.len(), 5);
        assert_eq!(start, 2);
        assert!(resolved[0].same_item(&single));
        assert_eq!(resolved[0].page, None);
        assert_eq!(
            resolved[1..4].iter().map(|t| t.page).collect::<Vec<_>>(),
            vec![Some(1), Some(2), Some(3)]
        );
        assert_eq!(resolved[4], saved);
    }

    #[tokio::test]
    async fn explicit_saved_part_is_a_leaf_even_without_cached_pages() {
        let saved = parts()[0].to_track(&track("saved"));
        let result = resolve_with(vec![saved.clone()], 0, None, HashMap::new(), |_| async {
            panic!("saved parts must not fetch or expand their video")
        })
        .await
        .unwrap();
        assert_eq!(result.tracks, vec![saved]);
        assert!(result.pages.is_empty());
    }
    #[tokio::test]
    async fn cold_partial_and_hot_cache_produce_identical_complete_sequences() {
        let tracks = vec![track("A"), parts()[1].to_track(&track("B")), track("C")];
        let mut outputs = Vec::new();
        for warm in [vec![], vec!["A"], vec!["A", "C"]] {
            let cache = warm.into_iter().map(|b| (b.to_owned(), parts())).collect();
            let result = resolve_with(tracks.clone(), 1, Some(2), cache, |_| async { Ok(parts()) })
                .await
                .unwrap();
            assert_eq!(result.start, 3);
            assert_eq!(result.tracks.len(), 7);
            assert_eq!(result.tracks[3].page, Some(2));
            outputs.push(result.tracks);
        }
        assert_eq!(outputs[0], outputs[1]);
        assert_eq!(outputs[1], outputs[2]);
    }
    #[tokio::test]
    async fn failed_resolution_never_returns_partial_plan() {
        let result = resolve_with(
            vec![track("A"), track("B")],
            0,
            None,
            HashMap::new(),
            |b| async move {
                if b == "A" {
                    Ok(parts())
                } else {
                    Err(BilibiliError::Parse("unavailable".into()))
                }
            },
        )
        .await;
        assert!(result.is_err());
    }
}
