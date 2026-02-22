//! User and verification query operations.

use crate::core::types::{Fid, Message, MessageType};
use crate::query::responses::{
    AllVerificationMessagesResponse, FidByUsernameResponse, UserByFidResponse,
    UserByUsernameNotFound, UserByUsernameResponse, UserNotFound, UserProfile, UsernameProof,
    UsernameProofByNameResponse, UsernameProofsByFidResponse, Verification,
    VerificationLookupFound, VerificationLookupNotFound, VerificationLookupResponse,
    VerificationsByFidResponse,
};
use crate::query::{QueryError, QueryResult, WaypointQuery};

use prost::Message as ProstMessage;

pub(crate) fn set_user_data_field(profile: &mut UserProfile, data_type: i32, value: String) {
    match data_type {
        1 => profile.pfp = Some(value),
        2 => profile.display_name = Some(value),
        3 => profile.bio = Some(value),
        5 => profile.url = Some(value),
        6 => profile.username = Some(value),
        7 => profile.location = Some(value),
        8 => profile.twitter = Some(value),
        9 => profile.github = Some(value),
        _ => {},
    }
}

impl<DB, HC> WaypointQuery<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    pub async fn do_get_user_by_fid(&self, fid: Fid) -> QueryResult<UserByFidResponse> {
        let messages = self.data_context.get_user_data_by_fid(fid, 20).await?;

        if messages.is_empty() {
            return Ok(UserByFidResponse::NotFound(UserNotFound {
                fid: fid.value(),
                found: false,
                error: "User data not found".to_string(),
            }));
        }

        Ok(UserByFidResponse::Found(Self::user_profile_from_messages(fid, &messages, None)))
    }

    fn user_profile_from_messages(
        fid: Fid,
        messages: &[Message],
        username: Option<&str>,
    ) -> UserProfile {
        let mut profile = UserProfile { fid: fid.value(), ..UserProfile::default() };
        profile.username = username.map(ToString::to_string);

        for message in messages {
            if message.message_type != MessageType::UserData {
                continue;
            }

            if let Ok(msg_data) =
                <crate::proto::MessageData as ProstMessage>::decode(&*message.payload)
                && let Some(crate::proto::message_data::Body::UserDataBody(user_data)) =
                    msg_data.body
            {
                set_user_data_field(&mut profile, user_data.r#type, user_data.value);
            }
        }

        profile
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

    fn username_proof_to_json(proof: &crate::proto::UserNameProof) -> UsernameProof {
        UsernameProof {
            name: String::from_utf8_lossy(&proof.name).to_string(),
            proof_type: Self::proof_type_name(proof.r#type).to_string(),
            fid: proof.fid,
            timestamp: proof.timestamp,
            owner: format!("0x{}", hex::encode(&proof.owner)),
        }
    }

    fn verification_message_to_json(message: &Message) -> Option<Verification> {
        if message.message_type != MessageType::Verification {
            return None;
        }

        let msg_data: crate::proto::MessageData = ProstMessage::decode(&*message.payload).ok()?;

        match msg_data.body {
            Some(crate::proto::message_data::Body::VerificationAddAddressBody(verification)) => {
                Some(Verification {
                    fid: msg_data.fid,
                    address: format!("0x{}", hex::encode(&verification.address)),
                    protocol: Self::protocol_name(verification.protocol).to_string(),
                    verification_type: Some(
                        Self::verification_type_name(verification.verification_type).to_string(),
                    ),
                    action: "add".to_string(),
                    timestamp: msg_data.timestamp,
                    chain_id: if verification.chain_id > 0 {
                        Some(verification.chain_id)
                    } else {
                        None
                    },
                })
            },
            Some(crate::proto::message_data::Body::VerificationRemoveBody(verification)) => {
                Some(Verification {
                    fid: msg_data.fid,
                    address: format!("0x{}", hex::encode(&verification.address)),
                    protocol: Self::protocol_name(verification.protocol).to_string(),
                    verification_type: None,
                    action: "remove".to_string(),
                    timestamp: msg_data.timestamp,
                    chain_id: None,
                })
            },
            _ => None,
        }
    }

    /// Get user verifications by FID
    pub async fn do_get_verifications_by_fid(
        &self,
        fid: Fid,
        limit: usize,
    ) -> QueryResult<VerificationsByFidResponse> {
        tracing::debug!("Query: Fetching verifications for FID: {}", fid);

        let messages = self.data_context.get_verifications_by_fid(fid, limit).await?;
        let verifications: Vec<Verification> =
            messages.iter().filter_map(Self::verification_message_to_json).collect();

        Ok(VerificationsByFidResponse {
            fid: fid.value(),
            count: verifications.len(),
            verifications,
        })
    }

    /// Get a single verification by FID and address
    pub async fn do_get_verification(
        &self,
        fid: Fid,
        address_hex: &str,
    ) -> QueryResult<VerificationLookupResponse> {
        tracing::debug!(
            "Query: Fetching verification for FID: {} and address: {}",
            fid,
            address_hex
        );

        let address = address_hex.trim_start_matches("0x");
        if address.is_empty() {
            return Err(QueryError::InvalidInput(
                "Invalid address format: empty address".to_string(),
            ));
        }

        let address_bytes = match hex::decode(address) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err(QueryError::InvalidInput(format!(
                    "Invalid address format: {}",
                    address_hex
                )));
            },
        };

        match self.data_context.get_verification(fid, &address_bytes).await? {
            Some(message) => match Self::verification_message_to_json(&message) {
                Some(verification) => {
                    Ok(VerificationLookupResponse::Found(VerificationLookupFound {
                        fid: fid.value(),
                        address: format!("0x{}", address),
                        found: true,
                        verification,
                    }))
                },
                None => Err(QueryError::Processing(format!(
                    "verification payload could not be processed for FID {} and address {}",
                    fid, address_hex
                ))),
            },
            None => Ok(VerificationLookupResponse::NotFound(VerificationLookupNotFound {
                fid: fid.value(),
                address: format!("0x{}", address),
                found: false,
                error: "Verification not found".to_string(),
            })),
        }
    }

    /// Get all verification messages by FID with timestamp filtering
    pub async fn do_get_all_verification_messages_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> QueryResult<AllVerificationMessagesResponse> {
        tracing::debug!(
            "Query: Fetching all verification messages for FID: {} with time filtering",
            fid
        );

        let messages = self
            .data_context
            .get_all_verification_messages_by_fid(fid, limit, start_time, end_time)
            .await?;

        let verifications: Vec<Verification> =
            messages.iter().filter_map(Self::verification_message_to_json).collect();

        Ok(AllVerificationMessagesResponse {
            fid: fid.value(),
            count: verifications.len(),
            start_time,
            end_time,
            verifications,
        })
    }

    /// Find FID by username
    pub async fn do_get_fid_by_username(
        &self,
        username: &str,
    ) -> QueryResult<FidByUsernameResponse> {
        match self.data_context.get_fid_by_username(username).await? {
            Some(fid) => Ok(FidByUsernameResponse {
                username: username.to_string(),
                found: true,
                fid: Some(fid.value()),
                error: None,
            }),
            None => Ok(FidByUsernameResponse {
                username: username.to_string(),
                found: false,
                fid: None,
                error: Some("Username not found".to_string()),
            }),
        }
    }

    /// Get user profile by username
    pub async fn do_get_user_by_username(
        &self,
        username: &str,
    ) -> QueryResult<UserByUsernameResponse> {
        match self.data_context.get_fid_by_username(username).await? {
            Some(fid) => {
                let messages = self.data_context.get_user_data_by_fid(fid, 20).await?;
                if messages.is_empty() {
                    return Ok(UserByUsernameResponse::NotFound(UserByUsernameNotFound {
                        username: username.to_string(),
                        fid: Some(fid.value()),
                        found: false,
                        error: "User data not found".to_string(),
                    }));
                }

                let profile = Self::user_profile_from_messages(fid, &messages, Some(username));
                Ok(UserByUsernameResponse::Found(profile))
            },
            None => Ok(UserByUsernameResponse::NotFound(UserByUsernameNotFound {
                username: username.to_string(),
                fid: None,
                found: false,
                error: "Username not found".to_string(),
            })),
        }
    }

    /// Get username proofs by FID
    pub async fn do_get_username_proofs_by_fid(
        &self,
        fid: Fid,
    ) -> QueryResult<UsernameProofsByFidResponse> {
        tracing::debug!("Query: Fetching username proofs for FID: {}", fid);

        let messages = self.data_context.get_username_proofs_by_fid(fid).await?;
        let proofs: Vec<UsernameProof> = messages
            .iter()
            .filter_map(|message| {
                serde_json::from_slice::<crate::proto::UserNameProof>(&message.payload)
                    .ok()
                    .map(|proof| Self::username_proof_to_json(&proof))
            })
            .collect();

        Ok(UsernameProofsByFidResponse { fid: fid.value(), count: proofs.len(), proofs })
    }

    /// Get a single username proof by name
    pub async fn do_get_username_proof(
        &self,
        name: &str,
    ) -> QueryResult<UsernameProofByNameResponse> {
        tracing::debug!("Query: Fetching username proof for name: {}", name);

        match self.data_context.get_username_proof_by_name(name).await? {
            Some(proof) => {
                let response = Self::username_proof_to_json(&proof);
                Ok(UsernameProofByNameResponse {
                    name: response.name,
                    found: true,
                    proof_type: Some(response.proof_type),
                    fid: Some(response.fid),
                    timestamp: Some(response.timestamp),
                    owner: Some(response.owner),
                    error: None,
                })
            },
            None => Ok(UsernameProofByNameResponse {
                name: name.to_string(),
                found: false,
                proof_type: None,
                fid: None,
                timestamp: None,
                owner: None,
                error: Some("Username proof not found".to_string()),
            }),
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
        assert_eq!(result.fid, 12345);
        assert_eq!(result.address, "0x1a2b");
        assert_eq!(result.protocol, "ethereum");
        assert_eq!(result.verification_type.as_deref(), Some("contract"));
        assert_eq!(result.action, "add");
        assert_eq!(result.chain_id, Some(10));
        assert_eq!(result.timestamp, 1672531200);
    }

    #[test]
    fn test_verification_message_to_json_add_omits_chain_id_when_zero() {
        let message = make_verification_add_message(12345, &[0xaa, 0xbb], 0, 0, 0, 1672531200);

        let result = TestService::verification_message_to_json(&message).unwrap();
        assert_eq!(result.chain_id, None);
    }

    #[test]
    fn test_verification_message_to_json_remove() {
        let message = make_verification_remove_message(12345, &[0xde, 0xad], 1, 1672617600);

        let result = TestService::verification_message_to_json(&message).unwrap();
        assert_eq!(result.fid, 12345);
        assert_eq!(result.address, "0xdead");
        assert_eq!(result.protocol, "solana");
        assert_eq!(result.action, "remove");
        assert_eq!(result.timestamp, 1672617600);
        assert_eq!(result.verification_type, None);
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
        assert!(matches!(
            result,
            Err(QueryError::InvalidInput(message)) if message == "Invalid address format: empty address"
        ));
    }

    #[tokio::test]
    async fn test_do_get_verification_invalid_hex_address() {
        let service = make_service(MockHub::default());

        let result = service.do_get_verification(Fid::from(12345), "0xzz").await;
        assert!(matches!(
            result,
            Err(QueryError::InvalidInput(message)) if message == "Invalid address format: 0xzz"
        ));
    }

    #[tokio::test]
    async fn test_do_get_verification_not_found_returns_found_false_json() {
        let service = make_service(MockHub::default());

        let result = service.do_get_verification(Fid::from(12345), "0x00112233").await;
        let parsed = result.expect("query succeeds");

        let VerificationLookupResponse::NotFound(not_found) = parsed else {
            panic!("expected not found response");
        };

        assert_eq!(not_found.fid, 12345);
        assert_eq!(not_found.address, "0x00112233");
        assert!(!not_found.found);
        assert_eq!(not_found.error, "Verification not found");
    }

    #[tokio::test]
    async fn test_do_get_verification_found_returns_verification_json() {
        let message = make_verification_add_message(12345, &[0xaa, 0xbb], 0, 0, 0, 1672531200);
        let service = make_service(MockHub { verification: Some(message) });

        let result = service.do_get_verification(Fid::from(12345), "0xaabb").await;
        let parsed = result.expect("query succeeds");

        let VerificationLookupResponse::Found(found) = parsed else {
            panic!("expected found response");
        };

        assert_eq!(found.fid, 12345);
        assert_eq!(found.address, "0xaabb");
        assert!(found.found);
        assert_eq!(found.verification.action, "add");
        assert_eq!(found.verification.protocol, "ethereum");
        assert_eq!(found.verification.verification_type.as_deref(), Some("eoa"));
    }
}
