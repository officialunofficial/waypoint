use crate::{
    database::client::Database,
    hub::error::Error,
    processor::consumer::EventProcessor,
    proto::{
        self, HubEvent, HubEventType, MergeMessageBody, MergeOnChainEventBody, Message, ShardChunksRequest,
    },
};
use futures::pin_mut;
use futures::stream::StreamExt;
use std::{sync::Arc, time::Duration};
use tonic::transport::Channel;
use tracing::{debug, error, info};

pub struct BlockReconciler {
    hub_client: proto::hub_service_client::HubServiceClient<Channel>,
    database: Arc<Database>,
    // We don't currently use this field but keep it for consistency with MessageReconciler
    #[allow(dead_code)]
    connection_timeout: Duration,
}

impl BlockReconciler {
    pub fn new(
        hub_client: proto::hub_service_client::HubServiceClient<Channel>,
        database: Arc<Database>,
        connection_timeout: Duration,
    ) -> Self {
        Self {
            hub_client,
            database,
            connection_timeout,
        }
    }
    
    pub async fn reconcile_block_range(
        &self,
        shard_id: u32,
        start_block: u64,
        end_block: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<(), Error> {
        info!("Reconciling blocks {} to {} for shard {}", start_block, end_block, shard_id);
        
        for block_height in start_block..=end_block {
            self.reconcile_block(shard_id, block_height, processor.clone()).await?;
        }
        
        Ok(())
    }
    
    pub async fn reconcile_block(
        &self,
        shard_id: u32,
        block_height: u64,
        processor: Arc<dyn EventProcessor>,
    ) -> Result<(), Error> {
        info!("Processing block {} for shard {}", block_height, shard_id);
        
        // Mark block as processing
        self.update_block_status(shard_id, block_height, "processing", None).await?;
        
        // Get the block data
        let block_stream = self.hub_client.clone()
            .get_blocks(tonic::Request::new(proto::BlocksRequest {
                shard_id,
                start_block_number: block_height,
                stop_block_number: Some(block_height),
            }))
            .await?
            .into_inner();
        
        // Process each block
        pin_mut!(block_stream);
        
        while let Some(block_result) = block_stream.next().await {
            match block_result {
                Ok(block) => {
                    info!("Processing block {} (hash: {})", 
                          block_height, 
                          hex::encode(&block.hash));
                    
                    // Get shard chunks for this block
                    let chunks_response = self.hub_client.clone()
                        .get_shard_chunks(tonic::Request::new(ShardChunksRequest {
                            shard_id,
                            start_block_number: block_height,
                            stop_block_number: Some(block_height),
                        }))
                        .await?;
                    
                    let mut message_count = 0;
                    let mut onchain_event_count = 0;
                    
                    // Process transactions in each shard chunk
                    for shard_chunk in chunks_response.into_inner().shard_chunks {
                        for tx in shard_chunk.transactions {
                            // Process user messages
                            for message in tx.user_messages {
                                let event = self.message_to_hub_event(message);
                                if let Err(e) = processor.process_event(event).await {
                                    error!("Failed to process message: {}", e);
                                    // Continue processing rather than stopping the entire batch
                                } else {
                                    message_count += 1;
                                }
                            }
                            
                            // Process system messages
                            for system_message in tx.system_messages {
                                // Handle on-chain events
                                if let Some(on_chain_event) = system_message.on_chain_event {
                                    let event = HubEvent {
                                        id: 0,
                                        r#type: HubEventType::MergeOnChainEvent as i32,
                                        body: Some(proto::hub_event::Body::MergeOnChainEventBody(
                                            MergeOnChainEventBody {
                                                on_chain_event: Some(on_chain_event),
                                            }
                                        )),
                                    };
                                    if let Err(e) = processor.process_event(event).await {
                                        error!("Failed to process onchain event: {}", e);
                                        // Continue processing rather than stopping the entire batch
                                    } else {
                                        onchain_event_count += 1;
                                    }
                                }
                            }
                        }
                    }
                    
                    info!("Processed block {} with {} messages and {} onchain events", 
                          block_height, message_count, onchain_event_count);
                    
                    // Mark block as completed
                    self.update_block_status(shard_id, block_height, "completed", None).await?;
                },
                Err(e) => {
                    error!("Error processing block {}: {:?}", block_height, e);
                    self.update_block_status(
                        shard_id, 
                        block_height, 
                        "failed", 
                        Some(e.to_string())
                    ).await?;
                    return Err(Error::from(e));
                }
            }
        }
        
        Ok(())
    }
    
    async fn update_block_status(
        &self, 
        shard_id: u32,
        block_height: u64,
        status: &str,
        error_message: Option<String>,
    ) -> Result<(), Error> {
        // Database operation to update block status
        let query = r#"
            INSERT INTO block_sync_state (shard_id, block_height, processed_at, status, error_message)
            VALUES ($1, $2, NOW(), $3, $4)
            ON CONFLICT (shard_id, block_height) 
            DO UPDATE SET 
                processed_at = NOW(),
                status = $3,
                error_message = $4
            "#;
        
        let result = sqlx::query(query)
            .bind(shard_id as i32)
            .bind(block_height as i64)
            .bind(status)
            .bind(error_message)
            .execute(&self.database.pool)
            .await;
        
        match result {
            Ok(_) => {
                debug!("Updated block {} with status: {}", block_height, status);
                Ok(())
            },
            Err(e) => {
                error!("Failed to update block status: {:?}", e);
                // Log the error but continue processing - non-fatal
                Ok(())
            }
        }
    }
    
    // Helper function similar to the one in MessageReconciler
    fn message_to_hub_event(&self, message: Message) -> HubEvent {
        // Same implementation as in MessageReconciler
        let merge_message_body = 
            MergeMessageBody { message: Some(message), deleted_messages: Vec::new() };

        HubEvent {
            id: 0, 
            r#type: HubEventType::MergeMessage as i32,
            body: Some(proto::hub_event::Body::MergeMessageBody(merge_message_body)),
        }
    }
    
    // Get latest block height from the Hub
    pub async fn get_latest_block_height(&self, shard_id: u32) -> Result<u64, Error> {
        let response = self.hub_client.clone()
            .get_info(tonic::Request::new(proto::GetInfoRequest {}))
            .await?;
            
        let info = response.into_inner();
        
        // Find the shard info for the specified shard
        for shard_info in info.shard_infos {
            if shard_info.shard_id == shard_id {
                return Ok(shard_info.max_height);
            }
        }
        
        // Default to 0 if we can't find the shard
        error!("Could not find information for shard {}", shard_id);
        Ok(0)
    }
    
    // Get a list of blocks in given range that need processing
    pub async fn get_unprocessed_blocks(
        &self,
        shard_id: u32,
        start_block: u64,
        end_block: u64,
        limit: usize,
    ) -> Result<Vec<u64>, Error> {
        let query = r#"
            WITH block_range AS (
                SELECT generate_series($1::bigint, $2::bigint) as block_height
            )
            SELECT block_height FROM block_range
            WHERE NOT EXISTS (
                SELECT 1 FROM block_sync_state 
                WHERE shard_id = $3 
                AND block_height = block_range.block_height
                AND status = 'completed'
            )
            ORDER BY block_height
            LIMIT $4
            "#;
        
        let result = sqlx::query_as::<_, (i64,)>(query)
            .bind(start_block as i64)
            .bind(end_block as i64)
            .bind(shard_id as i32)
            .bind(limit as i32)
            .fetch_all(&self.database.pool)
            .await;
        
        match result {
            Ok(rows) => {
                let blocks: Vec<u64> = rows.iter()
                    .map(|row| row.0 as u64)
                    .collect();
                
                debug!("Found {} unprocessed blocks for shard {}", blocks.len(), shard_id);
                Ok(blocks)
            },
            Err(e) => {
                error!("Failed to get unprocessed blocks: {:?}", e);
                Ok(vec![]) // Return empty vector instead of error
            }
        }
    }
}