macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<u64> for $name {
            fn from(val: u64) -> Self {
                Self(val)
            }
        }
    };
}

define_id!(
    TransactionId,
    "Type-safe identifier for Lunch Money transactions."
);
define_id!(
    CategoryId,
    "Type-safe identifier for Lunch Money categories."
);
define_id!(
    ManualAccountId,
    "Type-safe identifier for Lunch Money manual accounts."
);
define_id!(
    PlaidAccountId,
    "Type-safe identifier for Lunch Money Plaid accounts."
);
define_id!(TagId, "Type-safe identifier for Lunch Money tags.");
define_id!(
    RecurringId,
    "Type-safe identifier for Lunch Money recurring items."
);
define_id!(
    AttachmentId,
    "Type-safe identifier for transaction attachments."
);
define_id!(UserId, "Type-safe identifier for a Lunch Money user.");
