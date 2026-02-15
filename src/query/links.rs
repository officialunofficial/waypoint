//! Link query operations.

use crate::core::types::Fid;
use crate::query::{JsonMap, QueryError, QueryResult, WaypointQuery};

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
    ) -> QueryResult<JsonMap> {
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

                super::utils::process_link_message(&message, &msg_data).ok_or_else(|| {
                    QueryError::Processing(format!(
                        "link payload could not be processed for FID {}",
                        fid
                    ))
                })
            },
            Ok(None) => {
                let mut not_found = serde_json::Map::new();
                not_found.insert("fid".to_string(), serde_json::json!(fid.value()));
                not_found.insert("link_type".to_string(), serde_json::json!(link_type));
                not_found.insert("target_fid".to_string(), serde_json::json!(target_fid.value()));
                not_found.insert("found".to_string(), serde_json::json!(false));
                not_found.insert("error".to_string(), serde_json::json!("Link not found"));
                Ok(not_found)
            },
            Err(e) => Err(e.into()),
        }
    }

    /// Get links by FID
    pub async fn do_get_links_by_fid(
        &self,
        fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching links for FID: {}", fid);

        let messages = self.data_context.get_links_by_fid(fid, link_type, limit).await?;
        Ok(super::utils::format_links_response(messages, Some(fid)))
    }

    /// Get links by target
    pub async fn do_get_links_by_target(
        &self,
        target_fid: Fid,
        link_type: Option<&str>,
        limit: usize,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching links to target FID: {}", target_fid);

        let messages = self.data_context.get_links_by_target(target_fid, link_type, limit).await?;

        let links: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|message| {
                if let Ok(data) = ProstMessage::decode(&*message.payload) {
                    let msg_data: crate::proto::MessageData = data;
                    if let Some(link_obj) = super::utils::process_link_message(message, &msg_data) {
                        return Some(serde_json::Value::Object(link_obj));
                    }
                }
                None
            })
            .collect();

        Ok(serde_json::json!({
            "target_fid": target_fid.value(),
            "count": links.len(),
            "links": links
        }))
    }

    /// Get link compact state messages by FID
    pub async fn do_get_link_compact_state_by_fid(
        &self,
        fid: Fid,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching link compact state for FID: {}", fid);

        let messages = self.data_context.get_link_compact_state_by_fid(fid).await?;

        let links: Vec<serde_json::Value> = messages
            .iter()
            .filter_map(|message| {
                if let Ok(data) = ProstMessage::decode(&*message.payload) {
                    let msg_data: crate::proto::MessageData = data;
                    if let Some(link_obj) = super::utils::process_link_message(message, &msg_data) {
                        return Some(serde_json::Value::Object(link_obj));
                    }
                }
                None
            })
            .collect();

        Ok(serde_json::json!({
            "fid": fid.value(),
            "count": links.len(),
            "compact_links": links
        }))
    }

    /// Get all links by FID with timestamp filtering
    pub async fn do_get_all_links_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> QueryResult<serde_json::Value> {
        tracing::debug!("Query: Fetching all links for FID: {} with time filtering", fid);

        let messages =
            self.data_context.get_all_links_by_fid(fid, limit, _start_time, _end_time).await?;
        Ok(super::utils::format_links_response(messages, Some(fid)))
    }
}
