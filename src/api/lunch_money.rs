#![allow(unused_imports, dead_code)]

use crate::api::ExternalId;
use crate::metadata::LunchMoneyTxMetadata;
use crate::metadata::MaybeLunchMoneyTxMetadata;

pub use lunch_money::Client;
pub use lunch_money::TransactionQuery;

pub mod schema {
    use super::*;

    pub type Transaction = lunch_money::schema::Transaction<MaybeLunchMoneyTxMetadata, ExternalId>;
    pub type InsertObject = lunch_money::schema::InsertObject<LunchMoneyTxMetadata, ExternalId>;
    pub type UpdateObject = lunch_money::schema::UpdateObject<LunchMoneyTxMetadata, ExternalId>;
    pub type InsertTransactionsResponse =
        lunch_money::schema::InsertTransactionsResponse<MaybeLunchMoneyTxMetadata, ExternalId>;

    pub use crate::metadata::LunchMoneyTxMetadata;
    pub use crate::metadata::MaybeLunchMoneyTxMetadata;

    pub use lunch_money::schema::AccountStatus;
    pub use lunch_money::schema::AccountType;
    pub use lunch_money::schema::Category;
    pub use lunch_money::schema::ChildCategory;
    pub use lunch_money::schema::ManualAccount;
    pub use lunch_money::schema::SkippedExistingExternalIdObject;
    pub use lunch_money::schema::Tag;
    pub use lunch_money::schema::TransactionStatus;
    pub use lunch_money::schema::UpdateManualAccountObject;
}
