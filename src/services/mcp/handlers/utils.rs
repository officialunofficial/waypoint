//! MCP resource URI parsing utilities.

use percent_encoding::percent_decode_str;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaypointResource {
    UserByFid { fid: u64 },
    UserByUsername { username: String },
    VerificationsByFid { fid: u64 },
    VerificationByAddress { fid: u64, address: String },
    AllVerificationMessagesByFid { fid: u64 },
    Cast { fid: u64, hash: String },
    Conversation { fid: u64, hash: String },
    CastsByFid { fid: u64 },
    CastsByMention { fid: u64 },
    CastsByParent { fid: u64, hash: String },
    CastsByParentUrl { url: String },
    ReactionsByFid { fid: u64 },
    ReactionsByTargetCast { fid: u64, hash: String },
    ReactionsByTargetUrl { url: String },
    LinksByFid { fid: u64 },
    LinksByTarget { fid: u64 },
    LinkCompactStateByFid { fid: u64 },
    UsernameProofByName { name: String },
    UsernameProofsByFid { fid: u64 },
}

pub fn parse_waypoint_resource_uri(uri: &str) -> Result<WaypointResource, String> {
    let url = Url::parse(uri).map_err(|err| format!("Invalid resource URI: {err}"))?;

    if url.scheme() != "waypoint" {
        return Err("Unsupported resource scheme".to_string());
    }

    let mut segments: Vec<String> = Vec::new();

    if let Some(host) = url.host_str()
        && !host.is_empty()
    {
        segments.push(host.to_string());
    }

    if let Some(path_segments) = url.path_segments() {
        for segment in path_segments {
            if !segment.is_empty() {
                let decoded = percent_decode_str(segment)
                    .decode_utf8()
                    .map_err(|_| format!("Invalid UTF-8 in path segment: {segment}"))?;
                segments.push(decoded.into_owned());
            }
        }
    }

    if segments.is_empty() {
        return Err("Missing resource path".to_string());
    }

    let get_url_param = || -> Result<String, String> {
        url.query_pairs()
            .find(|(k, _)| k == "url")
            .map(|(_, v)| v.into_owned())
            .ok_or_else(|| "Missing required 'url' query parameter".to_string())
    };

    let segment_refs: Vec<&str> = segments.iter().map(String::as_str).collect();

    match segment_refs.as_slice() {
        ["users", fid] => Ok(WaypointResource::UserByFid { fid: parse_fid(fid)? }),
        ["users", "by-username", username] => {
            Ok(WaypointResource::UserByUsername { username: username.to_string() })
        },
        ["verifications", fid] => Ok(WaypointResource::VerificationsByFid { fid: parse_fid(fid)? }),
        ["verifications", "all-by-fid", fid] => {
            Ok(WaypointResource::AllVerificationMessagesByFid { fid: parse_fid(fid)? })
        },
        ["verifications", fid, address] => Ok(WaypointResource::VerificationByAddress {
            fid: parse_fid(fid)?,
            address: address.to_string(),
        }),
        ["casts", "by-fid", fid] => Ok(WaypointResource::CastsByFid { fid: parse_fid(fid)? }),
        ["casts", "by-mention", fid] => {
            Ok(WaypointResource::CastsByMention { fid: parse_fid(fid)? })
        },
        ["casts", "by-parent", fid, hash] => {
            Ok(WaypointResource::CastsByParent { fid: parse_fid(fid)?, hash: hash.to_string() })
        },
        ["casts", "by-parent-url"] => {
            Ok(WaypointResource::CastsByParentUrl { url: get_url_param()? })
        },
        ["casts", fid, hash] => {
            Ok(WaypointResource::Cast { fid: parse_fid(fid)?, hash: hash.to_string() })
        },
        ["conversations", fid, hash] => {
            Ok(WaypointResource::Conversation { fid: parse_fid(fid)?, hash: hash.to_string() })
        },
        ["reactions", "by-fid", fid] => {
            Ok(WaypointResource::ReactionsByFid { fid: parse_fid(fid)? })
        },
        ["reactions", "by-target-cast", fid, hash] => Ok(WaypointResource::ReactionsByTargetCast {
            fid: parse_fid(fid)?,
            hash: hash.to_string(),
        }),
        ["reactions", "by-target-url"] => {
            Ok(WaypointResource::ReactionsByTargetUrl { url: get_url_param()? })
        },
        ["links", "by-fid", fid] => Ok(WaypointResource::LinksByFid { fid: parse_fid(fid)? }),
        ["links", "by-target", fid] => Ok(WaypointResource::LinksByTarget { fid: parse_fid(fid)? }),
        ["links", "compact-state", fid] => {
            Ok(WaypointResource::LinkCompactStateByFid { fid: parse_fid(fid)? })
        },
        ["username-proofs", "by-name", name] => {
            Ok(WaypointResource::UsernameProofByName { name: name.to_string() })
        },
        ["username-proofs", fid] => {
            Ok(WaypointResource::UsernameProofsByFid { fid: parse_fid(fid)? })
        },
        _ => Err(format!("Unsupported resource path: {}", segments.join("/"))),
    }
}

fn parse_fid(segment: &str) -> Result<u64, String> {
    segment.parse::<u64>().map_err(|_| format!("Invalid FID: {segment}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_user_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://users/123").unwrap();
        assert_eq!(result, WaypointResource::UserByFid { fid: 123 });

        let result = parse_waypoint_resource_uri("waypoint:///users/456").unwrap();
        assert_eq!(result, WaypointResource::UserByFid { fid: 456 });
    }

    #[test]
    fn test_parse_user_by_username() {
        let result = parse_waypoint_resource_uri("waypoint://users/by-username/alice").unwrap();
        assert_eq!(result, WaypointResource::UserByUsername { username: "alice".to_string() });
    }

    #[test]
    fn test_parse_verifications() {
        let result = parse_waypoint_resource_uri("waypoint://verifications/123").unwrap();
        assert_eq!(result, WaypointResource::VerificationsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_verification_by_address() {
        let result = parse_waypoint_resource_uri("waypoint://verifications/123/0xabc123").unwrap();
        assert_eq!(
            result,
            WaypointResource::VerificationByAddress { fid: 123, address: "0xabc123".to_string() }
        );
    }

    #[test]
    fn test_parse_all_verification_messages_by_fid() {
        let result =
            parse_waypoint_resource_uri("waypoint://verifications/all-by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::AllVerificationMessagesByFid { fid: 123 });
    }

    #[test]
    fn test_parse_cast() {
        let result = parse_waypoint_resource_uri("waypoint://casts/123/0xabc").unwrap();
        assert_eq!(result, WaypointResource::Cast { fid: 123, hash: "0xabc".to_string() });
    }

    #[test]
    fn test_parse_casts_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::CastsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_casts_by_mention() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-mention/123").unwrap();
        assert_eq!(result, WaypointResource::CastsByMention { fid: 123 });
    }

    #[test]
    fn test_parse_casts_by_parent() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-parent/123/0xabc").unwrap();
        assert_eq!(result, WaypointResource::CastsByParent { fid: 123, hash: "0xabc".to_string() });
    }

    #[test]
    fn test_parse_casts_by_parent_url_query_param() {
        let result = parse_waypoint_resource_uri(
            "waypoint://casts/by-parent-url?url=https%3A%2F%2Fexample.com%2Fpost%2F123",
        )
        .unwrap();
        assert_eq!(
            result,
            WaypointResource::CastsByParentUrl { url: "https://example.com/post/123".to_string() }
        );
    }

    #[test]
    fn test_parse_casts_by_parent_url_missing_param() {
        let result = parse_waypoint_resource_uri("waypoint://casts/by-parent-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing required 'url' query parameter"));
    }

    #[test]
    fn test_parse_conversation() {
        let result = parse_waypoint_resource_uri("waypoint://conversations/123/0xabc").unwrap();
        assert_eq!(result, WaypointResource::Conversation { fid: 123, hash: "0xabc".to_string() });
    }

    #[test]
    fn test_parse_reactions_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://reactions/by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::ReactionsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_reactions_by_target_cast() {
        let result =
            parse_waypoint_resource_uri("waypoint://reactions/by-target-cast/123/0xdef").unwrap();
        assert_eq!(
            result,
            WaypointResource::ReactionsByTargetCast { fid: 123, hash: "0xdef".to_string() }
        );
    }

    #[test]
    fn test_parse_reactions_by_target_url_query_param() {
        let result = parse_waypoint_resource_uri(
            "waypoint://reactions/by-target-url?url=https%3A%2F%2Fwarpcast.com%2F~%2Fchannel%2Ftest",
        )
        .unwrap();
        assert_eq!(
            result,
            WaypointResource::ReactionsByTargetUrl {
                url: "https://warpcast.com/~/channel/test".to_string()
            }
        );
    }

    #[test]
    fn test_parse_reactions_by_target_url_missing_param() {
        let result = parse_waypoint_resource_uri("waypoint://reactions/by-target-url");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing required 'url' query parameter"));
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let result = parse_waypoint_resource_uri("http://users/123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported resource scheme"));
    }

    #[test]
    fn test_parse_invalid_fid() {
        let result = parse_waypoint_resource_uri("waypoint://users/notanumber");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid FID"));
    }

    #[test]
    fn test_parse_missing_path() {
        let result = parse_waypoint_resource_uri("waypoint://");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unsupported_path() {
        let result = parse_waypoint_resource_uri("waypoint://unknown/resource");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported resource path"));
    }

    #[test]
    fn test_parse_links_by_fid() {
        let result = parse_waypoint_resource_uri("waypoint://links/by-fid/123").unwrap();
        assert_eq!(result, WaypointResource::LinksByFid { fid: 123 });
    }

    #[test]
    fn test_parse_links_by_target() {
        let result = parse_waypoint_resource_uri("waypoint://links/by-target/456").unwrap();
        assert_eq!(result, WaypointResource::LinksByTarget { fid: 456 });
    }

    #[test]
    fn test_parse_links_compact_state() {
        let result = parse_waypoint_resource_uri("waypoint://links/compact-state/123").unwrap();
        assert_eq!(result, WaypointResource::LinkCompactStateByFid { fid: 123 });
    }

    #[test]
    fn test_parse_username_proofs() {
        let result = parse_waypoint_resource_uri("waypoint://username-proofs/123").unwrap();
        assert_eq!(result, WaypointResource::UsernameProofsByFid { fid: 123 });
    }

    #[test]
    fn test_parse_username_proof_by_name() {
        let result =
            parse_waypoint_resource_uri("waypoint://username-proofs/by-name/vitalik.eth").unwrap();
        assert_eq!(
            result,
            WaypointResource::UsernameProofByName { name: "vitalik.eth".to_string() }
        );
    }
}
