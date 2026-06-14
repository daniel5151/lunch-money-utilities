use crate::core::TagId;
use serde::Deserialize;
use serde::Serialize;

/// Response payload containing a list of tags.
#[derive(Deserialize, Debug)]
pub struct TagsResponse {
    /// List of tag objects.
    pub tags: Vec<Tag>,
}

/// A Lunch Money tag.
#[derive(Deserialize, Clone, Debug)]
pub struct Tag {
    /// Unique identifier for the tag.
    pub id: TagId,
    /// Name of the tag.
    pub name: String,
    /// Description of the tag.
    pub description: Option<String>,
    /// The text color of the tag.
    pub text_color: Option<String>,
    /// The background color of the tag.
    pub background_color: Option<String>,
    /// The date and time of when the tag was created.
    pub created_at: jiff::Timestamp,
    /// The date and time of when the tag was last updated.
    pub updated_at: jiff::Timestamp,
    /// If true, the tag is archived and hidden in the app UI.
    pub archived: bool,
    /// The date and time of when the tag was last archived.
    pub archived_at: Option<jiff::Timestamp>,
}

/// Request payload for creating a new tag.
#[derive(Serialize, Debug)]
pub struct CreateTagPayload {
    /// Name of the tag (between 1 and 100 characters).
    pub name: String,
    /// Description of the tag (up to 200 characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
