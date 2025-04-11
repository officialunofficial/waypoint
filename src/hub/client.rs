use crate::{
    config::HubConfig,
    hub::stream::EventStream,
    proto::{
        GetInfoRequest, GetInfoResponse, BlocksRequest, ShardChunksRequest, ShardChunksResponse,
        hub_service_client::HubServiceClient,
    },
};
use std::{sync::Arc, time::Duration};
use tokio_stream::Stream;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::info;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Not connected")]
    NotConnected,
    #[error("Transport error: {0}")]
    TransportError(#[from] tonic::transport::Error),
    #[error("gRPC status error: {0}")]
    StatusError(#[from] tonic::Status),
    #[error("Stream error: {0}")]
    StreamError(#[from] crate::redis::error::Error),
}

pub struct Hub {
    client: Option<HubServiceClient<Channel>>,
    config: Arc<HubConfig>,
    host: String,
}

impl Hub {
    pub fn new(config: impl Into<Arc<HubConfig>>) -> Result<Self, Error> {
        let config = config.into();
        let host = config.url.clone();
        Ok(Hub { client: None, config, host })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub async fn connect(&mut self) -> Result<(), Error> {
        info!("Connecting to Farcaster hub at {}", self.config.url);

        let channel = Channel::from_shared(format!("https://{}", self.config.url))
            .map_err(|e| Error::ConnectionError(e.to_string()))?
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .http2_keep_alive_interval(Duration::from_secs(10))
            .http2_adaptive_window(true)
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .timeout(Duration::from_secs(30))
            .connect()
            .await?;

        let client = HubServiceClient::new(channel);
        self.client = Some(client);

        let hub_info = self.get_hub_info().await?;
        info!("Connected to Farcaster hub: {:?}", hub_info);

        Ok(())
    }

    pub fn client(&mut self) -> Option<&mut HubServiceClient<Channel>> {
        self.client.as_mut()
    }

    pub async fn stream(&mut self) -> Result<EventStream, Error> {
        let client = self
            .client
            .as_mut()
            .ok_or(Error::NotConnected)?;
        Ok(EventStream::new(client))
    }

    pub async fn get_blocks(
        &mut self,
        shard_id: u32,
        start_block: u64,
        end_block: Option<u64>,
    ) -> Result<impl Stream<Item = Result<crate::proto::Block, tonic::Status>>, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        
        let request = BlocksRequest {
            shard_id,
            start_block_number: start_block,
            stop_block_number: end_block,
        };
        
        let response = client.get_blocks(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }
    
    pub async fn get_shard_chunks(
        &mut self,
        shard_id: u32,
        start_block: u64,
        end_block: Option<u64>,
    ) -> Result<ShardChunksResponse, Error> {
        let client = self.client.as_mut().ok_or(Error::NotConnected)?;
        
        let request = ShardChunksRequest {
            shard_id,
            start_block_number: start_block,
            stop_block_number: end_block,
        };
        
        let response = client.get_shard_chunks(tonic::Request::new(request)).await?;
        Ok(response.into_inner())
    }

    pub async fn disconnect(&mut self) -> Result<(), Error> {
        self.client = None;
        Ok(())
    }

    pub async fn get_hub_info(&mut self) -> Result<GetInfoResponse, Error> {
        let client = self
            .client
            .as_mut()
            .ok_or(Error::NotConnected)?;

        let request = tonic::Request::new(GetInfoRequest {});
        let response = client.get_info(request).await?;
        Ok(response.into_inner())
    }

    /// Check hub connection by requesting hub info
    pub async fn check_connection(&mut self) -> Result<bool, Error> {
        match self.get_hub_info().await {
            Ok(_) => Ok(true),
            Err(e) => {
                // Try to reconnect on errors
                match self.connect().await {
                    Ok(_) => Ok(true),
                    Err(_) => Err(e), // Return original error
                }
            },
        }
    }
}
