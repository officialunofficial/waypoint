use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmbedData {
    Url {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    CastId {
        cast_id: CastIdData,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastIdData {
    pub fid: u64,
    pub hash: HashData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HashData {
    #[serde(rename = "type")]
    pub hash_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedEmbed {
    #[serde(flatten)]
    pub data: EmbedData,
}

impl NormalizedEmbed {
    pub fn from_protobuf_embed(embed: &crate::proto::Embed) -> Self {
        match &embed.embed {
            Some(crate::proto::embed::Embed::Url(url)) => {
                NormalizedEmbed { data: EmbedData::Url { url: url.clone() } }
            },
            Some(crate::proto::embed::Embed::CastId(cast_id)) => NormalizedEmbed {
                data: EmbedData::CastId {
                    cast_id: CastIdData {
                        fid: cast_id.fid,
                        hash: HashData {
                            hash_type: "Buffer".to_string(),
                            data: cast_id.hash.clone(),
                        },
                    },
                },
            },
            None => panic!("Invalid embed: no url or cast_id"),
        }
    }
}
