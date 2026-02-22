//! Transport-neutral query DTOs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversationParams {
    pub recursive: bool,
    pub max_depth: usize,
    pub limit: usize,
}
