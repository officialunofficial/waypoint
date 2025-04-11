//! Tests for the repositories

#[cfg(test)]
mod tests {
    use crate::core::{
        Fid, Message, MessageId, MessageType,
        repository::{MessageRepository, RepositoryError, Result},
    };
    use async_trait::async_trait;
    use mockall::mock;
    use mockall::predicate::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    // Mock implementation of the MessageRepository
    mock! {
        pub MessageRepo {}

        #[async_trait]
        impl MessageRepository for MessageRepo {
            async fn store_message(&self, message: Message) -> Result<()>;
            async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message>;
            async fn get_messages_by_fid(&self, fid: Fid, message_type: MessageType, limit: usize, cursor: Option<MessageId>) -> Result<Vec<Message>>;
            async fn delete_message(&self, id: &MessageId, message_type: MessageType) -> Result<()>;
        }
    }

    // In-memory implementation of the repository for testing
    struct InMemoryMessageRepository {
        messages: Arc<Mutex<HashMap<(String, MessageType), Message>>>,
    }

    impl InMemoryMessageRepository {
        fn new() -> Self {
            Self { messages: Arc::new(Mutex::new(HashMap::new())) }
        }
    }

    #[async_trait]
    impl MessageRepository for InMemoryMessageRepository {
        async fn store_message(&self, message: Message) -> Result<()> {
            let mut messages = self.messages.lock().unwrap();
            let key = (message.id.value().to_string(), message.message_type);
            messages.insert(key, message);
            Ok(())
        }

        async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message> {
            let messages = self.messages.lock().unwrap();
            let key = (id.value().to_string(), message_type);

            messages
                .get(&key)
                .cloned()
                .ok_or_else(|| RepositoryError::NotFound(format!("Message not found: {}", id)))
        }

        async fn get_messages_by_fid(
            &self,
            _fid: Fid,
            message_type: MessageType,
            limit: usize,
            cursor: Option<MessageId>,
        ) -> Result<Vec<Message>> {
            let messages = self.messages.lock().unwrap();

            // In a real implementation, we would filter by FID
            // This is a simplified implementation that just returns all messages of the given type
            let mut result: Vec<Message> =
                messages.values().filter(|m| m.message_type == message_type).cloned().collect();

            // Apply cursor if provided
            if let Some(cursor) = cursor {
                result.retain(|m| m.id.value() > cursor.value());
            }

            // Apply limit
            if result.len() > limit {
                result.truncate(limit);
            }

            Ok(result)
        }

        async fn delete_message(&self, id: &MessageId, message_type: MessageType) -> Result<()> {
            let mut messages = self.messages.lock().unwrap();
            let key = (id.value().to_string(), message_type);

            if messages.remove(&key).is_some() {
                Ok(())
            } else {
                Err(RepositoryError::NotFound(format!("Message not found: {}", id)))
            }
        }
    }

    // Sample service that uses the repository
    struct MessageService<T: MessageRepository> {
        repository: T,
    }

    impl<T: MessageRepository> MessageService<T> {
        fn new(repository: T) -> Self {
            Self { repository }
        }

        async fn get_message(&self, id: &MessageId, message_type: MessageType) -> Result<Message> {
            self.repository.get_message(id, message_type).await
        }
    }

    #[tokio::test]
    async fn test_in_memory_repository() {
        // Create the repository
        let repo = InMemoryMessageRepository::new();

        // Create a message
        let message = Message::new("test1", MessageType::Cast, vec![1, 2, 3, 4]);

        // Store the message
        repo.store_message(message.clone()).await.unwrap();

        // Retrieve the message
        let retrieved = repo.get_message(&message.id, message.message_type).await.unwrap();

        // Check that it's the same
        assert_eq!(retrieved.id.value(), message.id.value());
        assert_eq!(retrieved.message_type, message.message_type);
        assert_eq!(retrieved.payload, message.payload);

        // Delete the message
        repo.delete_message(&message.id, message.message_type).await.unwrap();

        // Check that it's gone
        let result = repo.get_message(&message.id, message.message_type).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_message_service_with_mock() {
        // Create a mock repository
        let mut mock_repo = MockMessageRepo::new();

        // Set up the mock expectations
        let message_id = MessageId::new("test1");
        let message_type = MessageType::Cast;
        let expected_message = Message::new(message_id.clone(), message_type, vec![1, 2, 3, 4]);

        // Mock the get_message method
        mock_repo
            .expect_get_message()
            .with(eq(message_id.clone()), eq(message_type))
            .times(1)
            .returning(move |_, _| Ok(Message::new("test1", MessageType::Cast, vec![1, 2, 3, 4])));

        // Create the service with the mock repository
        let service = MessageService::new(mock_repo);

        // Call the service method
        let result = service.get_message(&message_id, message_type).await.unwrap();

        // Verify the result
        assert_eq!(result.id.value(), expected_message.id.value());
        assert_eq!(result.message_type, expected_message.message_type);
        assert_eq!(result.payload, expected_message.payload);
    }
}
