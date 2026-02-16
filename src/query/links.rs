//! Link query operations.

use crate::core::types::Fid;
use crate::query::responses::{
    LinkCompactStateResponse, LinkLookupResponse, LinkNotFoundResponse, LinksByFidResponse,
    LinksByTargetResponse,
};
use crate::query::{QueryError, QueryResult, WaypointQuery};

use prost::Message as ProstMessage;

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get a specific link
    pub async fn do_get_link(
        &self,
        fid: Fid,
        link_type: &str,
        target_fid: Fid,
    ) -> QueryResult<LinkLookupResponse> {
        tracing::debug!(
            "Query: Fetching link with FID: {}, type: {}, target: {}",
            fid,
            link_type,
            target_fid
        );

        match self.data_context.get_link(fid, link_type, target_fid).await {
            Ok(Some(message)) => {
                let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload)
                    .map_err(|err| {
                    QueryError::Processing(format!("failed to decode link payload: {}", err))
                })?;

                let link =
                    super::utils::process_link_message(&message, &msg_data).ok_or_else(|| {
                        QueryError::Processing(format!(
                            "link payload could not be processed for FID {}",
                            fid
                        ))
                    })?;
                Ok(LinkLookupResponse::Found(link))
            },
            Ok(None) => Ok(LinkLookupResponse::NotFound(LinkNotFoundResponse {
                fid: fid.value(),
                link_type: link_type.to_string(),
                target_fid: target_fid.value(),
                found: false,
                error: "Link not found".to_string(),
            })),
            Err(e) => Err(e.into()),
        }
    }

    /// Get links by FID
    pub async fn do_get_links_by_fid(
        &self,
        fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> QueryResult<LinksByFidResponse> {
        tracing::debug!("Query: Fetching links for FID: {}", fid);

        let messages = self.data_context.get_links_by_fid(fid, link_type, limit).await?;
        let links = super::utils::parse_link_messages(&messages);
        Ok(LinksByFidResponse { fid: fid.value(), count: links.len(), links })
    }

    /// Get links by target
    pub async fn do_get_links_by_target(
        &self,
        target_fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> QueryResult<LinksByTargetResponse> {
        tracing::debug!("Query: Fetching links to target FID: {}", target_fid);

        let messages = self.data_context.get_links_by_target(target_fid, link_type, limit).await?;
        let links = super::utils::parse_link_messages(&messages);

        Ok(LinksByTargetResponse { target_fid: target_fid.value(), count: links.len(), links })
    }

    /// Get link compact state messages by FID
    pub async fn do_get_link_compact_state_by_fid(
        &self,
        fid: Fid,
    ) -> QueryResult<LinkCompactStateResponse> {
        tracing::debug!("Query: Fetching link compact state for FID: {}", fid);

        let messages = self.data_context.get_link_compact_state_by_fid(fid).await?;
        let links = super::utils::parse_link_compact_state_messages(&messages);

        Ok(LinkCompactStateResponse { fid: fid.value(), count: links.len(), compact_links: links })
    }

    /// Get all links by FID with timestamp filtering
    pub async fn do_get_all_links_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> QueryResult<LinksByFidResponse> {
        tracing::debug!("Query: Fetching all links for FID: {} with time filtering", fid);

        let messages =
            self.data_context.get_all_links_by_fid(fid, limit, _start_time, _end_time).await?;
        let links = super::utils::parse_link_messages(&messages);
        Ok(LinksByFidResponse { fid: fid.value(), count: links.len(), links })
    }
}
