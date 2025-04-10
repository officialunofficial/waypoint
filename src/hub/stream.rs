use crate::proto::hub_service_client::HubServiceClient;
use tonic::transport::Channel;

pub struct EventStream {
    _client: HubServiceClient<Channel>,
}

impl EventStream {
    pub fn new(client: &mut HubServiceClient<Channel>) -> Self {
        Self { _client: client.clone() }
    }
}
