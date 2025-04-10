use crate::{
    hub::subscriber::{PostProcessHandler, PreProcessHandler},
    processor::{consumer::EventProcessor, format::format_message},
    proto::{HubEvent, HubEventType, hub_event::Body},
};
use async_trait::async_trait;
use futures::future::BoxFuture;
use std::sync::Arc;
use tracing::{debug, info};

#[derive(Clone)]
pub struct PrintProcessor {
    _resources: Arc<super::AppResources>, /* Prefixed with underscore to indicate intentionally
                                           * unused */
}

impl PrintProcessor {
    pub fn new(resources: Arc<super::AppResources>) -> Self {
        Self { _resources: resources }
    }

    pub fn create_handlers(
        _processor: Arc<Self>,
    ) -> (Option<PreProcessHandler>, Option<PostProcessHandler>) {
        let pre_process = Arc::new(move |events: &[HubEvent], _: &[Vec<u8>]| {
            let events = events.to_owned();
            Box::pin(async move {
                let mut results = Vec::with_capacity(events.len());
                for event in events {
                    let msg = match TryFrom::try_from(event.r#type).ok() {
                        Some(HubEventType::MergeMessage) => {
                            if let Some(Body::MergeMessageBody(body)) = event.body {
                                body.message
                            } else {
                                None
                            }
                        },
                        Some(HubEventType::PruneMessage) => {
                            if let Some(Body::PruneMessageBody(body)) = event.body {
                                body.message
                            } else {
                                None
                            }
                        },
                        Some(HubEventType::RevokeMessage) => {
                            if let Some(Body::RevokeMessageBody(body)) = event.body {
                                body.message
                            } else {
                                None
                            }
                        },
                        _ => None,
                    };

                    if let Some(msg) = msg {
                        debug!("Pre-processing message: {}", format_message(&msg));
                        results.push(false);
                    } else {
                        results.push(true);
                    }
                }
                results
            }) as BoxFuture<'static, Vec<bool>>
        });

        let post_process = Arc::new(move |events: &[HubEvent], _: &[Vec<u8>]| {
            let events_len = events.len();
            Box::pin(async move {
                info!("Processed batch of {} messages", events_len);
            }) as BoxFuture<'static, ()>
        });

        (Some(pre_process), Some(post_process))
    }
}

#[async_trait]
impl EventProcessor for PrintProcessor {
    async fn process_event(
        &self,
        event: HubEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        match &event.body {
            Some(Body::MergeMessageBody(body)) => {
                if let Some(msg) = &body.message {
                    info!("merge: {}", format_message(msg));
                }
            },
            Some(Body::PruneMessageBody(body)) => {
                if let Some(msg) = &body.message {
                    info!("prune: {}", format_message(msg));
                }
            },
            Some(Body::RevokeMessageBody(body)) => {
                if let Some(msg) = &body.message {
                    info!("revoke: {}", format_message(msg));
                }
            },
            _ => {},
        }
        Ok(())
    }
}
