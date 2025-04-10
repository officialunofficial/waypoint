use crate::{
    database::client::Database,
    hub::error::Error,
    processor::consumer::EventProcessor,
    proto::{
        self, FidRequest, HubEvent, HubEventType, LinksByFidRequest, MergeMessageBody, Message,
        ReactionsByFidRequest,
    },
};
use std::{sync::Arc, time::Duration};
use tonic::transport::Channel;
use tracing::{debug, error, info};

pub struct MessageReconciler {
    hub_client: proto::hub_service_client::HubServiceClient<Channel>,
    _database: Arc<Database>, // Prefixed with underscore to indicate intentionally unused
    _connection_timeout: Duration, // Prefixed with underscore to indicate intentionally unused
    _use_streaming_rpcs: bool, // Prefixed with underscore to indicate intentionally unused
}

impl MessageReconciler {
    pub fn new(
        hub_client: proto::hub_service_client::HubServiceClient<Channel>,
        database: Arc<Database>,
        connection_timeout: Duration,
        use_streaming_rpcs: bool,
    ) -> Self {
        Self {
            hub_client,
            _database: database,
            _connection_timeout: connection_timeout,
            _use_streaming_rpcs: use_streaming_rpcs,
        }
    }

    pub async fn reconcile_fid(
        &self,
        fid: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<(), Error> {
        info!("Starting reconciliation for FID {}", fid);
        let start_time = std::time::Instant::now();

        // Fetch all message types concurrently using tokio::try_join!
        let (casts, reactions, links, verifications, user_data) = tokio::try_join!(
            self.get_all_cast_messages(fid),
            self.get_all_reaction_messages(fid),
            self.get_all_link_messages(fid),
            self.get_all_verification_messages(fid),
            self.get_all_user_data_messages(fid)
        )?;

        // Process count for each message type
        let casts_count = casts.len();
        let reactions_count = reactions.len();
        let links_count = links.len();
        let verifications_count = verifications.len();
        let user_data_count = user_data.len();

        info!(
            "Fetched messages for FID {}: {} casts, {} reactions, {} links, {} verifications, {} user data",
            fid, casts_count, reactions_count, links_count, verifications_count, user_data_count
        );

        // Process each message type, but handle user_data specially
        let non_user_data_groups = vec![
            ("casts", casts),
            ("reactions", reactions),
            ("links", links),
            ("verifications", verifications),
        ];

        // Using a semaphore to limit concurrent processing
        let semaphore = Arc::new(tokio::sync::Semaphore::new(10));

        // First process all non-user_data messages
        for (message_type, messages) in non_user_data_groups {
            let count = messages.len();
            if count == 0 {
                continue;
            }

            info!("Processing {} {} messages for FID {}", count, message_type, fid);

            let mut success_count = 0;
            let mut error_count = 0;

            // Create a set of tasks to process messages in batches
            let mut handles = Vec::new();

            // Process in chunks of 100 messages
            for chunk in messages.chunks(100) {
                let processor_clone = Arc::clone(&processor);
                let chunk_vec = chunk.to_vec();
                let message_type_str = message_type.to_string();
                let semaphore_clone = Arc::clone(&semaphore);

                // Create events before spawning the task to avoid self reference
                let mut events = Vec::with_capacity(chunk_vec.len());
                for message in chunk_vec {
                    events.push(self.message_to_hub_event(message));
                }

                let handle = tokio::spawn(async move {
                    // Acquire permit to limit concurrency
                    let _permit = semaphore_clone.acquire().await.unwrap();

                    let mut chunk_success = 0;
                    let mut chunk_error = 0;

                    for event in events {
                        match processor_clone.process_event(event).await {
                            Ok(_) => chunk_success += 1,
                            Err(e) => {
                                error!(
                                    "Error processing {} message for FID {}: {:?}",
                                    message_type_str, fid, e
                                );
                                chunk_error += 1;
                            },
                        }
                    }

                    (chunk_success, chunk_error)
                });

                handles.push(handle);
            }

            // Wait for all processing to complete
            for handle in handles {
                match handle.await {
                    Ok((chunk_success, chunk_error)) => {
                        success_count += chunk_success;
                        error_count += chunk_error;
                    },
                    Err(e) => {
                        error!("Task error processing {} for FID {}: {:?}", message_type, fid, e);
                        error_count += 1;
                    },
                }
            }

            info!(
                "Completed processing {} {} messages for FID {} ({} succeeded, {} failed)",
                count, message_type, fid, success_count, error_count
            );
        }

        // Now handle user_data specially to ensure complete profiles are built
        let user_data_count = user_data.len();
        if user_data_count > 0 {
            info!("Processing {} user_data messages for FID {}", user_data_count, fid);

            // Regular user data processing
            {
                // Process user data
                let mut success_count = 0;
                let mut error_count = 0;

                // Process in chunks of 100 messages
                let mut handles = Vec::new();

                for chunk in user_data.chunks(100) {
                    let processor_clone = Arc::clone(&processor);
                    let chunk_vec = chunk.to_vec();
                    let semaphore_clone = Arc::clone(&semaphore);

                    // Create events before spawning the task to avoid self reference
                    let mut events = Vec::with_capacity(chunk_vec.len());
                    for message in chunk_vec {
                        events.push(self.message_to_hub_event(message));
                    }

                    let handle = tokio::spawn(async move {
                        // Acquire permit to limit concurrency
                        let _permit = semaphore_clone.acquire().await.unwrap();

                        let mut chunk_success = 0;
                        let mut chunk_error = 0;

                        for event in events {
                            match processor_clone.process_event(event).await {
                                Ok(_) => chunk_success += 1,
                                Err(e) => {
                                    error!(
                                        "Error processing user_data message for FID {}: {:?}",
                                        fid, e
                                    );
                                    chunk_error += 1;
                                },
                            }
                        }

                        (chunk_success, chunk_error)
                    });

                    handles.push(handle);
                }

                // Wait for all processing to complete
                for handle in handles {
                    match handle.await {
                        Ok((chunk_success, chunk_error)) => {
                            success_count += chunk_success;
                            error_count += chunk_error;
                        },
                        Err(e) => {
                            error!("Task error processing user_data for FID {}: {:?}", fid, e);
                            error_count += 1;
                        },
                    }
                }

                info!(
                    "Completed processing {} user_data messages for FID {} ({} succeeded, {} failed)",
                    user_data_count, fid, success_count, error_count
                );
            }
        }

        // Calculate total messages processed
        let total_count =
            casts_count + reactions_count + links_count + verifications_count + user_data_count;
        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciliation for FID {} in {:.2?}: processed {} total messages ({} casts, {} reactions, {} links, {} verifications, {} user data)",
            fid,
            elapsed,
            total_count,
            casts_count,
            reactions_count,
            links_count,
            verifications_count,
            user_data_count
        );

        Ok(())
    }

    #[allow(dead_code)]
    async fn reconcile_casts(
        &self,
        fid: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<usize, Error> {
        info!("Reconciling casts for FID {}", fid);
        let start_time = std::time::Instant::now();

        let messages = self.get_all_cast_messages(fid).await?;
        let count = messages.len();

        debug!("Retrieved {} casts for FID {}", count, fid);

        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, message) in messages.into_iter().enumerate() {
            let event = self.message_to_hub_event(message);

            match processor.process_event(event).await {
                Err(e) => {
                    error!(
                        "Error processing cast message {}/{} for FID {}: {:?}",
                        idx + 1,
                        count,
                        fid,
                        e
                    );
                    error_count += 1;
                },
                _ => {
                    success_count += 1;
                    if success_count % 100 == 0 {
                        debug!("Processed {}/{} casts for FID {}", success_count, count, fid);
                    }
                },
            }
        }

        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciling casts for FID {}: processed {} messages ({} succeeded, {} failed) in {:.2?}",
            fid, count, success_count, error_count, elapsed
        );

        // No additional success log needed

        Ok(count)
    }

    #[allow(dead_code)]
    async fn reconcile_reactions(
        &self,
        fid: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<usize, Error> {
        info!("Reconciling reactions for FID {}", fid);
        let start_time = std::time::Instant::now();

        let messages = self.get_all_reaction_messages(fid).await?;
        let count = messages.len();

        debug!("Retrieved {} reactions for FID {}", count, fid);

        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, message) in messages.into_iter().enumerate() {
            let event = self.message_to_hub_event(message);

            match processor.process_event(event).await {
                Err(e) => {
                    error!(
                        "Error processing reaction message {}/{} for FID {}: {:?}",
                        idx + 1,
                        count,
                        fid,
                        e
                    );
                    error_count += 1;
                },
                _ => {
                    success_count += 1;
                    if success_count % 100 == 0 {
                        debug!("Processed {}/{} reactions for FID {}", success_count, count, fid);
                    }
                },
            }
        }

        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciling reactions for FID {}: processed {} messages ({} succeeded, {} failed) in {:.2?}",
            fid, count, success_count, error_count, elapsed
        );

        Ok(count)
    }

    #[allow(dead_code)]
    async fn reconcile_links(
        &self,
        fid: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<usize, Error> {
        info!("Reconciling links for FID {}", fid);
        let start_time = std::time::Instant::now();

        let messages = self.get_all_link_messages(fid).await?;
        let count = messages.len();

        debug!("Retrieved {} links for FID {}", count, fid);

        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, message) in messages.into_iter().enumerate() {
            let event = self.message_to_hub_event(message);

            match processor.process_event(event).await {
                Err(e) => {
                    error!(
                        "Error processing link message {}/{} for FID {}: {:?}",
                        idx + 1,
                        count,
                        fid,
                        e
                    );
                    error_count += 1;
                },
                _ => {
                    success_count += 1;
                    if success_count % 100 == 0 {
                        debug!("Processed {}/{} links for FID {}", success_count, count, fid);
                    }
                },
            }
        }

        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciling links for FID {}: processed {} messages ({} succeeded, {} failed) in {:.2?}",
            fid, count, success_count, error_count, elapsed
        );

        Ok(count)
    }

    #[allow(dead_code)]
    async fn reconcile_verifications(
        &self,
        fid: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<usize, Error> {
        info!("Reconciling verifications for FID {}", fid);
        let start_time = std::time::Instant::now();

        let messages = self.get_all_verification_messages(fid).await?;
        let count = messages.len();

        debug!("Retrieved {} verifications for FID {}", count, fid);

        let mut success_count = 0;
        let mut error_count = 0;

        for (idx, message) in messages.into_iter().enumerate() {
            let event = self.message_to_hub_event(message);

            match processor.process_event(event).await {
                Err(e) => {
                    error!(
                        "Error processing verification message {}/{} for FID {}: {:?}",
                        idx + 1,
                        count,
                        fid,
                        e
                    );
                    error_count += 1;
                },
                _ => {
                    success_count += 1;
                    if success_count % 100 == 0 {
                        debug!(
                            "Processed {}/{} verifications for FID {}",
                            success_count, count, fid
                        );
                    }
                },
            }
        }

        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciling verifications for FID {}: processed {} messages ({} succeeded, {} failed) in {:.2?}",
            fid, count, success_count, error_count, elapsed
        );

        Ok(count)
    }

    #[allow(dead_code)]
    async fn reconcile_user_data(
        &self,
        fid: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<usize, Error> {
        info!("Reconciling user data for FID {}", fid);
        let start_time = std::time::Instant::now();

        let messages = self.get_all_user_data_messages(fid).await?;
        let count = messages.len();

        debug!("Retrieved {} user data messages for FID {}", count, fid);

        let mut success_count = 0;
        let mut error_count = 0;

        // Process user data messages one by one
        for (idx, message) in messages.into_iter().enumerate() {
            let event = self.message_to_hub_event(message);

            match processor.process_event(event).await {
                Err(e) => {
                    error!(
                        "Error processing user data message {}/{} for FID {}: {:?}",
                        idx + 1,
                        count,
                        fid,
                        e
                    );
                    error_count += 1;
                },
                _ => {
                    success_count += 1;
                    if success_count % 100 == 0 {
                        debug!(
                            "Processed {}/{} user data messages for FID {}",
                            success_count, count, fid
                        );
                    }
                },
            }
        }

        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciling user data for FID {}: processed {} messages ({} succeeded, {} failed) in {:.2?}",
            fid, count, success_count, error_count, elapsed
        );

        Ok(count)
    }

    async fn get_all_cast_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        // Increase page size from 100 to 1000 to reduce round trips
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        debug!("Fetching casts for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response =
                self.hub_client.clone().get_casts_by_fid(tonic::Request::new(request)).await?;

            let response = response.into_inner();
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            debug!(
                "Received page {} with {} casts for FID {}",
                page_count, page_messages_count, fid
            );

            if let Some(token) = response.next_page_token {
                if token.is_empty() {
                    break;
                }
                page_token = Some(token);
            } else {
                break;
            }
        }

        debug!(
            "Fetched a total of {} casts for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    async fn get_all_reaction_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        // Increase page size from 100 to 1000 to reduce round trips
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        debug!("Fetching reactions for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = ReactionsByFidRequest {
                fid,
                reaction_type: None, // Get all reaction types
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response =
                self.hub_client.clone().get_reactions_by_fid(tonic::Request::new(request)).await?;

            let response = response.into_inner();
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            debug!(
                "Received page {} with {} reactions for FID {}",
                page_count, page_messages_count, fid
            );

            if let Some(token) = response.next_page_token {
                if token.is_empty() {
                    break;
                }
                page_token = Some(token);
            } else {
                break;
            }
        }

        debug!(
            "Fetched a total of {} reactions for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    async fn get_all_link_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        // Increase page size from 100 to 1000 to reduce round trips
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        debug!("Fetching links for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = LinksByFidRequest {
                fid,
                link_type: None, // Get all link types
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response =
                self.hub_client.clone().get_links_by_fid(tonic::Request::new(request)).await?;

            let response = response.into_inner();
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            debug!(
                "Received page {} with {} links for FID {}",
                page_count, page_messages_count, fid
            );

            if let Some(token) = response.next_page_token {
                if token.is_empty() {
                    break;
                }
                page_token = Some(token);
            } else {
                break;
            }
        }

        debug!(
            "Fetched a total of {} links for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    async fn get_all_verification_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        // Increase page size from 100 to 1000 to reduce round trips
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        debug!("Fetching verifications for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = self
                .hub_client
                .clone()
                .get_verifications_by_fid(tonic::Request::new(request))
                .await?;

            let response = response.into_inner();
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            debug!(
                "Received page {} with {} verifications for FID {}",
                page_count, page_messages_count, fid
            );

            if let Some(token) = response.next_page_token {
                if token.is_empty() {
                    break;
                }
                page_token = Some(token);
            } else {
                break;
            }
        }

        debug!(
            "Fetched a total of {} verifications for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    pub async fn get_all_user_data_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        // Increase page size from 100 to 1000 to reduce round trips
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        debug!("Fetching user data for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response =
                self.hub_client.clone().get_user_data_by_fid(tonic::Request::new(request)).await?;

            let response = response.into_inner();
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            debug!(
                "Received page {} with {} user data messages for FID {}",
                page_count, page_messages_count, fid
            );

            if let Some(token) = response.next_page_token {
                if token.is_empty() {
                    break;
                }
                page_token = Some(token);
            } else {
                break;
            }
        }

        debug!(
            "Fetched a total of {} user data messages for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    pub fn message_to_hub_event(&self, message: Message) -> HubEvent {
        // Log message details
        let _message_type = self.get_message_type(&message);
        let _timestamp = message.data.as_ref().map(|d| d.timestamp).unwrap_or(0);
        let _fid = message.data.as_ref().map(|d| d.fid).unwrap_or(0);

        // info!(
        //     "Processing message: type={}, fid={}, timestamp={}, hash={}",
        //     message_type,
        //     fid,
        //     timestamp,
        //     hex::encode(&message.hash)
        // );

        // Create the HubEvent
        let merge_message_body =
            MergeMessageBody { message: Some(message), deleted_messages: Vec::new() };

        HubEvent {
            id: 0, // Will be set by the hub
            r#type: HubEventType::MergeMessage as i32,
            body: Some(proto::hub_event::Body::MergeMessageBody(merge_message_body)),
        }
    }

    fn get_message_type(&self, message: &Message) -> String {
        if let Some(data) = &message.data {
            if let Some(body) = &data.body {
                use proto::message_data::Body;
                return match body {
                    Body::CastAddBody(_) => "CastAdd".to_string(),
                    Body::CastRemoveBody(_) => "CastRemove".to_string(),
                    Body::ReactionBody(_) => "Reaction".to_string(),
                    Body::VerificationAddAddressBody(_) => "VerificationAddAddress".to_string(),
                    Body::VerificationRemoveBody(_) => "VerificationRemove".to_string(),
                    Body::UserDataBody(_) => "UserData".to_string(),
                    Body::LinkBody(_) => "Link".to_string(),
                    Body::UsernameProofBody(_) => "UsernameProof".to_string(),
                    _ => "Unknown".to_string(),
                };
            }
        }
        "Unknown".to_string()
    }
}
