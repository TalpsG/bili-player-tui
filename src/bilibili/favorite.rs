// Favorite folder API - P2 implementation

use crate::bilibili::api::BilibiliClient;
use crate::error::BilibiliError;
use crate::queue::track::Track;

/// Import tracks from a Bilibili favorite folder.
/// P2 stub - will be implemented in Phase 2.
pub async fn import_favorites(
    _client: &BilibiliClient,
    _folder_id: u64,
) -> Result<Vec<Track>, BilibiliError> {
    todo!("P2: Import favorites from folder ID")
}
