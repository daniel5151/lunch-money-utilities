use crate::core::CategoryId;
use serde::Deserialize;

/// Response payload containing a list of categories.
#[derive(Deserialize, Debug)]
pub struct CategoriesResponse {
    /// List of category objects.
    pub categories: Vec<Category>,
}

/// A Lunch Money category.
#[derive(Deserialize, Clone, Debug)]
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

/// A category that is a child of a category group.
#[derive(Deserialize, Clone, Debug)]
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
