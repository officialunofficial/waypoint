use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use async_trait::async_trait;
use prost::Message as ProstMessage;
use serde_json::{Value, json};

use waypoint::core::data_context::{DataAccessError, DataContextBuilder, Database, HubClient};
use waypoint::core::types::{Fid, Message, MessageId, MessageType as CoreMessageType};
use waypoint::query::{QueryError, WaypointQuery};

const ROOT_FID: u64 = 1001;
const PARENT_FID: u64 = 2002;
const QUOTED_FID: u64 = 3003;
const REPLY_FID: u64 = 4004;
const REACTOR_FID: u64 = 5005;

const ROOT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PARENT_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const QUOTED_HASH: &str = "cccccccccccccccccccccccccccccccccccccccc";
const REPLY_HASH: &str = "dddddddddddddddddddddddddddddddddddddddd";
const REACTION_CAST_HASH: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const REACTION_URL_HASH: &str = "ffffffffffffffffffffffffffffffffffffffff";
const LINK_HASH: &str = "1212121212121212121212121212121212121212";
const LINK_COMPACT_HASH: &str = "3434343434343434343434343434343434343434";
const MISSING_CAST_HASH: &str = "9999999999999999999999999999999999999999";

const TARGET_URL: &str = "https://example.com/post/123";
const EMPTY_PARENT_URL: &str = "https://example.com/empty";

#[derive(Clone, Debug)]
struct MockDb;

#[async_trait]
impl Database for MockDb {
    async fn get_message(
        &self,
        _id: &MessageId,
        _message_type: CoreMessageType,
    ) -> waypoint::core::data_context::Result<Message> {
        Err(DataAccessError::Other("mock db".to_string()))
    }

    async fn get_messages_by_fid(
        &self,
        _fid: Fid,
        _message_type: CoreMessageType,
        _limit: usize,
        _cursor: Option<MessageId>,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(vec![])
    }

    async fn store_message(&self, _message: Message) -> waypoint::core::data_context::Result<()> {
        Ok(())
    }

    async fn delete_message(
        &self,
        _id: &MessageId,
        _message_type: CoreMessageType,
    ) -> waypoint::core::data_context::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct MockHub {
    user_data_error: Option<String>,
    user_data_by_fid: HashMap<u64, Vec<Message>>,
    verifications_by_fid: HashMap<u64, Vec<Message>>,
    verification_by_key: HashMap<(u64, String), Message>,
    casts_by_key: HashMap<(u64, String), Message>,
    casts_by_parent: HashMap<(u64, String), Vec<Message>>,
    casts_by_parent_url: HashMap<String, Vec<Message>>,
    reactions_by_target_cast: HashMap<(u64, String), Vec<Message>>,
    reactions_by_target_url: HashMap<String, Vec<Message>>,
    links_by_fid: HashMap<u64, Vec<Message>>,
    links_by_target: HashMap<u64, Vec<Message>>,
    link_compact_state_by_fid: HashMap<u64, Vec<Message>>,
    link_by_key: HashMap<(u64, String, u64), Message>,
    username_proofs_by_fid: HashMap<u64, Vec<Message>>,
    username_proof_by_name: HashMap<String, waypoint::proto::UserNameProof>,
}

#[async_trait]
impl HubClient for MockHub {
    async fn get_user_data_by_fid(
        &self,
        fid: Fid,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        if let Some(error) = &self.user_data_error {
            return Err(DataAccessError::HubClient(error.clone()));
        }

        let mut messages = self.user_data_by_fid.get(&fid.value()).cloned().unwrap_or_default();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_user_data(
        &self,
        fid: Fid,
        _data_type: &str,
    ) -> waypoint::core::data_context::Result<Option<Message>> {
        Ok(self.user_data_by_fid.get(&fid.value()).and_then(|messages| messages.first().cloned()))
    }

    async fn get_username_proofs_by_fid(
        &self,
        fid: Fid,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(self.username_proofs_by_fid.get(&fid.value()).cloned().unwrap_or_default())
    }

    async fn get_username_proof_by_name(
        &self,
        name: &str,
    ) -> waypoint::core::data_context::Result<Option<waypoint::proto::UserNameProof>> {
        Ok(self.username_proof_by_name.get(name).cloned())
    }

    async fn get_verifications_by_fid(
        &self,
        fid: Fid,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        let mut messages = self.verifications_by_fid.get(&fid.value()).cloned().unwrap_or_default();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_verification(
        &self,
        fid: Fid,
        address: &[u8],
    ) -> waypoint::core::data_context::Result<Option<Message>> {
        Ok(self.verification_by_key.get(&(fid.value(), hex::encode(address))).cloned())
    }

    async fn get_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        let mut messages = self
            .casts_by_key
            .iter()
            .filter(|((cast_fid, _), _)| *cast_fid == fid.value())
            .map(|(_, message)| message.clone())
            .collect::<Vec<_>>();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_cast(
        &self,
        fid: Fid,
        hash: &[u8],
    ) -> waypoint::core::data_context::Result<Option<Message>> {
        Ok(self.casts_by_key.get(&(fid.value(), hex::encode(hash))).cloned())
    }

    async fn get_casts_by_mention(
        &self,
        _fid: Fid,
        _limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(vec![])
    }

    async fn get_casts_by_parent(
        &self,
        parent_fid: Fid,
        parent_hash: &[u8],
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        let key = (parent_fid.value(), hex::encode(parent_hash));
        let mut messages = self.casts_by_parent.get(&key).cloned().unwrap_or_default();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_casts_by_parent_url(
        &self,
        parent_url: &str,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        let mut messages = self.casts_by_parent_url.get(parent_url).cloned().unwrap_or_default();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_all_casts_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        self.get_casts_by_fid(fid, limit).await
    }

    async fn get_reaction(
        &self,
        _fid: Fid,
        _reaction_type: u8,
        _target_cast_fid: Option<Fid>,
        _target_cast_hash: Option<&[u8]>,
        _target_url: Option<&str>,
    ) -> waypoint::core::data_context::Result<Option<Message>> {
        Ok(None)
    }

    async fn get_reactions_by_fid(
        &self,
        _fid: Fid,
        _reaction_type: Option<u8>,
        _limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(vec![])
    }

    async fn get_reactions_by_target(
        &self,
        target_cast_fid: Option<Fid>,
        target_cast_hash: Option<&[u8]>,
        target_url: Option<&str>,
        _reaction_type: Option<u8>,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        if let Some(url) = target_url {
            let mut messages = self.reactions_by_target_url.get(url).cloned().unwrap_or_default();
            messages.truncate(limit);
            return Ok(messages);
        }

        if let (Some(fid), Some(hash)) = (target_cast_fid, target_cast_hash) {
            let key = (fid.value(), hex::encode(hash));
            let mut messages = self.reactions_by_target_cast.get(&key).cloned().unwrap_or_default();
            messages.truncate(limit);
            return Ok(messages);
        }

        Ok(vec![])
    }

    async fn get_all_reactions_by_fid(
        &self,
        _fid: Fid,
        _limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(vec![])
    }

    async fn get_all_verification_messages_by_fid(
        &self,
        fid: Fid,
        limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        self.get_verifications_by_fid(fid, limit).await
    }

    async fn get_link(
        &self,
        fid: Fid,
        link_type: &str,
        target_fid: Fid,
    ) -> waypoint::core::data_context::Result<Option<Message>> {
        Ok(self.link_by_key.get(&(fid.value(), link_type.to_string(), target_fid.value())).cloned())
    }

    async fn get_links_by_fid(
        &self,
        fid: Fid,
        _link_type: Option<&str>,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        let mut messages = self.links_by_fid.get(&fid.value()).cloned().unwrap_or_default();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_links_by_target(
        &self,
        target_fid: Fid,
        _link_type: Option<&str>,
        limit: usize,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        let mut messages =
            self.links_by_target.get(&target_fid.value()).cloned().unwrap_or_default();
        messages.truncate(limit);
        Ok(messages)
    }

    async fn get_link_compact_state_by_fid(
        &self,
        fid: Fid,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(self.link_compact_state_by_fid.get(&fid.value()).cloned().unwrap_or_default())
    }

    async fn get_all_links_by_fid(
        &self,
        _fid: Fid,
        _limit: usize,
        _start_time: Option<u64>,
        _end_time: Option<u64>,
    ) -> waypoint::core::data_context::Result<Vec<Message>> {
        Ok(vec![])
    }
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mcp_contract")
        .join(format!("{name}.json"))
}

fn normalize_contract_json(value: &mut Value) {
    if let Some(object) = value.as_object_mut() {
        if let Some(participants) = object.get_mut("participants").and_then(Value::as_object_mut)
            && let Some(fids) = participants.get_mut("fids").and_then(Value::as_array_mut)
        {
            fids.sort_by(|left, right| {
                left.as_str().unwrap_or_default().cmp(right.as_str().unwrap_or_default())
            });
        }

        for child in object.values_mut() {
            normalize_contract_json(child);
        }
        return;
    }

    if let Some(array) = value.as_array_mut() {
        for child in array {
            normalize_contract_json(child);
        }
    }
}

fn assert_json_fixture(name: &str, actual: Value) {
    let path = fixture_path(name);
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed reading fixture {}: {}", path.display(), error));
    let mut expected: Value = serde_json::from_str(&contents).unwrap_or_else(|error| {
        panic!("failed parsing fixture {} as JSON: {}", path.display(), error)
    });

    let mut actual = actual;
    normalize_contract_json(&mut expected);
    normalize_contract_json(&mut actual);

    assert_eq!(actual, expected, "fixture mismatch for {}", path.display());
}

fn query_error_to_json(error: QueryError) -> Value {
    match error {
        QueryError::InvalidInput(message) => {
            json!({ "error_type": "invalid_input", "message": message })
        },
        QueryError::DataAccess(error) => {
            json!({ "error_type": "data_access", "message": error.to_string() })
        },
        QueryError::Processing(message) => {
            json!({ "error_type": "processing", "message": message })
        },
    }
}

fn decode_hex(value: &str) -> Vec<u8> {
    hex::decode(value).expect("fixture hex must be valid")
}

fn make_user_data_message(
    id: &str,
    fid: u64,
    timestamp: u32,
    data_type: i32,
    value: &str,
) -> Message {
    let data = waypoint::proto::MessageData {
        r#type: waypoint::proto::MessageType::UserDataAdd as i32,
        fid,
        timestamp,
        network: waypoint::proto::FarcasterNetwork::Mainnet as i32,
        body: Some(waypoint::proto::message_data::Body::UserDataBody(
            waypoint::proto::UserDataBody { r#type: data_type, value: value.to_string() },
        )),
    };

    Message::new(id, CoreMessageType::UserData, data.encode_to_vec())
}

fn make_cast_embed_cast(fid: u64, hash: &str) -> waypoint::proto::Embed {
    waypoint::proto::Embed {
        embed: Some(waypoint::proto::embed::Embed::CastId(waypoint::proto::CastId {
            fid,
            hash: decode_hex(hash),
        })),
    }
}

#[allow(clippy::too_many_arguments)]
fn make_cast_message(
    id: &str,
    fid: u64,
    timestamp: u32,
    text: &str,
    parent_cast: Option<(u64, &str)>,
    embeds: Vec<waypoint::proto::Embed>,
    mentions: Vec<u64>,
) -> Message {
    let parent = parent_cast.map(|(parent_fid, parent_hash)| {
        waypoint::proto::cast_add_body::Parent::ParentCastId(waypoint::proto::CastId {
            fid: parent_fid,
            hash: decode_hex(parent_hash),
        })
    });

    let body = waypoint::proto::CastAddBody {
        embeds_deprecated: vec![],
        mentions,
        text: text.to_string(),
        mentions_positions: vec![],
        embeds,
        r#type: 0,
        parent,
    };

    let data = waypoint::proto::MessageData {
        r#type: waypoint::proto::MessageType::CastAdd as i32,
        fid,
        timestamp,
        network: waypoint::proto::FarcasterNetwork::Mainnet as i32,
        body: Some(waypoint::proto::message_data::Body::CastAddBody(body)),
    };

    Message::new(id, CoreMessageType::Cast, data.encode_to_vec())
}

fn make_reaction_cast_message(
    id: &str,
    fid: u64,
    timestamp: u32,
    reaction_type: i32,
    target_fid: u64,
    target_hash: &str,
) -> Message {
    let body = waypoint::proto::ReactionBody {
        r#type: reaction_type,
        target: Some(waypoint::proto::reaction_body::Target::TargetCastId(
            waypoint::proto::CastId { fid: target_fid, hash: decode_hex(target_hash) },
        )),
    };

    let data = waypoint::proto::MessageData {
        r#type: waypoint::proto::MessageType::ReactionAdd as i32,
        fid,
        timestamp,
        network: waypoint::proto::FarcasterNetwork::Mainnet as i32,
        body: Some(waypoint::proto::message_data::Body::ReactionBody(body)),
    };

    Message::new(id, CoreMessageType::Reaction, data.encode_to_vec())
}

fn make_reaction_url_message(
    id: &str,
    fid: u64,
    timestamp: u32,
    reaction_type: i32,
    target_url: &str,
) -> Message {
    let body = waypoint::proto::ReactionBody {
        r#type: reaction_type,
        target: Some(waypoint::proto::reaction_body::Target::TargetUrl(target_url.to_string())),
    };

    let data = waypoint::proto::MessageData {
        r#type: waypoint::proto::MessageType::ReactionAdd as i32,
        fid,
        timestamp,
        network: waypoint::proto::FarcasterNetwork::Mainnet as i32,
        body: Some(waypoint::proto::message_data::Body::ReactionBody(body)),
    };

    Message::new(id, CoreMessageType::Reaction, data.encode_to_vec())
}

fn make_link_message(
    id: &str,
    fid: u64,
    timestamp: u32,
    link_type: &str,
    target_fid: u64,
    display_timestamp: Option<u32>,
) -> Message {
    let body = waypoint::proto::LinkBody {
        r#type: link_type.to_string(),
        display_timestamp,
        target: Some(waypoint::proto::link_body::Target::TargetFid(target_fid)),
    };

    let data = waypoint::proto::MessageData {
        r#type: waypoint::proto::MessageType::LinkAdd as i32,
        fid,
        timestamp,
        network: waypoint::proto::FarcasterNetwork::Mainnet as i32,
        body: Some(waypoint::proto::message_data::Body::LinkBody(body)),
    };

    Message::new(id, CoreMessageType::Link, data.encode_to_vec())
}

fn make_link_compact_state_message(
    id: &str,
    fid: u64,
    timestamp: u32,
    link_type: &str,
    target_fids: Vec<u64>,
) -> Message {
    let body = waypoint::proto::LinkCompactStateBody { r#type: link_type.to_string(), target_fids };

    let data = waypoint::proto::MessageData {
        r#type: waypoint::proto::MessageType::LinkCompactState as i32,
        fid,
        timestamp,
        network: waypoint::proto::FarcasterNetwork::Mainnet as i32,
        body: Some(waypoint::proto::message_data::Body::LinkCompactStateBody(body)),
    };

    Message::new(id, CoreMessageType::Link, data.encode_to_vec())
}

fn make_username_proof_message(id: &str, proof: &waypoint::proto::UserNameProof) -> Message {
    let payload = serde_json::to_vec(proof).expect("username proof fixture should serialize");
    Message::new(id, CoreMessageType::UsernameProof, payload)
}

fn seeded_hub() -> MockHub {
    let mut hub = MockHub::default();

    hub.user_data_by_fid.insert(
        ROOT_FID,
        vec![
            make_user_data_message("u-1001-username", ROOT_FID, 1, 6, "alice"),
            make_user_data_message("u-1001-display", ROOT_FID, 2, 2, "Alice"),
            make_user_data_message("u-1001-bio", ROOT_FID, 3, 3, "building typed contracts"),
            make_user_data_message("u-1001-pfp", ROOT_FID, 4, 1, "https://example.com/alice.png"),
        ],
    );
    hub.user_data_by_fid.insert(
        PARENT_FID,
        vec![
            make_user_data_message("u-2002-username", PARENT_FID, 1, 6, "bob"),
            make_user_data_message("u-2002-display", PARENT_FID, 2, 2, "Bob"),
        ],
    );
    hub.user_data_by_fid.insert(
        QUOTED_FID,
        vec![
            make_user_data_message("u-3003-username", QUOTED_FID, 1, 6, "carol"),
            make_user_data_message("u-3003-display", QUOTED_FID, 2, 2, "Carol"),
        ],
    );
    hub.user_data_by_fid.insert(
        REPLY_FID,
        vec![
            make_user_data_message("u-4004-username", REPLY_FID, 1, 6, "dave"),
            make_user_data_message("u-4004-display", REPLY_FID, 2, 2, "Dave"),
        ],
    );

    let parent_cast =
        make_cast_message(PARENT_HASH, PARENT_FID, 10, "Parent context cast", None, vec![], vec![]);
    let quoted_cast =
        make_cast_message(QUOTED_HASH, QUOTED_FID, 11, "Quoted cast content", None, vec![], vec![]);
    let root_cast = make_cast_message(
        ROOT_HASH,
        ROOT_FID,
        12,
        "Root cast discussing fixture coverage and compatibility",
        Some((PARENT_FID, PARENT_HASH)),
        vec![make_cast_embed_cast(QUOTED_FID, QUOTED_HASH)],
        vec![REPLY_FID],
    );
    let reply_cast = make_cast_message(
        REPLY_HASH,
        REPLY_FID,
        13,
        "Reply in thread",
        Some((ROOT_FID, ROOT_HASH)),
        vec![],
        vec![],
    );

    hub.casts_by_key.insert((PARENT_FID, PARENT_HASH.to_string()), parent_cast);
    hub.casts_by_key.insert((QUOTED_FID, QUOTED_HASH.to_string()), quoted_cast);
    hub.casts_by_key.insert((ROOT_FID, ROOT_HASH.to_string()), root_cast);
    hub.casts_by_key.insert((REPLY_FID, REPLY_HASH.to_string()), reply_cast.clone());

    hub.casts_by_parent.insert((ROOT_FID, ROOT_HASH.to_string()), vec![reply_cast]);
    hub.casts_by_parent.insert((REPLY_FID, REPLY_HASH.to_string()), vec![]);
    hub.casts_by_parent_url.insert(EMPTY_PARENT_URL.to_string(), vec![]);

    let cast_reaction =
        make_reaction_cast_message(REACTION_CAST_HASH, REACTOR_FID, 22, 1, ROOT_FID, ROOT_HASH);
    let url_reaction = make_reaction_url_message(REACTION_URL_HASH, REACTOR_FID, 23, 2, TARGET_URL);

    hub.reactions_by_target_cast.insert((ROOT_FID, ROOT_HASH.to_string()), vec![cast_reaction]);
    hub.reactions_by_target_url.insert(TARGET_URL.to_string(), vec![url_reaction]);

    let follow_link = make_link_message(LINK_HASH, REACTOR_FID, 24, "follow", ROOT_FID, Some(77));
    hub.links_by_fid.insert(REACTOR_FID, vec![follow_link.clone()]);
    hub.links_by_target.insert(ROOT_FID, vec![follow_link]);

    hub.link_compact_state_by_fid.insert(
        ROOT_FID,
        vec![make_link_compact_state_message(
            LINK_COMPACT_HASH,
            ROOT_FID,
            25,
            "follow",
            vec![REACTOR_FID, REPLY_FID],
        )],
    );

    let proof_fname = waypoint::proto::UserNameProof {
        timestamp: 1_700_000_000,
        name: b"alice".to_vec(),
        owner: vec![0xaa, 0xbb],
        signature: vec![],
        fid: ROOT_FID,
        r#type: 1,
    };
    let proof_ens = waypoint::proto::UserNameProof {
        timestamp: 1_700_000_100,
        name: b"alice.eth".to_vec(),
        owner: vec![0x11, 0x22],
        signature: vec![],
        fid: ROOT_FID,
        r#type: 2,
    };

    hub.username_proofs_by_fid.insert(
        ROOT_FID,
        vec![
            make_username_proof_message("proof-fname", &proof_fname),
            make_username_proof_message("proof-ens", &proof_ens),
        ],
    );

    hub
}

fn make_query() -> WaypointQuery<MockDb, MockHub> {
    make_query_with_hub(seeded_hub())
}

fn make_query_with_hub(hub: MockHub) -> WaypointQuery<MockDb, MockHub> {
    let data_context = DataContextBuilder::new().with_database(MockDb).with_hub_client(hub).build();
    WaypointQuery::new(data_context)
}

#[tokio::test]
async fn user_by_fid_contract_fixture_matches() {
    let query = make_query();
    let result = query.do_get_user_by_fid(Fid::from(ROOT_FID)).await.expect("query succeeds");
    assert_json_fixture("user_by_fid_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn user_by_fid_not_found_contract_fixture_matches() {
    let query = make_query();
    let result = query.do_get_user_by_fid(Fid::from(7777)).await.expect("query succeeds");
    assert_json_fixture("user_by_fid_not_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn user_by_fid_hub_error_contract_fixture_matches() {
    let mut hub = seeded_hub();
    hub.user_data_error = Some("hub unavailable".to_string());
    let query = make_query_with_hub(hub);
    let error = query.do_get_user_by_fid(Fid::from(ROOT_FID)).await.expect_err("query should fail");
    assert_json_fixture("user_by_fid_hub_error", query_error_to_json(error));
}

#[tokio::test]
async fn verifications_by_fid_empty_contract_fixture_matches() {
    let query = make_query();
    let result =
        query.do_get_verifications_by_fid(Fid::from(7777), 10).await.expect("query succeeds");
    assert_json_fixture("verifications_by_fid_empty", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn casts_by_parent_url_empty_contract_fixture_matches() {
    let query = make_query();
    let result =
        query.do_get_casts_by_parent_url(EMPTY_PARENT_URL, 10).await.expect("query succeeds");
    assert_json_fixture("casts_by_parent_url_empty", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn verification_lookup_not_found_contract_fixture_matches() {
    let query = make_query();
    let result =
        query.do_get_verification(Fid::from(ROOT_FID), "0xabab").await.expect("query succeeds");
    assert_json_fixture("verification_lookup_not_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn link_lookup_not_found_contract_fixture_matches() {
    let query = make_query();
    let result = query
        .do_get_link(Fid::from(REACTOR_FID), "follow", Fid::from(9999))
        .await
        .expect("query succeeds");
    assert_json_fixture("link_lookup_not_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn cast_lookup_not_found_normalized_hash_contract_fixture_matches() {
    let query = make_query();
    let prefixed_hash = format!("0x{}", MISSING_CAST_HASH);
    let result =
        query.do_get_cast(Fid::from(ROOT_FID), &prefixed_hash).await.expect("query succeeds");
    assert_json_fixture(
        "cast_lookup_not_found_normalized_hash",
        serde_json::to_value(result).unwrap(),
    );
}

#[tokio::test]
async fn links_by_target_found_contract_fixture_matches() {
    let query = make_query();
    let result = query
        .do_get_links_by_target(Fid::from(ROOT_FID), Some("follow"), 10)
        .await
        .expect("query succeeds");
    assert_json_fixture("links_by_target_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn links_by_fid_found_contract_fixture_matches() {
    let query = make_query();
    let result =
        query.do_get_links_by_fid(Fid::from(REACTOR_FID), None, 10).await.expect("query succeeds");
    assert_json_fixture("links_by_fid_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn link_compact_state_found_contract_fixture_matches() {
    let query = make_query();
    let result =
        query.do_get_link_compact_state_by_fid(Fid::from(ROOT_FID)).await.expect("query succeeds");
    assert_json_fixture("link_compact_state_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn reactions_by_target_cast_contract_fixture_matches() {
    let query = make_query();
    let target_hash = decode_hex(ROOT_HASH);
    let result = query
        .do_get_reactions_by_target(
            Some(Fid::from(ROOT_FID)),
            Some(target_hash.as_slice()),
            None,
            None,
            10,
        )
        .await
        .expect("query succeeds");
    assert_json_fixture("reactions_by_target_cast", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn reactions_by_target_url_precedence_contract_fixture_matches() {
    let query = make_query();
    let target_hash = decode_hex(ROOT_HASH);
    let result = query
        .do_get_reactions_by_target(
            Some(Fid::from(ROOT_FID)),
            Some(target_hash.as_slice()),
            Some(TARGET_URL),
            None,
            10,
        )
        .await
        .expect("query succeeds");
    assert_json_fixture(
        "reactions_by_target_url_precedence",
        serde_json::to_value(result).unwrap(),
    );
}

#[tokio::test]
async fn reaction_lookup_not_found_url_precedence_contract_fixture_matches() {
    let query = make_query();
    let target_hash = decode_hex(ROOT_HASH);
    let result = query
        .do_get_reaction(
            Fid::from(REACTOR_FID),
            1,
            Some(Fid::from(ROOT_FID)),
            Some(target_hash.as_slice()),
            Some("https://example.com/missing"),
        )
        .await
        .expect("query succeeds");
    assert_json_fixture(
        "reaction_lookup_not_found_url_precedence",
        serde_json::to_value(result).unwrap(),
    );
}

#[tokio::test]
async fn reaction_lookup_missing_target_validation_error_contract_fixture_matches() {
    let query = make_query();
    let error = query
        .do_get_reaction(Fid::from(REACTOR_FID), 1, None, None, None)
        .await
        .expect_err("missing target should fail");
    assert_json_fixture("reaction_lookup_missing_target_error", query_error_to_json(error));
}

#[tokio::test]
async fn reaction_lookup_invalid_type_validation_error_contract_fixture_matches() {
    let query = make_query();
    let target_hash = decode_hex(ROOT_HASH);
    let error = query
        .do_get_reaction(
            Fid::from(REACTOR_FID),
            9,
            Some(Fid::from(ROOT_FID)),
            Some(target_hash.as_slice()),
            None,
        )
        .await
        .expect_err("invalid reaction type should fail");
    assert_json_fixture("reaction_lookup_invalid_type_error", query_error_to_json(error));
}

#[tokio::test]
async fn reactions_by_target_invalid_type_validation_error_contract_fixture_matches() {
    let query = make_query();
    let error = query
        .do_get_reactions_by_target(None, None, Some(TARGET_URL), Some(9), 10)
        .await
        .expect_err("invalid reaction type should fail");
    assert_json_fixture("reactions_by_target_invalid_type_error", query_error_to_json(error));
}

#[tokio::test]
async fn reactions_by_fid_invalid_type_validation_error_contract_fixture_matches() {
    let query = make_query();
    let error = query
        .do_get_reactions_by_fid(Fid::from(REACTOR_FID), Some(9), 10)
        .await
        .expect_err("invalid reaction type should fail");
    assert_json_fixture("reaction_lookup_invalid_type_error", query_error_to_json(error));
}

#[tokio::test]
async fn username_proofs_by_fid_contract_fixture_matches() {
    let query = make_query();
    let result =
        query.do_get_username_proofs_by_fid(Fid::from(ROOT_FID)).await.expect("query succeeds");
    assert_json_fixture("username_proofs_by_fid", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn conversation_contract_fixture_matches() {
    let query = make_query();
    let result = query
        .do_get_conversation(Fid::from(ROOT_FID), ROOT_HASH, true, 3, 10)
        .await
        .expect("query succeeds");
    assert_json_fixture("conversation_found", serde_json::to_value(result).unwrap());
}

#[tokio::test]
async fn conversation_not_found_normalized_hash_contract_fixture_matches() {
    let query = make_query();
    let prefixed_hash = format!("0x{}", MISSING_CAST_HASH);
    let result = query
        .do_get_conversation(Fid::from(ROOT_FID), &prefixed_hash, true, 3, 10)
        .await
        .expect("query succeeds");
    assert_json_fixture(
        "conversation_not_found_normalized_hash",
        serde_json::to_value(result).unwrap(),
    );
}

#[tokio::test]
async fn validation_error_contract_fixture_matches() {
    let query = make_query();
    let error = query
        .do_get_cast(Fid::from(ROOT_FID), "not-hex")
        .await
        .expect_err("invalid hash should fail");
    assert_json_fixture("cast_invalid_hash_error", query_error_to_json(error));
}
