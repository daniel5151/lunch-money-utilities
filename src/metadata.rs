use crate::api::Currency;
use jiff::Timestamp;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[expect(clippy::large_enum_variant)]
pub enum LunchMoneyTxMetadata {
    Import {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        delta_transaction_ids: Vec<u64>,
        original: LunchMoneyTxMetadataExpense,
    },
    Delta {
        original_transaction_id: u64,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LunchMoneyTxMetadataExpense {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub friendship_id: Option<u64>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub payment: bool,
    #[serde(
        default,
        with = "rust_decimal::serde::str_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub cost: Option<Decimal>,
    pub currency_code: Currency,
    pub date: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<LunchMoneyTxMetadataCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<LunchMoneyTxMetadataReceipt>,
    pub users: Vec<LunchMoneyTxMetadataExpenseUser>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LunchMoneyTxMetadataCategory {
    pub id: u32,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LunchMoneyTxMetadataReceipt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LunchMoneyTxMetadataExpenseUser {
    pub user_id: u64,
    #[serde(with = "rust_decimal::serde::str")]
    pub net_balance: Decimal,
    #[serde(
        default,
        with = "rust_decimal::serde::str_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub paid_share: Option<Decimal>,
    #[serde(
        default,
        with = "rust_decimal::serde::str_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub owed_share: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<LunchMoneyTxMetadataUserDetails>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LunchMoneyTxMetadataUserDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
}

impl From<crate::api::splitwise::schema::Expense> for LunchMoneyTxMetadataExpense {
    fn from(exp: crate::api::splitwise::schema::Expense) -> Self {
        let crate::api::splitwise::schema::Expense {
            // --- KEPT FIELDS JUSTIFICATION ---

            // The unique Splitwise expense ID is the primary key linking the transactions.
            id,

            // Kept to track which group the expense belongs to.
            group_id,

            // Kept to track 1-on-1 friendship expenses when not part of a group.
            friendship_id,

            // Kept as the primary user-facing title of the transaction.
            description,

            // Kept to preserve any descriptive/notes text attached to the expense.
            details,

            // Kept to distinguish settlement payments from real expenses.
            payment,

            // Kept to show the total cost of the overall bill before splits.
            cost,

            // Kept to record the original currency of the transaction.
            currency_code,

            // Kept to log when the expense actually occurred.
            date,

            // Kept to track when the expense was created in Splitwise.
            created_at,

            // Kept to track when the expense was last modified in Splitwise.
            updated_at,

            // Kept to resolve/map the Splitwise category to a Lunch Money category.
            category,

            // Kept to store receipt image attachment links.
            receipt,

            // Kept to detail the split balances, payments, and names of participants.
            users,

            // --- SKIPPED FIELDS JUSTIFICATION ---

            // Internal Splitwise bundling ID; not generally useful or relevant inside Lunch Money manual transactions.
            expense_bundle_id: _,

            // Scheduling / recurring transaction fields. These are only useful for future recurring transactions in Splitwise
            // and do not represent metadata of a past/realized individual transaction log.
            repeats: _,
            repeat_interval: _,
            email_reminder: _,
            email_reminder_in_advance: _,
            next_repeat: _,

            // Count of comments. Redundant since the actual comments themselves are excluded.
            comments_count: _,

            // Confirmation status of the transaction in Splitwise; not useful for historical ledger accounting.
            transaction_confirmed: _,

            // Redundant repayments details. Individual net shares and debt/balances are already tracked under `users`.
            repayments: _,

            // Full User profiles (including emails, avatars, etc.) of who created/updated/deleted the expense.
            // Excluded because they would bloat the metadata and are not needed as the active participants are in `users`.
            created_by: _,
            updated_by: _,
            deleted_by: _,

            // Deletion info. Deletions are not synchronized to Lunch Money (we skip/delete them, so they never carry metadata).
            deleted_at: _,

            // Redundant with `category` object's inner ID. only used for assertions
            category_id,

            // Extremely verbose comment text and profile details which easily blow past the 4096-byte Lunch Money limit.
            comments: _,
        } = exp;

        if let (Some(cat_id), Some(cat)) = (category_id, category.as_ref()) {
            assert_eq!(
                cat_id, cat.id,
                "Splitwise category_id ({}) does not match category's inner id ({})",
                cat_id, cat.id
            );
        }

        Self {
            id,
            group_id,
            friendship_id,
            description,
            details,
            payment,
            cost,
            currency_code,
            date,
            created_at,
            updated_at,
            category: category.map(|c| {
                let crate::api::splitwise::schema::Category {
                    id,
                    name,
                    // Category icons are skipped because we only need the text category name and ID.
                    icon: _,
                    icon_types: _,
                } = c;
                LunchMoneyTxMetadataCategory { id, name }
            }),
            receipt: receipt.and_then(|r| {
                let crate::api::splitwise::schema::Receipt { large, original } = r;
                if large.is_none() && original.is_none() {
                    None
                } else {
                    Some(LunchMoneyTxMetadataReceipt { large, original })
                }
            }),
            users: users
                .into_iter()
                .map(|u| {
                    let crate::api::splitwise::schema::ExpenseUser {
                        user_id,
                        net_balance,
                        paid_share,
                        owed_share,
                        user,
                    } = u;
                    LunchMoneyTxMetadataExpenseUser {
                        user_id,
                        net_balance,
                        paid_share,
                        owed_share,
                        user: user.map(|ud| {
                            let crate::api::splitwise::schema::UserDetails {
                                id,
                                first_name,
                                last_name,
                                // User picture links are skipped to avoid inflating metadata size.
                                picture: _,
                            } = ud;
                            if let Some(uid) = id {
                                assert_eq!(
                                    user_id, uid,
                                    "Splitwise user_id ({}) does not match nested user id ({})",
                                    user_id, uid
                                );
                            }
                            LunchMoneyTxMetadataUserDetails {
                                id,
                                first_name,
                                last_name,
                            }
                        }),
                    }
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[expect(clippy::large_enum_variant)]
pub enum MaybeLunchMoneyTxMetadata {
    Expected(LunchMoneyTxMetadata),
    Unexpected(serde_json::Value),
}

impl<'de> Deserialize<'de> for MaybeLunchMoneyTxMetadata {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let val = serde_json::Value::deserialize(deserializer)?;
        if let Ok(expected) = serde_json::from_value::<LunchMoneyTxMetadata>(val.clone()) {
            return Ok(Self::Expected(expected));
        }
        Ok(Self::Unexpected(val))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_metadata_roundtrip_and_null_omission() {
        let expense = LunchMoneyTxMetadataExpense {
            id: 12345,
            group_id: Some(999),
            friendship_id: None,
            description: "Test Expense".to_string(),
            details: None,
            payment: false,
            cost: Some(Decimal::new(1500, 2)),
            currency_code: Currency::new("USD"),
            date: Timestamp::from_second(1700000000).unwrap(),
            created_at: None,
            updated_at: Some(Timestamp::from_second(1700000000).unwrap()),
            category: Some(LunchMoneyTxMetadataCategory {
                id: 10,
                name: "Food".to_string(),
            }),
            receipt: None,
            users: vec![LunchMoneyTxMetadataExpenseUser {
                user_id: 111,
                net_balance: Decimal::new(1000, 2),
                paid_share: Some(Decimal::new(1500, 2)),
                owed_share: None,
                user: Some(LunchMoneyTxMetadataUserDetails {
                    id: Some(111),
                    first_name: Some("Alice".to_string()),
                    last_name: None,
                }),
            }],
        };

        let metadata = LunchMoneyTxMetadata::Import {
            delta_transaction_ids: Vec::new(),
            original: expense,
        };

        let serialized = serde_json::to_string(&metadata).unwrap();

        assert!(
            !serialized.contains("null"),
            "Serialized JSON contains null: {}",
            serialized
        );
        assert!(serialized.contains("\"group_id\":999"));
        assert!(!serialized.contains("friendship_id"));
        assert!(!serialized.contains("details"));
        assert!(!serialized.contains("last_name"));

        let deserialized: LunchMoneyTxMetadata = serde_json::from_str(&serialized).unwrap();
        assert_eq!(metadata, deserialized);
    }

    #[test]
    fn test_deserialize_raw_verbose_splitwise_json() {
        let raw_json = r#"{
            "kind": "import",
            "original": {
                "id": 12345,
                "group_id": 999,
                "friendship_id": null,
                "description": "Test Expense",
                "payment": false,
                "cost": "15.00",
                "currency_code": "USD",
                "date": "2023-11-14T22:13:20Z",
                "created_at": "2023-11-14T22:13:20Z",
                "category": {
                    "id": 10,
                    "name": "Food",
                    "icon": "http://example.com/icon.png",
                    "icon_types": {
                        "slim": { "small": "url", "large": "url" }
                    }
                },
                "users": [
                    {
                        "user_id": 111,
                        "net_balance": "10.00",
                        "paid_share": "15.00",
                        "user": {
                            "id": 111,
                            "first_name": "Alice",
                            "picture": { "medium": "url" }
                        }
                    }
                ],
                "comments": [
                    {
                        "id": 1,
                        "content": "A comment",
                        "comment_type": "user"
                    }
                ],
                "repayments": [
                    { "from": 222, "to": 111, "amount": "5.00" }
                ],
                "repeats": false
            }
        }"#;

        let res: Result<LunchMoneyTxMetadata, _> = serde_json::from_str(raw_json);
        assert!(
            res.is_ok(),
            "Failed to deserialize verbose raw splitwise JSON: {:?}",
            res.err()
        );

        let metadata = res.unwrap();
        let LunchMoneyTxMetadata::Import { original, .. } = metadata else {
            panic!("Expected Import variant");
        };
        assert_eq!(original.id, 12345);
        assert_eq!(original.category.unwrap().name, "Food");
        assert_eq!(
            original.users[0]
                .user
                .as_ref()
                .unwrap()
                .first_name
                .as_deref(),
            Some("Alice")
        );
    }
}
