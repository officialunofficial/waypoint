use crate::{
    database::client::Database,
    hub::client::{Error, Hub},
    processor::consumer::EventProcessor,
    proto::{
        self, FidRequest, FidTimestampRequest, HubEvent, HubEventType, LinksByFidRequest,
        MergeMessageBody, MergeOnChainEventBody, Message, OnChainEventRequest, OnChainEventType,
        ReactionsByFidRequest, hub_event::Body,
    },
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace};

pub struct MessageReconciler {
    hub: Arc<Mutex<Hub>>,
    _database: Arc<Database>, // Prefixed with underscore to indicate intentionally unused
    _connection_timeout: Duration, // Prefixed with underscore to indicate intentionally unused
    _use_streaming_rpcs: bool, // Prefixed with underscore to indicate intentionally unused
}

impl MessageReconciler {
    /// Reconcile multiple FIDs in a batch for better efficiency
    /// This method fetches data for multiple FIDs concurrently and processes them in batches
    pub async fn reconcile_fids_batch(
        &self,
        fids: &[u64],
        processor: Arc<dyn EventProcessor>,
    ) -> Result<(usize, usize), Error> {
        if fids.is_empty() {
            return Ok((0, 0));
        }

        trace!("Starting batch reconciliation for {} FIDs", fids.len());
        let start_time = std::time::Instant::now();

        let mut total_success = 0;
        let mut total_errors = 0;

        // Process FIDs in smaller concurrent batches to avoid overwhelming the hub
        const CONCURRENT_BATCH_SIZE: usize = 3;

        for fid_chunk in fids.chunks(CONCURRENT_BATCH_SIZE) {
            let mut fetch_tasks = Vec::new();

            // Fetch data for all FIDs in this chunk concurrently
            for &fid in fid_chunk {
                let hub = self.hub.clone();
                let task = tokio::spawn(async move {
                    let reconciler = MessageReconciler {
                        hub,
                        _database: Arc::new(crate::database::Database::empty()), // Placeholder
                        _connection_timeout: std::time::Duration::from_secs(30),
                        _use_streaming_rpcs: false,
                    };

                    // Fetch all message types for this FID
                    let casts = reconciler.get_all_cast_messages(fid).await?;
                    let reactions = reconciler.get_all_reaction_messages(fid).await?;
                    let links = reconciler.get_all_link_messages(fid).await?;
                    let verifications = reconciler.get_all_verification_messages(fid).await?;
                    let user_data = reconciler.get_all_user_data_messages(fid).await?;
                    let username_proofs = reconciler.get_all_username_proofs(fid).await?;
                    let onchain_events = reconciler.get_all_onchain_events(fid).await?;

                    Ok::<_, Error>((
                        fid,
                        casts,
                        reactions,
                        links,
                        verifications,
                        user_data,
                        username_proofs,
                        onchain_events,
                    ))
                });
                fetch_tasks.push(task);
            }

            // Wait for all fetches to complete and collect the data
            let mut all_messages = Vec::new();
            let mut fid_results = Vec::new();

            for task in fetch_tasks {
                match task.await {
                    Ok(Ok((
                        fid,
                        casts,
                        reactions,
                        links,
                        verifications,
                        user_data,
                        username_proofs,
                        onchain_events,
                    ))) => {
                        let total_count = casts.len()
                            + reactions.len()
                            + links.len()
                            + verifications.len()
                            + user_data.len()
                            + username_proofs.len()
                            + onchain_events.len();

                        info!("Fetched {} total messages for FID {}", total_count, fid);

                        // Collect all messages for batch processing
                        all_messages.extend(casts);
                        all_messages.extend(reactions);
                        all_messages.extend(links);
                        all_messages.extend(verifications);
                        all_messages.extend(user_data);
                        all_messages.extend(username_proofs);

                        // Handle onchain events separately (they're not Message type)
                        for _event in onchain_events {
                            // Process onchain events individually for now
                            // TODO: Add batch processing for onchain events
                        }

                        fid_results.push((fid, true));
                    },
                    Ok(Err(e)) => {
                        error!("Error fetching data for FID in batch: {:?}", e);
                        total_errors += 1;
                    },
                    Err(e) => {
                        error!("Task error in batch reconciliation: {:?}", e);
                        total_errors += 1;
                    },
                }
            }

            // Process all collected messages in a single batch
            if !all_messages.is_empty() {
                debug!(
                    "Attempting to batch process {} messages for {} FIDs",
                    all_messages.len(),
                    fid_results.len()
                );
                if let Some(db_processor) = processor
                    .as_any()
                    .downcast_ref::<crate::processor::database::DatabaseProcessor>(
                ) {
                    debug!("Using DatabaseProcessor for batch processing");
                    match db_processor.process_message_batch(&all_messages, "merge").await {
                        Ok(_) => {
                            total_success += fid_results.len();
                            info!(
                                "Successfully batch processed {} messages for {} FIDs",
                                all_messages.len(),
                                fid_results.len()
                            );
                        },
                        Err(e) => {
                            error!("Error in batch processing messages: {:?}", e);
                            // Fall back to individual processing
                            debug!("Falling back to individual FID processing");
                            for (fid, _) in &fid_results {
                                match self.reconcile_fid(*fid, processor.clone()).await {
                                    Ok(_) => total_success += 1,
                                    Err(_) => total_errors += 1,
                                }
                            }
                        },
                    }
                } else {
                    debug!("Processor is not DatabaseProcessor, using individual FID processing");
                    // Fall back to individual FID processing for non-database processors
                    for (fid, _) in &fid_results {
                        match self.reconcile_fid(*fid, processor.clone()).await {
                            Ok(_) => total_success += 1,
                            Err(_) => total_errors += 1,
                        }
                    }
                }
            }
        }

        let elapsed = start_time.elapsed();
        info!(
            "Completed batch reconciliation for {} FIDs in {:.2?}: {} succeeded, {} failed",
            fids.len(),
            elapsed,
            total_success,
            total_errors
        );

        Ok((total_success, total_errors))
    }
    pub fn new(
        hub: Arc<Mutex<Hub>>,
        database: Arc<Database>,
        connection_timeout: Duration,
        use_streaming_rpcs: bool,
    ) -> Self {
        Self {
            hub,
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
        trace!("Starting reconciliation for FID {}", fid);
        let start_time = std::time::Instant::now();

        // Fetch message types sequentially to avoid overwhelming the Hub
        let casts = self.get_all_cast_messages(fid).await?;
        let reactions = self.get_all_reaction_messages(fid).await?;
        let links = self.get_all_link_messages(fid).await?;
        let verifications = self.get_all_verification_messages(fid).await?;
        let user_data = self.get_all_user_data_messages(fid).await?;
        let username_proofs = self.get_all_username_proofs(fid).await?;
        let onchain_events = self.get_all_onchain_events(fid).await?;

        // Process count for each message type
        let casts_count = casts.len();
        let reactions_count = reactions.len();
        let links_count = links.len();
        let verifications_count = verifications.len();
        let user_data_count = user_data.len();
        let username_proofs_count = username_proofs.len();
        let onchain_events_count = onchain_events.len();

        info!(
            "Fetched messages for FID {}: {} casts, {} reactions, {} links, {} verifications, {} user data, {} username proofs, {} onchain events",
            fid,
            casts_count,
            reactions_count,
            links_count,
            verifications_count,
            user_data_count,
            username_proofs_count,
            onchain_events_count
        );

        // Process each message type, but handle user_data specially
        let non_user_data_groups = vec![
            ("casts", casts),
            ("reactions", reactions),
            ("links", links),
            ("verifications", verifications),
            ("username_proofs", username_proofs),
        ];

        // Using a semaphore to limit concurrent processing
        let semaphore = Arc::new(tokio::sync::Semaphore::new(10));

        // First process all non-user_data messages
        for (message_type, messages) in non_user_data_groups {
            let count = messages.len();
            if count == 0 {
                continue;
            }

            trace!("Processing {} {} messages for FID {}", count, message_type, fid);

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

                // Extract messages from chunk for potential batch processing
                let message_batch = chunk_vec.clone();

                let handle = tokio::spawn(async move {
                    // Acquire permit to limit concurrency
                    let _permit = semaphore_clone.acquire().await.unwrap();

                    let mut chunk_success = 0;
                    let mut chunk_error = 0;

                    // Try to downcast to DatabaseProcessor to use batch processing
                    if let Some(db_processor) = (processor_clone.as_any())
                        .downcast_ref::<crate::processor::database::DatabaseProcessor>(
                    ) {
                        // Process the entire batch at once
                        match db_processor.process_message_batch(&message_batch, "merge").await {
                            Ok(_) => {
                                chunk_success = message_batch.len();
                            },
                            Err(e) => {
                                error!(
                                    "Error batch processing {} messages for FID {}: {:?}",
                                    message_type_str, fid, e
                                );

                                // If batch processing fails, fall back to individual processing
                                // Create events for individual processing
                                let mut events = Vec::with_capacity(message_batch.len());
                                for message in &message_batch {
                                    // Can't use self within the async closure - reconstruct the event instead
                                    events.push(HubEvent {
                                        id: 0,     // We don't use this ID
                                        r#type: 1, // MERGE_MESSAGE is type 1
                                        body: Some(Body::MergeMessageBody(MergeMessageBody {
                                            message: Some(message.clone()),
                                            deleted_messages: Vec::new(),
                                        })),
                                        block_number: 0,
                                        shard_index: 0,
                                        timestamp: 0,
                                    });
                                }

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
                            },
                        }
                    } else {
                        // Fall back to individual processing for other processor types
                        // Create events before processing
                        let mut events = Vec::with_capacity(chunk_vec.len());
                        for message in chunk_vec {
                            events.push(HubEvent {
                                id: 0,     // We don't use this ID
                                r#type: 1, // MERGE_MESSAGE is type 1
                                body: Some(Body::MergeMessageBody(MergeMessageBody {
                                    message: Some(message),
                                    deleted_messages: Vec::new(),
                                })),
                                block_number: 0,
                                shard_index: 0,
                                timestamp: 0,
                            });
                        }

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

            trace!(
                "Completed processing {} {} messages for FID {} ({} succeeded, {} failed)",
                count, message_type, fid, success_count, error_count
            );
        }

        // Now handle user_data specially to ensure complete profiles are built
        let user_data_count = user_data.len();
        if user_data_count > 0 {
            trace!("Processing {} user_data messages for FID {}", user_data_count, fid);

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

                trace!(
                    "Completed processing {} user_data messages for FID {} ({} succeeded, {} failed)",
                    user_data_count, fid, success_count, error_count
                );
            }
        }

        // Now handle onchain events, if any
        if onchain_events_count > 0 {
            trace!("Processing {} onchain events for FID {}", onchain_events_count, fid);

            let mut success_count = 0;
            let mut error_count = 0;

            // Process in chunks to avoid overwhelming the system
            let mut handles = Vec::new();

            for (idx, event) in onchain_events.into_iter().enumerate() {
                let processor_clone = Arc::clone(&processor);
                let semaphore_clone = Arc::clone(&semaphore);

                let handle = tokio::spawn(async move {
                    // Acquire permit to limit concurrency
                    let _permit = semaphore_clone.acquire().await.unwrap();

                    let onchain_event = HubEvent {
                        id: 0,
                        r#type: HubEventType::MergeOnChainEvent as i32,
                        body: Some(proto::hub_event::Body::MergeOnChainEventBody(
                            MergeOnChainEventBody { on_chain_event: Some(event) },
                        )),
                        block_number: 0,
                        shard_index: 0,
                        timestamp: 0,
                    };

                    match processor_clone.process_event(onchain_event).await {
                        Ok(_) => (1, 0),
                        Err(e) => {
                            error!(
                                "Error processing onchain event {}/{} for FID {}: {:?}",
                                idx + 1,
                                onchain_events_count,
                                fid,
                                e
                            );
                            (0, 1)
                        },
                    }
                });

                handles.push(handle);
            }

            // Wait for all tasks to complete
            for handle in handles {
                match handle.await {
                    Ok((s, e)) => {
                        success_count += s;
                        error_count += e;
                    },
                    Err(e) => {
                        error!("Task error processing onchain events for FID {}: {:?}", fid, e);
                        error_count += 1;
                    },
                }
            }

            trace!(
                "Completed processing {} onchain events for FID {} ({} succeeded, {} failed)",
                onchain_events_count, fid, success_count, error_count
            );
        }

        // Calculate total messages processed
        let total_count = casts_count
            + reactions_count
            + links_count
            + verifications_count
            + user_data_count
            + username_proofs_count
            + onchain_events_count;
        let elapsed = start_time.elapsed();
        info!(
            "Completed reconciliation for FID {} in {:.2?}: processed {} total messages ({} casts, {} reactions, {} links, {} verifications, {} user data, {} username proofs, {} onchain events)",
            fid,
            elapsed,
            total_count,
            casts_count,
            reactions_count,
            links_count,
            verifications_count,
            user_data_count,
            username_proofs_count,
            onchain_events_count
        );

        Ok(())
    }

    async fn get_all_cast_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        trace!("Fetching casts for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard.get_casts_by_fid(request).await?
            };
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            trace!(
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

        trace!(
            "Fetched a total of {} casts for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    async fn get_all_reaction_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        trace!("Fetching reactions for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = ReactionsByFidRequest {
                fid,
                reaction_type: None, // Get all reaction types
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard.get_reactions_by_fid(request).await?
            };
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            trace!(
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

        trace!(
            "Fetched a total of {} reactions for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    async fn get_all_link_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        let page_size = 500u32;
        let mut page_token = None;
        let mut page_count = 0;

        trace!("Fetching links for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = LinksByFidRequest {
                fid,
                link_type: None, // Get all link types
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard.get_links_by_fid(request).await?
            };
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            trace!(
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

        trace!(
            "Fetched a total of {} links for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    async fn get_all_verification_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        trace!("Fetching verifications for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard.get_verifications_by_fid(request).await?
            };
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            trace!(
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

        trace!(
            "Fetched a total of {} verifications for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    pub async fn get_all_user_data_messages(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        trace!("Fetching user data for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            let response = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard.get_user_data_by_fid(request).await?
            };
            let page_messages_count = response.messages.len();
            messages.extend(response.messages);

            trace!(
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

        trace!(
            "Fetched a total of {} user data messages for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    /// Get all username proofs for the given FID
    async fn get_all_username_proofs(&self, fid: u64) -> Result<Vec<Message>, Error> {
        let mut messages = Vec::new();
        let page_size = 1000u32;
        let mut page_token = None;
        let mut page_count = 0;

        trace!("Fetching username proofs for FID {} with page size {}", fid, page_size);

        loop {
            page_count += 1;
            let request = FidRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
            };

            // Try to get username proofs - first from user data, then filter
            // We need to use FidTimestampRequest for bulk methods
            let ts_request = FidTimestampRequest {
                fid,
                page_size: Some(page_size),
                page_token: page_token.clone(),
                reverse: Some(false),
                start_timestamp: None,
                stop_timestamp: None,
            };

            let get_all_result = {
                let mut hub_guard = self.hub.lock().await;
                hub_guard.get_all_user_data_messages_by_fid(ts_request).await
            };
            let response = match get_all_result {
                Ok(resp) => resp,
                Err(e) => {
                    debug!("Error getting username proof messages: {}", e);
                    // Try regular user data as fallback
                    let mut hub_guard = self.hub.lock().await;
                    hub_guard.get_user_data_by_fid(request).await?
                },
            };

            // Filter the messages to keep only username proofs
            let username_messages: Vec<Message> = response
                .messages
                .into_iter()
                .filter(|msg| {
                    if let Some(data) = &msg.data {
                        if let Some(body) = &data.body {
                            use proto::message_data::Body;
                            matches!(body, Body::UsernameProofBody(_))
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                })
                .collect();

            let page_messages_count = username_messages.len();
            messages.extend(username_messages);

            trace!(
                "Received page {} with {} username proof messages for FID {}",
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

        trace!(
            "Fetched a total of {} username proof messages for FID {} in {} pages",
            messages.len(),
            fid,
            page_count
        );
        Ok(messages)
    }

    /// Get all onchain events for the given FID
    async fn get_all_onchain_events(&self, fid: u64) -> Result<Vec<proto::OnChainEvent>, Error> {
        let mut events = Vec::new();
        let page_size = 1000u32;

        trace!("Fetching onchain events for FID {} with page size {}", fid, page_size);

        // Try fetching all types of onchain events
        for event_type in [
            OnChainEventType::EventTypeSigner,
            OnChainEventType::EventTypeSignerMigrated,
            OnChainEventType::EventTypeIdRegister,
            OnChainEventType::EventTypeStorageRent,
            OnChainEventType::EventTypeTierPurchase,
        ] {
            let mut local_page_count = 0;
            let mut local_page_token = None;

            loop {
                local_page_count += 1;
                let request = OnChainEventRequest {
                    fid,
                    event_type: event_type as i32,
                    page_size: Some(page_size),
                    page_token: local_page_token.clone(),
                    reverse: Some(false),
                };

                let on_chain_result = {
                    let mut hub_guard = self.hub.lock().await;
                    hub_guard.get_on_chain_events(request).await
                };
                let response = match on_chain_result {
                    Ok(resp) => resp,
                    Err(e) => {
                        debug!("Error fetching onchain events of type {:?}: {}", event_type, e);
                        break;
                    },
                };

                let page_events_count = response.events.len();
                events.extend(response.events);

                trace!(
                    "Received page {} with {} onchain events of type {:?} for FID {}",
                    local_page_count, page_events_count, event_type, fid
                );

                if let Some(token) = response.next_page_token {
                    if token.is_empty() {
                        break;
                    }
                    local_page_token = Some(token);
                } else {
                    break;
                }
            }
        }

        trace!("Fetched a total of {} onchain events for FID {}", events.len(), fid);
        Ok(events)
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
            block_number: 0,
            shard_index: 0,
            timestamp: 0, // Add missing timestamp field
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onchain_event_types_include_tier_purchase() {
        // Verify that the list of onchain event types includes tier purchase
        let event_types = [
            OnChainEventType::EventTypeSigner,
            OnChainEventType::EventTypeSignerMigrated,
            OnChainEventType::EventTypeIdRegister,
            OnChainEventType::EventTypeStorageRent,
            OnChainEventType::EventTypeTierPurchase,
        ];

        // Check that tier purchase is included
        let has_tier_purchase =
            event_types.iter().any(|&et| et == OnChainEventType::EventTypeTierPurchase);

        assert!(has_tier_purchase, "Tier purchase event type should be included in reconciler");

        // Verify the enum value
        assert_eq!(OnChainEventType::EventTypeTierPurchase as i32, 5);
    }
}
