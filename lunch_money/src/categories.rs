//! Category schema models and responses.

/// JSON schemas for categories.
pub mod schemas {
    use crate::core::CategoryId;
    use serde::Deserialize;
    use serde::Serialize;

    /// Response payload containing a list of categories.
    #[derive(Deserialize, Debug)]
    pub struct CategoriesResponse {
        /// List of category objects.
        pub categories: Vec<Category>,
    }

    /// A Lunch Money category.
    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub struct Category {
        /// System-defined unique ID for the category.
        pub id: CategoryId,
        /// Name of the category.
        pub name: String,
        /// The description of the category or `null` if not set.
        pub description: Option<String>,
        /// If true, the transactions in this category will be treated as income.
        pub is_income: bool,
        /// If true, the transactions in this category will be excluded from the budget.
        pub exclude_from_budget: bool,
        /// If true, the transactions in this category will be excluded from totals.
        pub exclude_from_totals: bool,
        /// The date and time of when the category was last updated.
        pub updated_at: jiff::Timestamp,
        /// The date and time of when the category was created.
        pub created_at: jiff::Timestamp,
        /// ID of the parent category group, if applicable.
        pub group_id: Option<CategoryId>,
        /// Whether this category is a group containing other categories.
        pub is_group: bool,
        /// Optional list of children categories (only populated for groups).
        pub children: Option<Vec<ChildCategory>>,
        /// Whether this category is archived.
        pub archived: bool,
        /// The date and time of when the category was last archived.
        pub archived_at: Option<jiff::Timestamp>,
        /// An integer specifying the position in which the category is displayed.
        pub order: Option<u32>,
        /// If true, the category is collapsed in the Lunch Money GUI.
        pub collapsed: bool,
    }

    /// Input type representing a child category when creating/updating a category group.
    #[derive(Serialize, Deserialize, Clone, Debug)]
    #[serde(untagged)]
    pub enum CategoryChildInput {
        /// ID of an existing category.
        Id(CategoryId),
        /// Name of a new child category to be created.
        Name(String),
        /// Full Category object.
        Category(Category),
    }

    /// Request payload for creating a new category or category group.
    #[derive(Serialize, Clone, Debug)]
    pub struct CreateCategoryPayload {
        /// Name of the category (1-100 characters).
        pub name: String,
        /// Description of the category (up to 200 characters).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        /// If true, transactions in this category will be treated as income.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_income: Option<bool>,
        /// If true, transactions in this category will be excluded from the budget.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub exclude_from_budget: Option<bool>,
        /// If true, transactions in this category will be excluded from totals.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub exclude_from_totals: Option<bool>,
        /// If true, the category is created as a category group.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_group: Option<bool>,
        /// ID of an existing category group to assign this category to.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub group_id: Option<CategoryId>,
        /// If true, the category is archived.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub archived: Option<bool>,
        /// List of children to add to the new category group (only if is_group is true).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub children: Option<Vec<CategoryChildInput>>,
        /// Position index in the GUI.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub order: Option<u32>,
        /// If true, the category group is collapsed in the GUI.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub collapsed: Option<bool>,
    }

    /// Request payload for updating an existing category or category group.
    #[derive(Serialize, Clone, Debug, Default)]
    pub struct UpdateCategoryPayload {
        /// New name of the category (1-100 characters).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        /// New description of the category (up to 200 characters).
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub description: Option<Option<String>>,
        /// If true, transactions in this category will be treated as income.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_income: Option<bool>,
        /// If true, transactions in this category will be excluded from the budget.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub exclude_from_budget: Option<bool>,
        /// If true, transactions in this category will be excluded from totals.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub exclude_from_totals: Option<bool>,
        /// If true, the category is archived.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub archived: Option<bool>,
        /// ID of an existing category group to assign this category to.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub group_id: Option<Option<CategoryId>>,
        /// is_group cannot be changed but can be passed for validation.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub is_group: Option<bool>,
        /// List of children categories for category groups.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub children: Option<Vec<CategoryChildInput>>,
        /// Position index in the GUI.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub order: Option<Option<u32>>,
        /// If true, the category group is collapsed in the GUI.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub collapsed: Option<Option<bool>>,
        /// If set, updates the archived timestamp.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[expect(clippy::option_option)]
        pub archived_at: Option<Option<jiff::Timestamp>>,
    }

    /// Response returned when a category deletion fails due to existing dependencies.
    #[derive(Deserialize, Clone, Debug)]
    pub struct DeleteCategoryDependenciesResponse {
        /// Name of the category that was attempted to be deleted.
        pub category_name: String,
        /// Detailed count of dependents blocking deletion.
        pub dependents: CategoryDependents,
    }

    /// Counts of dependent objects preventing a category from being deleted.
    #[derive(Deserialize, Clone, Debug)]
    pub struct CategoryDependents {
        /// Number of budgets depending on the category.
        pub budget: u32,
        /// Number of category rules depending on the category.
        pub category_rules: u32,
        /// Number of transactions depending on the category.
        pub transactions: u32,
        /// Number of child categories in the category group.
        pub children: u32,
        /// Number of recurring transactions depending on the category.
        pub recurring: u32,
        /// Number of auto created categories based on Plaid categories.
        pub plaid_cats: u32,
    }

    /// Response returned by the delete category endpoint.
    #[derive(Debug, Clone)]
    pub enum DeleteCategoryResult {
        /// The category was deleted successfully.
        Deleted,
        /// The category could not be deleted because it has dependencies.
        Dependencies(DeleteCategoryDependenciesResponse),
    }

    /// A category that is a child of a category group.
    #[derive(Deserialize, Serialize, Clone, Debug)]
    pub struct ChildCategory {
        /// Unique identifier for the category.
        pub id: CategoryId,
        /// Name of the category.
        pub name: String,
        /// The description of the category or `null` if not set.
        pub description: Option<String>,
        /// If true, the transactions in this category will be treated as income.
        pub is_income: bool,
        /// If true, the transactions in this category will be excluded from the budget.
        pub exclude_from_budget: bool,
        /// If true, the transactions in this category will be excluded from totals.
        pub exclude_from_totals: bool,
        /// The date and time of when the category was last updated.
        pub updated_at: jiff::Timestamp,
        /// The date and time of when the category was created.
        pub created_at: jiff::Timestamp,
        /// ID of the parent category group.
        pub group_id: Option<CategoryId>,
        /// Whether this category is archived.
        pub archived: bool,
        /// The date and time of when the category was last archived.
        pub archived_at: Option<jiff::Timestamp>,
        /// An index specifying the position in which the category is displayed.
        pub order: Option<u32>,
        /// If true, the category is collapsed in the Lunch Money GUI.
        pub collapsed: Option<bool>,
    }
}
