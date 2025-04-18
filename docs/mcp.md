# Model Context Protocol (MCP) Integration

Waypoint includes an MCP (Model Context Protocol) service that allows AI assistants to query Farcaster data. This document describes how to use the MCP service with Waypoint.

## Overview

The Model Context Protocol is a standardized way for AI assistants to interact with external tools and data sources. Waypoint's MCP service provides access to Farcaster data including:

- User profiles
- Verification data (linked wallets)
- Casts (posts)
- Cast replies and mentions

The service exposes a standard MCP interface that can be consumed by AI assistants supporting the protocol.

## Configuration

To enable and configure the MCP service, add the following section to your Waypoint configuration file:

```toml
[mcp]
enabled = true
bind_address = "127.0.0.1"
port = 8000
```

### Configuration Options

- `enabled`: Boolean to enable/disable the MCP service
- `bind_address`: The network address to bind the MCP service to (use `0.0.0.0` to allow external connections)
- `port`: The port to listen on for MCP connections

## Available Tools

The Waypoint MCP service provides the following tools:

### User Data Tools

- `get_user_by_fid`: Retrieve a user's profile information
  - Parameters:
    - `fid`: Farcaster user ID (number)

### Verification Tools

- `get_verifications_by_fid`: Retrieve a user's verified wallet addresses
  - Parameters:
    - `fid`: Farcaster user ID (number)
    - `limit`: Maximum number of results to return (defaults to 10)

### Cast Tools

- `get_cast`: Retrieve a specific cast by its ID
  - Parameters:
    - `fid`: Farcaster user ID of the cast author
    - `hash`: Hash of the cast in hex format

- `get_casts_by_fid`: Retrieve casts created by a specific user
  - Parameters:
    - `fid`: Farcaster user ID
    - `limit`: Maximum number of results to return (defaults to 10)

- `get_casts_by_mention`: Retrieve casts that mention a specific user
  - Parameters:
    - `fid`: Farcaster user ID being mentioned
    - `limit`: Maximum number of results to return (defaults to 10)

- `get_casts_by_parent`: Retrieve replies to a specific cast
  - Parameters:
    - `parent_fid`: Farcaster user ID of the parent cast author
    - `parent_hash`: Hash of the parent cast in hex format
    - `limit`: Maximum number of results to return (defaults to 10)

- `get_casts_by_parent_url`: Retrieve casts that reply to a specific URL
  - Parameters:
    - `parent_url`: URL of the parent content
    - `limit`: Maximum number of results to return (defaults to 10)

- `get_all_casts_by_fid`: Retrieve casts with timestamp filtering
  - Parameters:
    - `fid`: Farcaster user ID
    - `limit`: Maximum number of results to return (defaults to 10)
    - `start_time`: Optional start timestamp (Unix time in seconds)
    - `end_time`: Optional end timestamp (Unix time in seconds)

## Example Responses

### User Data Response

```json
{
  "fid": 1234,
  "display_name": "Example User",
  "bio": "This is my bio",
  "pfp": "https://example.com/avatar.png",
  "username": "example",
  "location": "San Francisco",
  "url": "https://example.com"
}
```

### Verifications Response

```json
{
  "fid": 1234,
  "count": 2,
  "verifications": [
    {
      "fid": 1234,
      "address": "0x1234567890abcdef1234567890abcdef12345678",
      "protocol": "ethereum",
      "type": "eoa",
      "timestamp": 1672531200
    },
    {
      "fid": 1234,
      "address": "0xabcdef1234567890abcdef1234567890abcdef12",
      "protocol": "ethereum",
      "type": "contract",
      "chain_id": 1,
      "timestamp": 1672617600
    }
  ]
}
```

### Cast Response

```json
{
  "fid": 1234,
  "timestamp": 1672531200,
  "hash": "0x1234567890abcdef1234567890abcdef12345678",
  "text": "This is a sample cast",
  "mentions": [5678, 9012],
  "mentions_positions": [0, 10],
  "embeds": [
    {
      "type": "url",
      "url": "https://example.com"
    }
  ],
  "parent": {
    "type": "url",
    "url": "https://example.com/parent"
  }
}
```

### Multiple Casts Response

```json
{
  "fid": 1234,
  "count": 2,
  "casts": [
    {
      "fid": 1234,
      "timestamp": 1672531200,
      "hash": "0x1234567890abcdef1234567890abcdef12345678",
      "text": "This is a sample cast"
    },
    {
      "fid": 1234,
      "timestamp": 1672617600,
      "hash": "0xabcdef1234567890abcdef1234567890abcdef12",
      "text": "This is another sample cast"
    }
  ]
}
```

## Using with AI Assistants

To use Waypoint's MCP service with an AI assistant, you'll need to:

1. Ensure the MCP service is enabled and running in Waypoint
2. Configure your AI assistant to connect to the MCP service
3. Use the provided tools to query Farcaster data

Example prompts for AI assistants will vary depending on the specific assistant and MCP integration method.

## Security Considerations

- The MCP service does not currently implement authentication
- When exposing the service externally, consider using a reverse proxy with authentication
- Limit the `bind_address` to localhost (`127.0.0.1`) if the service should only be accessible locally