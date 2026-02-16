//! Transport-agnostic query core.

pub mod error;
pub mod responses;
pub mod types;

mod casts;
mod links;
mod reactions;
mod users;
mod utils;

use crate::core::data_context::{DataContext, Database, HubClient};
use crate::core::types::Fid;

pub use error::{QueryError, QueryResult};
pub use utils::parse_hash_bytes;

#[derive(Clone)]
pub struct WaypointQuery<DB, HC> {
    pub(crate) data_context: DataContext<DB, HC>,
}

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: Database + Clone + Send + Sync + 'static,
    HC: HubClient + Clone + Send + Sync + 'static,
{
    pub fn new(data_context: DataContext<DB, HC>) -> Self {
        Self { data_context }
    }

    pub async fn do_get_conversation(
        &self,
        fid: Fid,
        cast_hash: &str,
        recursive: bool,
        max_depth: usize,
        limit: usize,
    ) -> QueryResult<responses::ConversationResponse> {
        self.do_get_conversation_impl(fid, cast_hash, recursive, max_depth, limit).await
    }
}
