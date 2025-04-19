//! MCP handlers for User Data operations

use crate::core::types::{Fid, MessageType};
use crate::services::mcp::base::WaypointMcpService;

use prost::Message as ProstMessage;

// Common types are used in the handler implementations

impl<DB, HC> WaypointMcpService<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    /// Get user data by FID using Hub client
    pub async fn do_get_user_by_fid(&self, fid: Fid) -> String {
        // Use the data context to fetch user data
        match self.data_context.get_user_data_by_fid(fid, 20).await {
            Ok(messages) => {
                if messages.is_empty() {
                    return format!("No user data found for FID {}", fid);
                }

                // Create a structured user profile from the messages
                let mut profile = serde_json::Map::new();

                // Add the FID to the profile
                profile.insert(
                    "fid".to_string(),
                    serde_json::Value::Number(serde_json::Number::from(fid.value())),
                );

                // Process each message to extract user data
                for message in messages {
                    if message.message_type != MessageType::UserData {
                        continue;
                    }

                    // Try to decode the message payload as MessageData
                    if let Ok(data) = ProstMessage::decode(&*message.payload) {
                        let msg_data: crate::proto::MessageData = data;

                        // Extract the user_data_body if present
                        if let Some(crate::proto::message_data::Body::UserDataBody(user_data)) =
                            msg_data.body
                        {
                            // Map user data type to field name
                            let field_name = match user_data.r#type {
                                1 => "pfp",          // USER_DATA_TYPE_PFP
                                2 => "display_name", // USER_DATA_TYPE_DISPLAY
                                3 => "bio",          // USER_DATA_TYPE_BIO
                                5 => "url",          // USER_DATA_TYPE_URL
                                6 => "username",     // USER_DATA_TYPE_USERNAME
                                7 => "location",     // USER_DATA_TYPE_LOCATION
                                8 => "twitter",      // USER_DATA_TYPE_TWITTER
                                9 => "github",       // USER_DATA_TYPE_GITHUB
                                _ => continue,       // Unknown type
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
                serde_json::to_string_pretty(&profile)
                    .unwrap_or_else(|_| format!("Error formatting user data for FID {}", fid))
            },
            Err(e) => format!("Error fetching user data: {}", e),
        }
    }

    /// Get user verifications by FID
    pub async fn do_get_verifications_by_fid(&self, fid: Fid, limit: usize) -> String {
        tracing::info!("MCP: Fetching verifications for FID: {}", fid);

        // Use the data context to fetch verifications
        match self.data_context.get_verifications_by_fid(fid, limit).await {
            Ok(messages) => {
                if messages.is_empty() {
                    return format!("No verifications found for FID {}", fid);
                }

                // Create a structured array of verification objects
                let mut verifications = Vec::new();

                // Process each verification message
                for message in messages {
                    if message.message_type != MessageType::Verification {
                        continue;
                    }

                    // Try to decode the message payload as MessageData
                    if let Ok(data) = ProstMessage::decode(&*message.payload) {
                        let msg_data: crate::proto::MessageData = data;

                        // Extract verification data
                        if let Some(crate::proto::message_data::Body::VerificationAddAddressBody(
                            verification,
                        )) = msg_data.body
                        {
                            // Create verification object
                            let mut verif_obj = serde_json::Map::new();

                            // Format the address as hex
                            let address_hex = format!("0x{}", hex::encode(&verification.address));

                            // Add base verification info
                            verif_obj.insert(
                                "fid".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(fid.value())),
                            );
                            verif_obj.insert(
                                "address".to_string(),
                                serde_json::Value::String(address_hex),
                            );

                            // Add protocol info
                            let protocol = match verification.protocol {
                                0 => "ethereum",
                                1 => "solana",
                                _ => "unknown",
                            };
                            verif_obj.insert(
                                "protocol".to_string(),
                                serde_json::Value::String(protocol.to_string()),
                            );

                            // Add verification type
                            let verif_type = match verification.verification_type {
                                0 => "eoa",      // Externally Owned Account
                                1 => "contract", // Smart Contract
                                _ => "unknown",
                            };
                            verif_obj.insert(
                                "type".to_string(),
                                serde_json::Value::String(verif_type.to_string()),
                            );

                            // Add chain id if present (for contract verifications)
                            if verification.chain_id > 0 {
                                verif_obj.insert(
                                    "chain_id".to_string(),
                                    serde_json::Value::Number(serde_json::Number::from(
                                        verification.chain_id,
                                    )),
                                );
                            }

                            // Add timestamp
                            verif_obj.insert(
                                "timestamp".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(
                                    msg_data.timestamp,
                                )),
                            );

                            // Add to the array
                            verifications.push(serde_json::Value::Object(verif_obj));
                        }
                    }
                }

                // Wrap in a result object with metadata
                let result = serde_json::json!({
                    "fid": fid.value(),
                    "count": verifications.len(),
                    "verifications": verifications
                });

                // Convert to JSON string
                serde_json::to_string_pretty(&result)
                    .unwrap_or_else(|_| format!("Error formatting verifications for FID {}", fid))
            },
            Err(e) => format!("Error fetching verifications: {}", e),
        }
    }
}
