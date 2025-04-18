# Waypoint Architecture

This document provides an overview of Waypoint's architecture, which consists of three main components:
1. A streaming service for processing real-time Snapchain events through Redis consumer groups
2. A backfill system for historical data processing using a FID-based approach
3. A Model Context Protocol (MCP) service for AI assistants to access Farcaster data

For details on the data architecture and DataContext pattern, see [data-architecture.md](data-architecture.md).

## Streaming Service Architecture

```mermaid
%%{init: {'theme': 'base', 'themeVariables': { 'background': '#f5f5f5' }}}%%
sequenceDiagram
    participant Hub as Snapchain
    participant Subscriber as HubSubscriber
    participant Redis as Redis Streams
    participant Consumer as StreamingService Consumer
    participant Processor as Event Processors
    participant DB as PostgreSQL Database

    rect rgb(191, 223, 255)
    note right of Hub: HubEvent Stream Processing
    
    Hub->>Subscriber: gRPC Event Stream (SubscribeRequest)
    activate Subscriber
    
    loop For Each Stream Event
        Hub->>Subscriber: HubEvent (MergeMessage, PruneMessage, etc.)
        Subscriber->>Subscriber: Filter spam events
        Subscriber->>Subscriber: Group by event type (casts, reactions, etc.)
        
        par Publish to Multiple Streams
            Subscriber->>Redis: Publish to "casts" stream
            Subscriber->>Redis: Publish to "reactions" stream
            Subscriber->>Redis: Publish to "links" stream
            Subscriber->>Redis: Publish to "user_data" stream
            Subscriber->>Redis: Publish to "verifications" stream
            Subscriber->>Redis: Publish to "username_proofs" stream
            Subscriber->>Redis: Publish to "onchain:*" streams
        end
        
        Subscriber->>Redis: Store last processed event ID
    end
    deactivate Subscriber
    end

    rect rgb(255, 204, 204)
    note right of Consumer: Consumer Group Processing
    
    par For Each Message Type
        Consumer->>Redis: Create consumer group
        
        loop Process Messages
            Consumer->>Redis: Reserve messages from stream (XREADGROUP)
            Redis->>Consumer: Batch of stream entries
            
            Consumer->>Processor: Process message batch
            
            par Process with Multiple Processors
                Processor->>Processor: DatabaseProcessor processes events
                Processor->>DB: Store events in database
                Processor->>Processor: PrintProcessor (debug logging)
            end
            
            Processor->>Consumer: Processing results
            Consumer->>Redis: Acknowledge messages (XACK)
            
            Consumer->>Redis: Claim stale messages (XCLAIM)
            Consumer->>Redis: Process claimed messages
        end
    end
    
    par Cleanup Tasks
        Consumer->>Redis: Trim old events (XTRIM)
    end
    end
```

## Component Descriptions

### 1. HubSubscriber

The HubSubscriber component establishes a gRPC connection to a Snapchain node and consumes the event stream in real-time:

- Connects to the node using a gRPC streaming API
- Subscribes to specific event types (MergeMessage, PruneMessage, RevokeMessage, etc.)
- Filters spam messages (currently pulling from Warpcast labels, but will be expanded to include other spam detection methods)
- Groups events by type (casts, reactions, links, etc.)
- Publishes events to Redis streams
- Tracks the last processed event ID for resuming after restarts

### 2. Redis Streams

Redis streams serve as a durable message queue between the HubSubscriber and Consumer:

- Provides persistent storage for events in transit
- Enables backpressure handling through consumer groups
- Maintains separate streams for different event types
- Supports event acknowledgment and claiming of stale messages
- Allows trimming of old events to manage memory usage

### 3. StreamingService Consumer

The Consumer component processes events from Redis streams:

- Creates consumer groups for each message type
- Reads messages in batches for efficient processing
- Handles concurrent processing of different message types
- Manages acknowledgments of successfully processed messages
- Claims and reprocesses stale/stuck messages
- Implements graceful shutdown procedures

### 4. Event Processors

Processors handle the actual business logic for the events:

- **DatabaseProcessor**: Persists events to PostgreSQL
  - Handles different message types (casts, reactions, etc.)
  - Manages transaction boundaries
  - Implements upsert logic for existing records

- **PrintProcessor**: Optional debug processor
  - Logs events for debugging and monitoring
  - Can be enabled via configuration

### 5. Database (PostgreSQL)

The PostgreSQL database is the final destination for processed events:

- Stores normalized Farcaster data
- Provides relational model for querying
- Supports vector extensions for similarity search
- Maintains indexes for efficient querying

## Backfill System Architecture

### FID-based Backfill

The FID-based approach processes messages by Farcaster user ID (FID):

```mermaid
%%{init: {'theme': 'base', 'themeVariables': { 'background': '#f5f5f5' }}}%%
sequenceDiagram
    participant Queue as Redis Queue
    participant Worker as FID Worker
    participant Hub as Snapchain Hub
    participant Processor as Database Processor
    participant DB as PostgreSQL

    Queue->>Worker: BackfillJob with FIDs [1,2,3,...]
    
    loop For Each FID
        Worker->>Hub: Request all message types for FID
        Hub->>Worker: Messages (casts, reactions, etc.)
        
        par Process Different Message Types
            Worker->>Processor: Process casts
            Worker->>Processor: Process reactions
            Worker->>Processor: Process links
            Worker->>Processor: Process user_data
            Worker->>Processor: Process verifications
        end
        
        Processor->>DB: Store processed messages
    end
    
    Worker->>Queue: Mark job as completed
```

## MCP Service Architecture

The MCP (Model Context Protocol) service enables AI assistants to access Farcaster data through a standardized protocol.

```mermaid
%%{init: {'theme': 'base', 'themeVariables': { 'background': '#f5f5f5' }}}%%
sequenceDiagram
    participant AI as AI Assistant
    participant MCP as MCP Service
    participant DC as DataContext
    participant Hub as Farcaster Hub
    participant DB as PostgreSQL Database

    AI->>MCP: Connect to MCP service
    MCP->>AI: Provide available tools

    rect rgb(191, 223, 255)
    note right of AI: Tool Usage Flow
    
    AI->>MCP: Call tool (e.g., get_user_by_fid)
    
    MCP->>DC: Request data via DataContext
    
    alt Data in Database
        DC->>DB: Query stored data
        DB->>DC: Return stored data
    else Data from Hub
        DC->>Hub: Request from Farcaster Hub
        Hub->>DC: Return data from Hub
    end
    
    DC->>MCP: Process and format data
    MCP->>AI: Return structured JSON response
    
    end
```

### MCP Service Components

- **MCP Service Handler**: Implements the Model Context Protocol server
- **Tool Implementations**: Methods for accessing different types of Farcaster data
- **DataContext Integration**: Uses the DataContext pattern to access data
- **Response Formatting**: Converts Farcaster data into structured JSON responses
- **Protocol Support**: Implements SSE transport for server-sent events

## Key Features

- **FID-based Backfill**: Optimized for targeted user data processing
- **MCP Integration**: Provides AI assistants with structured access to Farcaster data
- **Memory efficient**: Optimized Snapchain event processing
- **Efficient Buffer Management**: Carefully managed memory allocations
- **Batch Processing**: Processes events in batches for efficiency
- **Concurrency Control**: Manages parallel processing with semaphores
- **Error Handling**: Implements retries with exponential backoff
- **Graceful Shutdown**: Proper shutdown sequence for minimal data loss
- **Connection Monitoring**: Detects and recovers from stale connections
- **Dead Letter Queuing**: Optionally moves problematic messages to dead letter queues
- **Checkpointing**: Tracks progress for resumable operations