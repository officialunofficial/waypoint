use crate::hub::client::AuthenticatedHubServiceClient;

pub struct EventStream {
    _client: AuthenticatedHubServiceClient,
}

impl EventStream {
    pub fn new(client: &mut AuthenticatedHubServiceClient) -> Self {
        Self { _client: client.clone() }
    }
}
