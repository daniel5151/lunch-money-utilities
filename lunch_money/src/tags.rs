//! Tag schema models and payloads.

/// JSON schemas for tags.
pub mod schemas {
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

    /// Request payload for updating an existing tag.
    #[derive(Serialize, Clone, Debug, Default)]
    pub struct UpdateTagPayload {
        /// If set, the new name of the tag (1-100 characters).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        /// If set, the new description of the tag (up to 200 characters).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub description: Option<Option<String>>,
        /// If set, the new text color of the tag.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub text_color: Option<Option<String>>,
        /// If set, the new background color of the tag.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub background_color: Option<Option<String>>,
        /// If set, will indicate if this tag is archived.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub archived: Option<bool>,
        /// If set, updates the archived timestamp.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub archived_at: Option<Option<jiff::Timestamp>>,
    }

    /// Response returned when a tag deletion fails due to existing dependencies.
    #[derive(Deserialize, Clone, Debug)]
    pub struct DeleteTagDependenciesResponse {
        /// Name of the tag that was attempted to be deleted.
        pub tag_name: String,
        /// Detailed count of dependents blocking deletion.
        pub dependents: TagDependents,
    }

    /// Counts of dependent objects preventing a tag from being deleted.
    #[derive(Deserialize, Clone, Debug)]
    pub struct TagDependents {
        /// Number of rules depending on the tag.
        pub rules: u32,
        /// Number of transactions depending on the tag.
        pub transactions: u32,
    }

    /// Response returned by the delete tag endpoint.
    #[derive(Debug, Clone)]
    pub enum DeleteTagResult {
        /// The tag was deleted successfully.
        Deleted,
        /// The tag could not be deleted because it has dependencies.
        Dependencies(DeleteTagDependenciesResponse),
    }
}
