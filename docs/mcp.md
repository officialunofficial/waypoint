# Model Context Protocol (MCP) Integration Guide

Waypoint now supports the Model Context Protocol (MCP), allowing AI assistants to query Snapchain data directly through a standardized interface.

## What is MCP?

The Model Context Protocol (MCP) is a specification that allows AI assistants to communicate with external tools using a standardized JSON-RPC based protocol. This integration lets AI agents query real-time Snapchain data from your Waypoint instance.

The Waypoint MCP integration includes a customizable prompt system that supports preserving Ethereum Name Service (ENS) domains (like "vitalik.eth") when working with Farcaster usernames.

## Configuration

To enable MCP integration, configure the following settings in your `config.toml` file or through environment variables:

```toml
[mcp]
# Enable MCP integration
enabled = true
# Bind address for the MCP server
bind_address = "0.0.0.0"
# Port for the MCP server
port = 8000
```

Or use environment variables:

```bash
WAYPOINT_MCP__ENABLED=true
WAYPOINT_MCP__BIND_ADDRESS=0.0.0.0
WAYPOINT_MCP__PORT=8000
```

## Transport Protocol

The MCP service uses Server-Sent Events (SSE) over HTTP:

- **WaypointMcpTools**:
  - Full-featured service with access to Snapchain/Farcaster data
  - Accessible at `http://<host>:<port>/sse`
  - Provides tools for accessing Farcaster user data, casts, and verifications
  - Configure with `bind_address` and `port` settings

## Architecture

The MCP integration in Waypoint is implemented as a separate service that:

1. Starts an MCP-compatible server using the SSE protocol
2. Exposes tools for querying data
3. Handles AI assistant requests through the MCP protocol

The MCP service is automatically started when you run Waypoint via `waypoint start` or Docker Compose, as long as it's enabled in the configuration.

### Data Access

The MCP service uses the Data Context pattern for efficient and flexible data access:

- **Unified Data Access**: Accesses data from both Snapchain Hub and PostgreSQL database through a single interface
- **Prioritized Data Sources**: Always tries to fetch fresh data from the Hub first, with database fallback
- **Efficient Resource Management**: Shares database and Hub client connections between requests
- **Type-Safe Interfaces**: Uses Rust's trait system for clean abstractions

The Data Context pattern is detailed in the [Architecture Documentation](architecture.md#data-architecture).

## Available Tools

The Waypoint MCP integration provides the following tools to AI assistants:

> **Note**: This implementation provides access to Farcaster user data, casts, reactions, links, and verifications via a comprehensive interface. It allows accessing users by FID or username and supports exploring the social graph through follow relationships.

### User Tools

#### Get User by FID


```json
{
  "method": "callTool",
  "params": {
    "name": "get_user_by_fid",
    "input": {
      "fid": 12345
    }
  }
}
```

The response includes a JSON object with all available user data fields:

```json
{
  "fid": 12345,
  "display_name": "Alex Smith",
  "username": "alex",
  "bio": "Building on Farcaster",
  "pfp": "https://example.com/avatar.jpg",
  "url": "https://example.com",
  "location": "San Francisco, CA",
  "twitter": "alexsmith",
  "github": "asmith"
}
```

The implementation fetches up to 20 UserData messages for the specified FID and maps each type to the appropriate field in the response. Fields not set by the user will be omitted from the response.

#### Get User Verifications

Retrieve verified wallet addresses for a user. This tool fetches verification messages from the Hub and provides detailed information about each verified address.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_verifications_by_fid",
    "input": {
      "fid": 12345,
      "limit": 10
    }
  }
}
```

The response includes detailed information about each verification:

```json
{
  "fid": 12345,
  "count": 2,
  "verifications": [
    {
      "fid": 12345,
      "address": "0x1a2b3c4d5e6f7890abcdef1234567890abcdef12",
      "protocol": "ethereum",
      "type": "eoa",
      "timestamp": 1672531200
    },
    {
      "fid": 12345,
      "address": "0xabcdef1234567890abcdef1234567890abcdef12",
      "protocol": "ethereum",
      "type": "contract",
      "chain_id": 1,
      "timestamp": 1672617600
    }
  ]
}
```

The response includes:
- Protocol type (ethereum, solana)
- Verification type (eoa for personal wallets, contract for smart contracts)
- Chain ID (for contract verifications)
- Timestamp when the verification was created

#### Get Casts by User

Retrieve casts (posts) from a specific user.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_casts_by_fid",
    "input": {
      "fid": 12345,
      "limit": 10
    }
  }
}
```

The response includes an array of casts with detailed information:

```json
{
  "fid": 12345,
  "count": 2,
  "casts": [
    {
      "fid": 12345,
      "hash": "0x1a2b3c4d5e6f...",
      "timestamp": 1672531200,
      "text": "This is a sample cast with #hashtags and @mentions",
      "mentions": [456, 789],
      "mentions_positions": [32, 42],
      "embeds": [
        {
          "type": "url",
          "url": "https://example.com/article"
        }
      ]
    },
    {
      "fid": 12345,
      "hash": "0xabcdef1234...",
      "timestamp": 1672444800,
      "text": "This is a reply to another cast",
      "parent": {
        "type": "cast",
        "fid": 789,
        "hash": "0x9876543210..."
      }
    }
  ]
}
```

#### Get Specific Cast

Retrieve a specific cast by its author FID and hash.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_cast",
    "input": {
      "fid": 12345,
      "hash": "0x1a2b3c4d5e6f7890abcdef1234567890abcdef12"
    }
  }
}
```

#### Get Cast Mentions

Retrieve casts that mention a specific user.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_casts_by_mention",
    "input": {
      "fid": 12345,
      "limit": 10
    }
  }
}
```

#### Get Cast Replies

Retrieve replies to a specific cast.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_casts_by_parent",
    "input": {
      "parent_fid": 12345,
      "parent_hash": "0x1a2b3c4d5e6f7890abcdef1234567890abcdef12",
      "limit": 10
    }
  }
}
```

#### Get URL Replies

Retrieve casts that reply to a specific URL.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_casts_by_parent_url",
    "input": {
      "parent_url": "https://example.com/article",
      "limit": 10
    }
  }
}
```

#### Get All Casts with Time Filtering

Retrieve casts from a user with optional timestamp filtering.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_all_casts_by_fid",
    "input": {
      "fid": 12345,
      "limit": 20,
      "start_time": 1672531200,
      "end_time": 1682531200
    }
  }
}
```

#### Get User by Username

Find a user's profile by their Farcaster username instead of FID.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_user_by_username",
    "input": {
      "username": "alice"
    }
  }
}
```

The response includes the same comprehensive profile information as get_user_by_fid, but allows searching by username instead of requiring an FID.

#### Get FID by Username

Find a user's FID by their Farcaster username.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_fid_by_username",
    "input": {
      "username": "alice"
    }
  }
}
```

Response:
```json
{
  "username": "alice",
  "fid": 12345,
  "found": true
}
```

#### Get Link (Follow Relationship)

Check if a user follows another user (or any other link type relationship).

```json
{
  "method": "callTool",
  "params": {
    "name": "get_link",
    "input": {
      "fid": 12345,
      "target_fid": 6789
    }
  }
}
```

The `link_type` defaults to "follow" if not specified, making it easy to check follow relationships.

#### Get Links by FID (Who a User Follows)

Find users that a specific user follows.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_links_by_fid",
    "input": {
      "fid": 12345,
      "limit": 20
    }
  }
}
```

By default, this returns "follow" type links unless a different link_type is specified.

#### Get Links by Target (Who Follows a User)

Find users that follow a specific user.

```json
{
  "method": "callTool",
  "params": {
    "name": "get_links_by_target",
    "input": {
      "target_fid": 12345,
      "limit": 20
    }
  }
}
```

By default, this returns "follow" type links unless a different link_type is specified.


## Using the Waypoint Prompt

The Waypoint MCP integration includes a customizable prompt for AI assistants:

```json
{
  "method": "getPrompt",
  "params": {
    "name": "waypoint_prompt",
    "arguments": {
      "fid": 12345,
      "username": "vitalik.eth"
    }
  }
}
```

The prompt supports two parameters:
- `fid` (required): The Farcaster ID to focus on
- `username` (optional): The username associated with the FID, including full ENS domains

This provides the assistant with a context-aware prompt for exploring Farcaster data about the specified user, preserving the complete username including any ".eth" domain.

## Example Usage

AI assistants can use these tools to query Snapchain data:

### Getting User Information

```
User: "Find me information about Farcaster user with FID 12345"

AI: "Let me fetch information about that Farcaster user for you."
    [AI uses the get_user_by_fid tool]
    
    "Here's what I found for user with FID 12345:
     - Display name: Alex Farcaster
     - Username: alexfarcaster
     - Bio: Building the future of decentralized social
     - Profile picture: https://example.com/pfp.jpg
     - URL: https://example.com
     - Location: San Francisco, CA
     - Twitter: @alexfarcaster
     - GitHub: alexfarcaster"
```

The `get_user_by_fid` tool fetches all UserData messages for the specified FID and converts them into a structured JSON object. Each UserData message contains a specific piece of user information based on its type (e.g., display name, bio, profile picture URL, etc.), which is then mapped to the appropriate field in the response. This implementation handles the decoding of the protobuf messages and provides a clean, standardized interface for AI assistants to access user profile data.

### Getting a User's Verified Wallets

```
User: "What wallets does the user with FID 12345 have verified?"

AI: "Let me check which wallet addresses this user has verified on Farcaster."
    [AI uses the get_verifications_by_fid tool with the FID]
    
    "User 12345 has verified the following wallet addresses:
     - Ethereum EOA wallet: 0x1a2b3c4d5e6f... (verified on Jan 1, 2023)
     - Ethereum contract wallet: 0xabcdef1234... on chain ID 1 (verified on Jan 2, 2023)
     - Solana wallet: AbCdEfGhIjK... (verified on Jan 3, 2023)
    "
```

The `get_verifications_by_fid` tool fetches verification messages from the Hub for the specified FID and processes them to extract the address, protocol type, verification type, and other metadata. This gives AI assistants complete information about the user's verified addresses, letting them provide rich context about the user's blockchain presence.

### Getting a User's Recent Casts

```
User: "Show me the recent posts from user with FID 12345"

AI: "Let me fetch the most recent casts from this Farcaster user."
    [AI uses the get_casts_by_fid tool with the FID]
    
    "Here are the most recent posts from user 12345:
     - 'Just discovered this amazing new DeFi protocol...' (posted Jan 1, 2023)
     - 'Anyone going to the ETH conference next month?' (posted Dec 30, 2022)
     - 'My thoughts on the latest crypto market trends...' (posted Dec 25, 2022)
    "
```

### Finding Casts That Mention a User

```
User: "Who has mentioned FID 12345 recently?"

AI: "Let me search for recent mentions of this Farcaster user."
    [AI uses the get_casts_by_mention tool with the FID]
    
    "Here are recent mentions of user 12345:
     - FID 789: 'Enjoying the conversation with @12345 about governance models' (posted Jan 5, 2023)
     - FID 456: 'Great thread by @12345 on token economics' (posted Jan 3, 2023)
     - FID 123: 'Agree with @12345's take on this topic' (posted Dec 28, 2022)
    "
```

### Finding Replies to a Specific Cast

```
User: "Show me all the replies to the cast with FID 12345 and hash 0x1a2b3c4d..."

AI: "Let me fetch the replies to this specific cast."
    [AI uses the get_casts_by_parent tool with the parent FID and hash]
    
    "Here are the replies to that cast:
     - FID 456: 'This is a really insightful point!'
     - FID 789: 'I had a similar experience with this'
     - FID 101: 'Could you elaborate more on the second part?'
    "
```

### Finding Casts During a Specific Time Period

```
User: "What did FID 12345 post during the first week of January 2023?"

AI: "Let me search for posts during that specific time period."
    [AI uses the get_all_casts_by_fid tool with start_time and end_time parameters]
    
    "During the first week of January 2023, user 12345 posted:
     - Jan 7: 'Wrapping up a productive week of building'
     - Jan 5: 'Just pushed a major update to my project'
     - Jan 3: 'Happy new year everyone! Looking forward to building in 2023'
    "
```

### Finding a User by Their Username

```
User: "Can you find information about the Farcaster user @alice?"

AI: "Let me look up that username on Farcaster."
    [AI uses the get_user_by_username tool with username "alice"]
    
    "Here's what I found for @alice on Farcaster:
     - FID: 12345
     - Display name: Alice Builder
     - Bio: Building cool things with Farcaster
     - Location: New York, NY
     - Twitter: @alicebuilder
     - Website: https://alice.dev
    "
```

### Checking Follow Relationships

```
User: "Does user @alice follow user @bob on Farcaster?"

AI: "Let me check if Alice follows Bob on Farcaster."
    [AI first uses get_fid_by_username for "alice" and then for "bob"]
    [Then AI uses get_link with Alice's FID as source and Bob's FID as target]
    
    "Yes, Alice (FID 12345) follows Bob (FID 6789) on Farcaster."
```

```
User: "Who follows @alice on Farcaster?"

AI: "Let me find who follows Alice on Farcaster."
    [AI uses get_fid_by_username to get Alice's FID]
    [Then AI uses get_links_by_target with Alice's FID]
    
    "Alice has 5 followers on Farcaster:
     - Bob (FID 6789)
     - Carol (FID 2468)
     - Dave (FID 1357)
     - Eve (FID 9876)
     - Frank (FID 5432)
    "
```

## Connecting AI Assistants to Waypoint MCP

AI assistants can connect to Waypoint's MCP service in several ways:

### HTTP Mode Connection

Connect to the MCP service using the SSE endpoint:

```
http://waypoint-host:8000/sse
```

Example connection using the MCP client library:
```javascript
import { createClient } from "@modelcontextprotocol/client";

const client = createClient({
  url: "http://waypoint-host:8000/sse"
});

// List available tools
const toolList = await client.listTools();
console.log(toolList); // Will show get_user_by_fid, get_verifications, get_casts_by_user tools

// Get user profile data
const userData = await client.callTool({
  name: "get_user_by_fid",
  input: { fid: 12345 }
});
console.log(userData);

// Get recent casts from a user
const casts = await client.callTool({
  name: "get_casts_by_user",
  input: { fid: 12345, limit: 5 }
});
console.log(casts);
```


### Docker Setup

When using Docker Compose, the MCP service is already configured and exposed:

- MCP Service: `http://localhost:8000/sse`

The service is enabled by default and will automatically start with Waypoint.

## Extending the MCP Integration

Developers can extend Waypoint's MCP capabilities by adding more tools to the `src/services/mcp.rs` file:

1. Define new data structures for tool inputs/outputs
2. Implement the tool functionality in the `WaypointMcpService<DB, HC>` implementation
3. Add a delegate method in the `WaypointMcpTools` wrapper to use the `#[tool]` macro
4. Use the DataContext for data access to benefit from the abstraction

The recently implemented `do_get_user_by_fid` function demonstrates this pattern:

1. It receives a Farcaster ID (FID) parameter
2. Uses the `DataContext` to make a gRPC request to the Hub via `get_user_data_by_fid`
3. Processes the returned `MessagesResponse` by decoding the protobuf messages 
4. Extracts the relevant data from each `UserDataBody` based on its type
5. Formats the data into a clean JSON structure for the client
6. Handles potential errors gracefully with descriptive messages

This layered approach promotes clean separation of concerns:
- The `WaypointMcpTools` class handles the MCP protocol interface
- The `WaypointMcpService` implements the business logic
- The `DataContext` provides data access abstraction
- The Hub/Database clients handle the actual data retrieval

Example of adding a new tool:

```rust
// 1. First, add the request structure
#[derive(Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct SearchCastsRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

// 2. Add the implementation in WaypointMcpService
impl<DB, HC> WaypointMcpService<DB, HC>
where
    DB: crate::core::data_context::Database + Clone + Send + Sync + 'static,
    HC: crate::core::data_context::HubClient + Clone + Send + Sync + 'static,
{
    // Implementation method with business logic
    async fn do_search_casts(&self, query: &str, limit: usize) -> String {
        info!("MCP: Searching casts for query: {}", query);
        
        // Use the Data Context to access multiple data sources
        match self.data_context.search_casts(query, limit).await {
            Ok(messages) => {
                let results = messages.iter()
                    .map(|message| Self::message_to_json(message))
                    .collect::<Vec<_>>();
                
                let result = serde_json::json!({
                    "query": query,
                    "results": results
                });
                
                result.to_string()
            },
            Err(e) => format!("Error searching casts: {}", e)
        }
    }
}

// 3. Add the tool wrapper in WaypointMcpTools
#[tool(tool_box)]
impl WaypointMcpTools {
    // Existing tools...
    
    #[tool(description = "Search for casts matching a query")]
    async fn search_casts(
        &self,
        #[tool(param)] request: SearchCastsRequest,
    ) -> String {
        // Delegate to the implementation in WaypointMcpService
        self.service.do_search_casts(&request.query, request.limit).await
    }
}
```

To add a completely new data access method:

1. Add a new method to the Database and/or HubClient traits in `src/core/data_context.rs`
2. Implement the method in the appropriate provider (`src/database/providers.rs` or `src/hub/providers.rs`)
3. Add the corresponding method to the DataContext struct
4. Use the new method in your MCP tool

## Troubleshooting

If you encounter issues with the MCP integration:

1. Check that MCP is enabled in the configuration (`enabled = true`)
2. Verify the bind address and port settings are correct
3. Ensure the port (8000 by default) is correctly exposed in Docker or your deployment environment
4. Check Waypoint logs for any MCP-related error messages
5. Make sure your Farcaster Hub connection is working
6. Verify that the service started successfully by checking the logs for "MCP service started" messages
7. Test the connection with a simple MCP client tool

For more information on MCP, visit the [Model Context Protocol specification](https://spec.modelcontextprotocol.io/)