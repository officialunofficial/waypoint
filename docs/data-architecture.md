# Waypoint Data Architecture

This document provides a UML class diagram and explanation of Waypoint's data architecture, which follows the DataContext pattern for data access.

## Class Diagram

```mermaid
%%{init: {'theme': 'base', 'themeVariables': { 'background': '#f5f5f5' }}}%%
classDiagram
    class Message {
        +MessageId id
        +MessageType message_type
        +Vec~u8~ payload
        +new(id, message_type, payload)
    }

    class MessageId {
        +String value
        +value(): String
    }

    class MessageType {
        <<enumeration>>
        Cast
        Reaction
        Verification
        UserData
        LinkAdd
        LinkRemove
        UsernameProof
    }

    class Fid {
        +u64 value
        +value(): u64
        +from(value: u64): Fid
    }

    class DataAccessError {
        <<enumeration>>
        +Database(sqlx::Error)
        +Redis(String)
        +NotFound(String)
        +Hub(String)
        +Other(String)
    }

    class Database {
        <<interface>>
        +store_message(message: Message): Result
        +get_message(id: &MessageId, message_type: MessageType): Result~Message~
        +get_messages_by_fid(fid: Fid, message_type: MessageType, limit: usize, cursor: Option~MessageId~): Result~Vec~Message~~
        +delete_message(id: &MessageId, message_type: MessageType): Result
    }

    class HubClient {
        <<interface>>
        +get_user_data_by_fid(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_user_data(fid: Fid, data_type: &str): Result~Option~Message~~
        +get_username_proofs_by_fid(fid: Fid): Result~Vec~Message~~
        +get_verifications_by_fid(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_casts_by_fid(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_cast(fid: Fid, hash: &[u8]): Result~Option~Message~~
        +get_casts_by_mention(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_casts_by_parent(parent_fid: Fid, parent_hash: &[u8], limit: usize): Result~Vec~Message~~
        +get_casts_by_parent_url(parent_url: &str, limit: usize): Result~Vec~Message~~
        +get_all_casts_by_fid(fid: Fid, limit: usize, start_time: Option~u64~, end_time: Option~u64~): Result~Vec~Message~~
    }

    class DataContext~DB, HC~ {
        -DB database
        -HC hub_client
        +new(database: DB, hub_client: HC): Self
        +get_user_data_by_fid(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_user_data(fid: Fid, data_type: &str): Result~Option~Message~~
        +get_username_proofs_by_fid(fid: Fid): Result~Vec~Message~~
        +get_verifications_by_fid(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_casts_by_fid(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_cast(fid: Fid, hash: &[u8]): Result~Option~Message~~
        +get_casts_by_mention(fid: Fid, limit: usize): Result~Vec~Message~~
        +get_casts_by_parent(parent_fid: Fid, parent_hash: &[u8], limit: usize): Result~Vec~Message~~
        +get_casts_by_parent_url(parent_url: &str, limit: usize): Result~Vec~Message~~
        +get_all_casts_by_fid(fid: Fid, limit: usize, start_time: Option~u64~, end_time: Option~u64~): Result~Vec~Message~~
    }

    class DataContextBuilder~DB, HC~ {
        -Option~DB~ database
        -Option~HC~ hub_client
        +new(): Self
        +with_database(database: DB): Self
        +with_hub_client(hub_client: HC): Self
        +build(): DataContext~DB, HC~
    }

    class FarcasterHubClient {
        -Hub hub
        +new(hub: Arc~Hub~): Self
    }

    class Hub {
        +Option~Channel~ channel
        +Option~HubServiceClient~ client
        +Arc~HubConfig~ config
        +String host
        +new(config: Arc~HubConfig~): Result~Hub, Error~
        +connect(): Result
        +stream(): Result~EventStream, Error~
        +get_hub_info(): Result~GetInfoResponse~
        +get_fids(page_size: Option~u32~, page_token: Option~Vec~u8~~, reverse: Option~bool~): Result~FidsResponse~
    }
    
    class EventStream {
        +subscribe(): Result~impl Stream~HubEvent~, Error~
    }

    class HubConfig {
        +String url
        +u32 retry_max_attempts
        +u64 retry_base_delay_ms
        +u64 retry_max_delay_ms
        +f32 retry_jitter_factor
    }

    class PostgresDatabase {
        -Pool pool
        +new(pool: Pool): Self
    }

    %% Object inheritance
    Database <|.. PostgresDatabase
    HubClient <|.. FarcasterHubClient
    
    %% Object composition/association
    Message o-- MessageId
    Message o-- MessageType
    DataContext o-- Database
    DataContext o-- HubClient
    Hub o-- HubConfig
    Hub o-- EventStream
    FarcasterHubClient o-- Hub

    %% Usage relationships
    DataContextBuilder --> DataContext
    PostgresDatabase --> Message
    PostgresDatabase --> MessageId
    PostgresDatabase --> MessageType
    PostgresDatabase --> Fid
    PostgresDatabase --> DataAccessError
    FarcasterHubClient --> Message
    FarcasterHubClient --> MessageId
    FarcasterHubClient --> Fid
    FarcasterHubClient --> DataAccessError
```

## Architecture Overview

Waypoint's data architecture follows the DataContext pattern to provide unified data access across different sources. This architecture enables:

1. Clean abstraction of data access logic
2. Consistent interfaces for both database and hub operations
3. Flexible composition through dependency injection
4. Testability with mock implementations
5. Configurable storage efficiency with optional raw message storage

### Key Components

#### Domain Entities

- **Message**: The core entity representing a Farcaster message
- **MessageId**: Unique identifier for messages
- **MessageType**: Enumeration of message types (Cast, Reaction, etc.)
- **Fid**: Farcaster ID representing a user

#### Core Interfaces

- **Database**: Interface for local database storage and retrieval
- **HubClient**: Interface for accessing the Farcaster Hub API

#### DataContext Pattern

- **DataContext**: Main data access layer that coordinates between local database and hub client
- **DataContextBuilder**: Builder pattern for creating DataContext instances with different components

#### Concrete Implementations

- **PostgresDatabase**: PostgreSQL implementation of the Database interface
- **FarcasterHubClient**: Implementation of HubClient for Farcaster Hub communication
- **Hub**: Low-level client for gRPC communication with the Hub

### Design Patterns

1. **DataContext Pattern**: Centralizes data access operations and coordinates between different data sources
2. **Builder Pattern**: Simplifies the creation of complex DataContext objects
3. **Dependency Injection**: Interfaces injected into services that need them
4. **Error Handling**: Unified error type (DataAccessError) for all data operations

### Data Flow

1. Services request data from the DataContext
2. DataContext determines whether to fetch from local database or hub
3. If data exists locally, it's returned from the database
4. If not, it's fetched from the Farcaster Hub
5. Results are processed and returned to the service layer

This architecture allows Waypoint to efficiently manage data access across multiple sources while maintaining a consistent interface for the rest of the application.

### Storage Optimization

Waypoint includes configurable storage options to optimize database usage:

#### Message Storage

By default, Waypoint stores all messages in the `messages` table for maximum data fidelity. However, this can be disabled to save storage space:

```toml
[database]
store_messages = false
```

When `store_messages` is set to `false`, the system will:

1. Skip storing any data in the `messages` table completely
2. Still store the processed data in type-specific tables (casts, reactions, etc.)
3. Significantly reduce database size for high-volume installations

This is particularly useful for deployments that:
- Have limited storage capacity
- Process a high volume of messages
- Don't require the messages table for compliance or recovery purposes
- Only need the structured data in the type-specific tables

The processed data (casts, reactions, etc.) remains fully available regardless of this setting.