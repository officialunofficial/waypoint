//! Event processor abstractions and registry
use crate::{
    app::AppState,
    core::MessageType,
    proto::{self, HubEvent, HubEventType, MessageType as ProtoMessageType},
};
use async_trait::async_trait;
use prost::Message as ProstMessage;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tracing::warn;

/// Event processor error
#[derive(Error, Debug)]
pub enum ProcessorError {
    #[error("Processor not registered for message type: {0:?}")]
    NotRegistered(MessageType),

    #[error("Failed to process event: {0}")]
    Processing(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Decoding error: {0}")]
    Decoding(#[from] prost::DecodeError),
}

/// Result type for processor operations
pub type Result<T> = std::result::Result<T, ProcessorError>;

/// Trait for event processors
#[async_trait]
pub trait EventProcessor: Send + Sync {
    /// Get the name of the processor
    fn name(&self) -> &str;

    /// Process an event
    async fn process(&self, event: HubEvent) -> Result<()>;

    /// Get message types supported by this processor
    fn supported_types(&self) -> Vec<MessageType>;
}

/// Registry for event processors
pub struct ProcessorRegistry {
    state: Arc<AppState>,
    processors: HashMap<MessageType, Vec<Arc<dyn EventProcessor>>>,
}

impl ProcessorRegistry {
    /// Create a new processor registry
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state, processors: HashMap::new() }
    }

    /// Get the application state
    pub fn state(&self) -> &Arc<AppState> {
        &self.state
    }

    /// Register a processor for specific message types
    pub fn register<P: EventProcessor + 'static>(&mut self, processor: P) -> &mut Self {
        let processor = Arc::new(processor);

        for message_type in processor.supported_types() {
            self.processors.entry(message_type).or_default().push(processor.clone());
        }

        self
    }

    /// Process an event with all registered processors
    pub async fn process_event(&self, event: HubEvent) -> Result<()> {
        use proto::hub_event::Body;

        // Map the protobuf event type to our domain message type
        let message_type = match event.r#type {
            // Handle message events based on the message type inside
            i if i == HubEventType::MergeMessage as i32 => {
                if let Some(Body::MergeMessageBody(body)) = &event.body {
                    if let Some(message) = &body.message {
                        if let Some(data) = &message.data {
                            match data.r#type {
                                i if i == ProtoMessageType::CastAdd as i32 => MessageType::Cast,
                                i if i == ProtoMessageType::CastRemove as i32 => MessageType::Cast,
                                i if i == ProtoMessageType::ReactionAdd as i32 => {
                                    MessageType::Reaction
                                },
                                i if i == ProtoMessageType::ReactionRemove as i32 => {
                                    MessageType::Reaction
                                },
                                i if i == ProtoMessageType::LinkAdd as i32 => MessageType::Link,
                                i if i == ProtoMessageType::LinkRemove as i32 => MessageType::Link,
                                i if i == ProtoMessageType::VerificationAddEthAddress as i32 => {
                                    MessageType::Verification
                                },
                                i if i == ProtoMessageType::VerificationRemove as i32 => {
                                    MessageType::Verification
                                },
                                i if i == ProtoMessageType::UserDataAdd as i32 => {
                                    MessageType::UserData
                                },
                                i if i == ProtoMessageType::LendStorage as i32 => {
                                    MessageType::LendStorage
                                },
                                _ => return Ok(()), // Ignore unknown message types
                            }
                        } else {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            },
            // Handle other event types directly
            i if i == HubEventType::MergeOnChainEvent as i32 => MessageType::OnchainSigner,
            i if i == HubEventType::MergeUsernameProof as i32 => MessageType::UsernameProof,
            i if i == HubEventType::PruneMessage as i32 => return Ok(()), // Ignore prune events
            _ => return Ok(()),                                           // Ignore unknown events
        };

        // Get processors for this message type
        let processors = match self.processors.get(&message_type) {
            Some(p) => p,
            None => {
                warn!("No processors registered for message type: {:?}", message_type);
                return Ok(());
            },
        };

        // Process the event with all registered processors
        for processor in processors {
            processor.process(event.clone()).await?;
        }

        Ok(())
    }

    /// Decode and process a raw event message
    pub async fn process_raw_event(&self, data: &[u8]) -> Result<()> {
        let event = proto::HubEvent::decode(data)?;
        self.process_event(event).await
    }
}
