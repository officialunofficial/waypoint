//! User and verification query operations.

use crate::core::types::{Fid, Message, MessageType};
use crate::query::WaypointQuery;
use crate::query::types::TimeRange;

use prost::Message as ProstMessage;

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get user data by FID
    pub async fn do_get_user_by_fid(&self, fid: Fid) -> String {
        match self.data_context.get_user_data_by_fid(fid, 20).await {
            Ok(messages) => {
                if messages.is_empty() {
                    return format!("No user data found for FID {}", fid);
                }

                let mut profile = serde_json::Map::new();
                profile.insert(
                    "fid".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(fid.value())),
                );

                for message in messages {
                    if message.message_type != MessageType::UserData {
                        continue;
                    }

                    if let Ok(msg_data) =
                        <crate::proto::MessageData as ProstMessage>::decode(&*message.payload)
                        && let Some(crate::proto::message_data::Body::UserDataBody(user_data)) =
                            msg_data.body
                    {
                        let field_name = match user_data.r#type {
                            1 => "pfp",
                            2 => "display_name",
                            3 => "bio",
                            5 => "url",
                            6 => "username",
                            7 => "location",
                            8 => "twitter",
                            9 => "github",
                            _ => continue,
                        };

                        profile.insert(
                            field_name.to_string(),
                            serde_json::Value::String(user_data.value),
                        );
                    }
                }

                serde_json::to_string_pretty(&profile)
                    .unwrap_or_else(|_| format!("Error formatting user data for FID {}", fid))
            },
            Err(e) => format!("Error fetching user data: {}", e),
        }
    }

    fn protocol_name(protocol: i32) -> &'static str {
        match protocol {
            0 => "ethereum",
            1 => "solana",
            _ => "unknown",
        }
    }

    fn verification_type_name(verification_type: u32) -> &'static str {
        match verification_type {
            0 => "eoa",
            1 => "contract",
            _ => "unknown",
        }
    }

    fn proof_type_name(proof_type: i32) -> &'static str {
        match proof_type {
            1 => "fname",
            2 => "ens_l1",
            3 => "basename",
            _ => "unknown",
        }
    }

    fn username_proof_to_json(proof: &crate::proto::UserNameProof) -> serde_json::Value {
        serde_json::json!({
            "name": String::from_utf8_lossy(&proof.name),
            "type": Self::proof_type_name(proof.r#type),
            "fid": proof.fid,
            "timestamp": proof.timestamp,
            "owner": format!("0x{}", hex::encode(&proof.owner)),
        })
    }

    fn verification_message_to_json(message: &Message) -> Option<serde_json::Value> {
        if message.message_type != MessageType::Verification {
            return None;
        }

        let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload).ok()?;

        match msg_data.body {
            Some(crate::proto::message_data::Body::VerificationAddAddressBody(verification)) => {
                let mut value = serde_json::json!({
                    "fid": msg_data.fid,
                    "address": format!("0x{}", hex::encode(&verification.address)),
                    "protocol": Self::protocol_name(verification.protocol),
                    "type": Self::verification_type_name(verification.verification_type),
                    "action": "add",
                    "timestamp": msg_data.timestamp,
                });

                if verification.chain_id > 0 {
                    value["chain_id"] = serde_json::json!(verification.chain_id);
                }

                Some(value)
            },
            Some(crate::proto::message_data::Body::VerificationRemoveBody(verification)) => {
                Some(serde_json::json!({
                    "fid": msg_data.fid,
                    "address": format!("0x{}", hex::encode(&verification.address)),
                    "protocol": Self::protocol_name(verification.protocol),
                    "action": "remove",
                    "timestamp": msg_data.timestamp,
                }))
            },
            _ => None,
        }
    }

    /// Get user verifications by FID
    pub async fn do_get_verifications_by_fid(&self, fid: Fid, limit: usize) -> String {
        tracing::debug!("Query: Fetching verifications for FID: {}", fid);

        match self.data_context.get_verifications_by_fid(fid, limit).await {
            Ok(messages) => {
                if messages.is_empty() {
                    return format!("No verifications found for FID {}", fid);
                }

                let verifications: Vec<serde_json::Value> =
                    messages.iter().filter_map(Self::verification_message_to_json).collect();

                let result = serde_json::json!({
                    "fid": fid.value(),
                    "count": verifications.len(),
                    "verifications": verifications
                });

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Error formatting verifications for FID {}", fid))
            },
            Err(e) => format!("Error fetching verifications: {}", e),
        }
    }

    /// Get a single verification by FID and address
    pub async fn do_get_verification(&self, fid: Fid, address_hex: &str) -> String {
        tracing::debug!(
            "Query: Fetching verification for FID: {} and address: {}",
            fid,
            address_hex
        );

        let address = address_hex.trim_start_matches("0x");
        if address.is_empty() {
            return "Invalid address format: empty address".to_string();
        }

        let address_bytes = match hex::decode(address) {
            Ok(bytes) => bytes,
            Err(_) => return format!("Invalid address format: {}", address_hex),
        };

        match self.data_context.get_verification(fid, &address_bytes).await {
            Ok(Some(message)) => match Self::verification_message_to_json(&message) {
                Some(verification) => {
                    let result = serde_json::json!({
                        "fid": fid.value(),
                        "address": format!("0x{}", address),
                        "found": true,
                        "verification": verification,
                    });
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                        format!("Error formatting verification for FID {}", fid)
                    })
                },
                None => format!(
                    "Verification found but could not be processed for FID {} and address {}",
                    fid, address_hex
                ),
            },
            Ok(None) => {
                let result = serde_json::json!({
                    "fid": fid.value(),
                    "address": format!("0x{}", address),
                    "found": false,
                    "error": "Verification not found",
                });

                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("No verification found for FID {} and address {}", fid, address_hex)
                })
            },
            Err(e) => format!("Error fetching verification: {}", e),
        }
    }

    /// Get all verification messages by FID with timestamp filtering
    pub async fn do_get_all_verification_messages_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> String {
        tracing::debug!(
            "Query: Fetching all verification messages for FID: {} with time filtering",
            fid
        );

        match self
            .data_context
            .get_all_verification_messages_by_fid(fid, limit, start_time, end_time)
            .await
        {
            Ok(messages) => {
                let verifications: Vec<serde_json::Value> =
                    messages.iter().filter_map(Self::verification_message_to_json).collect();

                if verifications.is_empty() {
                    let time_range = TimeRange::new(start_time, end_time).describe();

                    return format!("No verification messages found for FID {}{}", fid, time_range);
                }

                let result = serde_json::json!({
                    "fid": fid.value(),
                    "count": verifications.len(),
                    "start_time": start_time,
                    "end_time": end_time,
                    "verifications": verifications,
                });

                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("Error formatting verification messages for FID {}", fid)
                })
            },
            Err(e) => format!("Error fetching verification messages: {}", e),
        }
    }

    /// Find FID by username
    pub async fn do_get_fid_by_username(&self, username: &str) -> String {
        match self.data_context.get_fid_by_username(username).await {
            Ok(Some(fid)) => {
                // Create a structured response with FID information
                let result = serde_json::json!({
                    "username": username,
                    "fid": fid.value(),
                    "found": true
                });

                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("Error formatting response for username {}", username)
                })
            },
            Ok(None) => {
                // Return a JSON response indicating the username was not found
                let result = serde_json::json!({
                    "username": username,
                    "found": false,
                    "error": "Username not found"
                });

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Username not found: {}", username))
            },
            Err(e) => {
                // Return a JSON response with the error
                let result = serde_json::json!({
                    "username": username,
                    "found": false,
                    "error": format!("Error: {}", e)
                });

                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("Error fetching FID for username {}: {}", username, e)
                })
            },
        }
    }

    /// Get user profile by username
    pub async fn do_get_user_by_username(&self, username: &str) -> String {
        // First find the FID for this username
        match self.data_context.get_fid_by_username(username).await {
            Ok(Some(fid)) => {
                // Now fetch the user data for this FID
                match self.data_context.get_user_data_by_fid(fid, 20).await {
                    Ok(messages) => {
                        if messages.is_empty() {
                            return format!(
                                "Username '{}' resolved to FID {}, but no user data found",
                                username, fid
                            );
                        }

                        // Create a structured user profile from the messages
                        let mut profile = serde_json::Map::new();

                        // Add the FID and username to the profile
                        profile.insert(
                            "fid".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(fid.value())),
                        );

                        profile.insert(
                            "username".to_string(),
                            serde_json::Value::String(username.to_string()),
                        );

                        // Process each message to extract user data
                        for message in messages {
                            if message.message_type != crate::core::types::MessageType::UserData {
                                continue;
                            }

                            // Try to decode the message payload as MessageData
                            if let Ok(data) = prost::Message::decode(&*message.payload) {
                                let msg_data: crate::proto::MessageData = data;

                                // Extract the user_data_body if present
                                if let Some(crate::proto::message_data::Body::UserDataBody(
                                    user_data,
                                )) = msg_data.body
                                {
                                    // Map user data type to field name
                                    let field_name = match user_data.r#type {
                                        1 => "pfp",          // USER_DATA_TYPE_PFP
                                        2 => "display_name", // USER_DATA_TYPE_DISPLAY
                                        3 => "bio",          // USER_DATA_TYPE_BIO
                                        5 => "url",          // USER_DATA_TYPE_URL
                                        6 => "username", // USER_DATA_TYPE_USERNAME (already set above, but include for completeness)
                                        7 => "location", // USER_DATA_TYPE_LOCATION
                                        8 => "twitter",  // USER_DATA_TYPE_TWITTER
                                        9 => "github",   // USER_DATA_TYPE_GITHUB
                                        _ => continue,   // Unknown type
                                    };

                                    // Add to the profile
                                    profile.insert(
                                        field_name.to_string(),
                                        serde_json::Value::String(user_data.value),
                                    );
                                }
                            }
                        }

                        // Convert the profile to a JSON string
                        serde_json::to_string_pretty(&profile).unwrap_or_else(|_| {
                            format!("Error formatting user data for username {}", username)
                        })
                    },
                    Err(e) => format!("Error fetching user data: {}", e),
                }
            },
            Ok(None) => {
                // Return a JSON response indicating the username was not found
                let result = serde_json::json!({
                    "username": username,
                    "found": false,
                    "error": "Username not found"
                });

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Username not found: {}", username))
            },
            Err(e) => {
                // Return a JSON response with the error
                let result = serde_json::json!({
                    "username": username,
                    "found": false,
                    "error": format!("Error: {}", e)
                });

                serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
                    format!("Error fetching user by username {}: {}", username, e)
                })
            },
        }
    }

    /// Get username proofs by FID
    pub async fn do_get_username_proofs_by_fid(&self, fid: Fid) -> String {
        tracing::debug!("Query: Fetching username proofs for FID: {}", fid);

        match self.data_context.get_username_proofs_by_fid(fid).await {
            Ok(messages) => {
                if messages.is_empty() {
                    return format!("No username proofs found for FID {}", fid);
                }

                let proofs: Vec<serde_json::Value> = messages
                    .iter()
                    .filter_map(|message| {
                        serde_json::from_slice::<crate::proto::UserNameProof>(&message.payload)
                            .ok()
                            .map(|proof| Self::username_proof_to_json(&proof))
                    })
                    .collect();

                let result = serde_json::json!({
                    "fid": fid.value(),
                    "count": proofs.len(),
                    "proofs": proofs
                });

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Error formatting username proofs for FID {}", fid))
            },
            Err(e) => format!("Error fetching username proofs: {}", e),
        }
    }

    /// Get a single username proof by name
    pub async fn do_get_username_proof(&self, name: &str) -> String {
        tracing::debug!("Query: Fetching username proof for name: {}", name);

        match self.data_context.get_username_proof_by_name(name).await {
            Ok(Some(proof)) => {
                let mut result = Self::username_proof_to_json(&proof);
                result["found"] = serde_json::json!(true);

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Error formatting username proof for {}", name))
            },
            Ok(None) => {
                let result = serde_json::json!({
                    "name": name,
                    "found": false,
                    "error": "Username proof not found",
                });

                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Username proof not found for {}", name))
            },
            Err(e) => format!("Error fetching username proof: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data_context::{DataAccessError, DataContextBuilder};
    use crate::core::types::MessageId;
    use async_trait::async_trait;

    #[derive(Clone, Debug)]
    struct MockDb;

    #[async_trait]
    impl crate::core::data_context::Database for MockDb {
        async fn get_message(
            &self,
            _id: &MessageId,
            _message_type: MessageType,
        ) -> crate::core::data_context::Result<Message> {
            Err(DataAccessError::Other("mock".into()))
        }

        async fn get_messages_by_fid(
            &self,
            _fid: Fid,
            _message_type: MessageType,
            _limit: usize,
            _cursor: Option<MessageId>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn store_message(&self, _message: Message) -> crate::core::data_context::Result<()> {
            Ok(())
        }

        async fn delete_message(
            &self,
            _id: &MessageId,
            _message_type: MessageType,
        ) -> crate::core::data_context::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Debug, Default)]
    struct MockHub {
        verification: Option<Message>,
    }

    #[async_trait]
    impl crate::core::data_context::HubClient for MockHub {
        async fn get_user_data_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_user_data(
            &self,
            _fid: Fid,
            _data_type: &str,
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }

        async fn get_username_proofs_by_fid(
            &self,
            _fid: Fid,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_username_proof_by_name(
            &self,
            _name: &str,
        ) -> crate::core::data_context::Result<Option<crate::proto::UserNameProof>> {
            Ok(None)
        }

        async fn get_verifications_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_verification(
            &self,
            _fid: Fid,
            _address: &[u8],
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(self.verification.clone())
        }

        async fn get_casts_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_cast(
            &self,
            _fid: Fid,
            _hash: &[u8],
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }

        async fn get_casts_by_mention(
            &self,
            _fid: Fid,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_casts_by_parent(
            &self,
            _parent_fid: Fid,
            _parent_hash: &[u8],
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_casts_by_parent_url(
            &self,
            _parent_url: &str,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_all_casts_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_reaction(
            &self,
            _fid: Fid,
            _reaction_type: u8,
            _target_cast_fid: Option<Fid>,
            _target_cast_hash: Option<&[u8]>,
            _target_url: Option<&str>,
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }

        async fn get_reactions_by_fid(
            &self,
            _fid: Fid,
            _reaction_type: Option<u8>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_reactions_by_target(
            &self,
            _target_cast_fid: Option<Fid>,
            _target_cast_hash: Option<&[u8]>,
            _target_url: Option<&str>,
            _reaction_type: Option<u8>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_all_reactions_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_all_verification_messages_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_link(
            &self,
            _fid: Fid,
            _link_type: &str,
            _target_fid: Fid,
        ) -> crate::core::data_context::Result<Option<Message>> {
            Ok(None)
        }

        async fn get_links_by_fid(
            &self,
            _fid: Fid,
            _link_type: Option<&str>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_links_by_target(
            &self,
            _target_fid: Fid,
            _link_type: Option<&str>,
            _limit: usize,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_link_compact_state_by_fid(
            &self,
            _fid: Fid,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }

        async fn get_all_links_by_fid(
            &self,
            _fid: Fid,
            _limit: usize,
            _start_time: Option<u64>,
            _end_time: Option<u64>,
        ) -> crate::core::data_context::Result<Vec<Message>> {
            Ok(vec![])
        }
    }

    type TestService = WaypointQuery<MockDb, MockHub>;

    fn make_service(mock_hub: MockHub) -> TestService {
        let data_context =
            DataContextBuilder::new().with_database(MockDb).with_hub_client(mock_hub).build();
        WaypointQuery::new(data_context)
    }

    fn make_verification_add_message(
        fid: u64,
        address: &[u8],
        protocol: i32,
        verification_type: u32,
        chain_id: u32,
        timestamp: u32,
    ) -> Message {
        let data = crate::proto::MessageData {
            r#type: crate::proto::MessageType::VerificationAddEthAddress as i32,
            fid,
            timestamp,
            network: crate::proto::FarcasterNetwork::Mainnet as i32,
            body: Some(crate::proto::message_data::Body::VerificationAddAddressBody(
                crate::proto::VerificationAddAddressBody {
                    address: address.to_vec(),
                    claim_signature: vec![],
                    block_hash: vec![],
                    verification_type,
                    chain_id,
                    protocol,
                },
            )),
        };

        Message::new("verification-add", MessageType::Verification, data.encode_to_vec())
    }

    fn make_verification_remove_message(
        fid: u64,
        address: &[u8],
        protocol: i32,
        timestamp: u32,
    ) -> Message {
        let data = crate::proto::MessageData {
            r#type: crate::proto::MessageType::VerificationRemove as i32,
            fid,
            timestamp,
            network: crate::proto::FarcasterNetwork::Mainnet as i32,
            body: Some(crate::proto::message_data::Body::VerificationRemoveBody(
                crate::proto::VerificationRemoveBody { address: address.to_vec(), protocol },
            )),
        };

        Message::new("verification-remove", MessageType::Verification, data.encode_to_vec())
    }

    #[test]
    fn test_verification_message_to_json_add_includes_chain_id() {
        let message = make_verification_add_message(12345, &[0x1a, 0x2b], 0, 1, 10, 1672531200);

        let result = TestService::verification_message_to_json(&message).unwrap();
        assert_eq!(result["fid"], 12345);
        assert_eq!(result["address"], "0x1a2b");
        assert_eq!(result["protocol"], "ethereum");
        assert_eq!(result["type"], "contract");
        assert_eq!(result["action"], "add");
        assert_eq!(result["chain_id"], 10);
        assert_eq!(result["timestamp"], 1672531200);
    }

    #[test]
    fn test_verification_message_to_json_add_omits_chain_id_when_zero() {
        let message = make_verification_add_message(12345, &[0xaa, 0xbb], 0, 0, 0, 1672531200);

        let result = TestService::verification_message_to_json(&message).unwrap();
        let obj = result.as_object().unwrap();
        assert!(!obj.contains_key("chain_id"));
    }

    #[test]
    fn test_verification_message_to_json_remove() {
        let message = make_verification_remove_message(12345, &[0xde, 0xad], 1, 1672617600);

        let result = TestService::verification_message_to_json(&message).unwrap();
        assert_eq!(result["fid"], 12345);
        assert_eq!(result["address"], "0xdead");
        assert_eq!(result["protocol"], "solana");
        assert_eq!(result["action"], "remove");
        assert_eq!(result["timestamp"], 1672617600);
    }

    #[test]
    fn test_verification_message_to_json_invalid_payload_returns_none() {
        let message =
            Message::new("invalid", MessageType::Verification, vec![0xde, 0xad, 0xbe, 0xef]);
        assert!(TestService::verification_message_to_json(&message).is_none());
    }

    #[tokio::test]
    async fn test_do_get_verification_invalid_empty_address() {
        let service = make_service(MockHub::default());

        let result = service.do_get_verification(Fid::from(12345), "0x").await;
        assert_eq!(result, "Invalid address format: empty address");
    }

    #[tokio::test]
    async fn test_do_get_verification_invalid_hex_address() {
        let service = make_service(MockHub::default());

        let result = service.do_get_verification(Fid::from(12345), "0xzz").await;
        assert_eq!(result, "Invalid address format: 0xzz");
    }

    #[tokio::test]
    async fn test_do_get_verification_not_found_returns_found_false_json() {
        let service = make_service(MockHub::default());

        let result = service.do_get_verification(Fid::from(12345), "0x00112233").await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["fid"], 12345);
        assert_eq!(parsed["address"], "0x00112233");
        assert_eq!(parsed["found"], false);
        assert_eq!(parsed["error"], "Verification not found");
    }

    #[tokio::test]
    async fn test_do_get_verification_found_returns_verification_json() {
        let message = make_verification_add_message(12345, &[0xaa, 0xbb], 0, 0, 0, 1672531200);
        let service = make_service(MockHub { verification: Some(message) });

        let result = service.do_get_verification(Fid::from(12345), "0xaabb").await;
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["fid"], 12345);
        assert_eq!(parsed["address"], "0xaabb");
        assert_eq!(parsed["found"], true);
        assert_eq!(parsed["verification"]["action"], "add");
        assert_eq!(parsed["verification"]["protocol"], "ethereum");
        assert_eq!(parsed["verification"]["type"], "eoa");
    }
}
