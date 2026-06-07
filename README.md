# `splitwise-lunchmoney`

Sync Splitwise transactions (and global outstanding balances) into Lunch Money manual accounts.

> [!WARNING]
>
> This is 100% free range Gemini Flash 3.5 vibe code.
>
> While the prompter (Daniel Prilik) _has_ been auditing code as it's generated,
> taking care to make sure obvious slop gets refactored and tightened up... you
> may wish to audit the code yourself before using this project.
>
> That said - the prompter _is_ actively using this code with his personal
> splitwise / lunch-money accounts... so it's probably fine™️

---

## ⚡ Key Features

- **Multiple Sync Strategies**:
  - **Rolling Time Window (`sync window`)**: Syncs transactions within a relative timeframe (e.g. `3 days`, `30 days`). Perfect for periodic automation via `cron`!
  - **Group/Individual Sync (`sync group`)**: Syncs transactions for a specific Splitwise group, or individual non-group transactions.
  - **Global Balance Sync (`sync balances`)**: Syncs net outstanding Splitwise balances to Lunch Money manual accounts.
- **Interactive Configuration Wizard (`init`)**: Walks you through setting up credentials, fetches active manual accounts from your Lunch Money profile, auto-maps them based on name patterns (`Splitwise {CURRENCY}`), and generates a boilerplate category map.
- **Multi-Currency Support**: Automatically maps and syncs transactions and balances to their respective manual accounts based on currency.
- **Dry-Run Mode**: Use `--dry-run` on any sync command to preview modifications safely.
- **CSV Reporting**: Supports exporting synchronization operations (inserts, updates, deletes, and balance updates) to CSV files using `--csv`.
- **Data Preservation**: Only modifies a transaction's `amount` and `currency` in Lunch Money. Any local changes to `payee`, `notes`, or `date` in Lunch Money are preserved.


---

## ⚙️ Core Domain & Sync Rules

- **External ID Mapping**: Splitwise transaction IDs are recorded in Lunch Money as `external_id` (prefixed as `splitwise:{id}`).
- **Transaction Diffs & Updates**:
  - Transactions are fetched with `include_group_children=true` and `include_split_parents=true`.
  - Split parent transactions are ignored in diffing and skipped for deletion to preserve manual splits in Lunch Money.
  - Transactions are only updated in Lunch Money if `amount` or `currency` changes. Edits to `payee`, `notes`, or `date` inside Lunch Money are preserved.
- **Double-Entry Liability Rules**:
  - For manual accounts of type `Loan` (liability), transaction amount signs are inverted during sync analysis and API inserts/updates to match Lunch Money's double-entry rules.
  - Manual accounts matching liability types (`Credit`, `Loan`, `OtherLiability`) store outstanding debt as positive numbers in Lunch Money. Negative Splitwise balances are inverted to positive when syncing (e.g. `-100.00 USD` -> `+100.00 USD`). Asset accounts are updated directly without inversion.
- **Payee Name Resolution**:
  - For group transactions, the payee name is set to the Splitwise group's name.
  - For non-group (individual) transactions, the payee is set to the formatted name of the other person involved in the transaction. If that cannot be resolved, it falls back to `"Non-group"`.
- **Ignored Group Balances**: In `sync balances`, balances of ignored groups are automatically subtracted from the user's global outstanding balance to ensure manual accounts reflect only active groups/non-group balances.
- **Unmapped Balance Warnings**: During balance sync, any non-zero Splitwise balances in currencies not mapped to a manual account in the configuration will trigger a warning.

---

## 🔧 Commands & Subcommands

### 1. Configuration Wizard
- **`init`**: Runs the interactive configuration setup.
  ```bash
  cargo run -- init
  ```
  Generates `splitwise-lunchmoney.toml` in the current directory.

### 2. Sync Operations (`sync`)
- **`sync window <WINDOW>`**: Syncs all transactions within a rolling time window (e.g., `3 days`, `24h`, `1 week`, `30 days`).
  - `--from <YYYY-MM-DD>`: Optional date to offset the window from (defaults to today's date).
  - `--dry-run`: Print what would be synced without modifying Lunch Money.
  - `--tag <TAG>`: Apply a custom tag to imported transactions in Lunch Money.
  - `--no-groups`: Only sync individual (non-group) transactions.
  - `--csv <PATH>`: Write the operations to a CSV file.
  - `--no-loan-tag`: Bypass applying the configured `loan_tag` to synced transactions.
  - `--no-ignore`: Bypass check for ignored groups.
  ```bash
  cargo run -- sync window "3 days" --dry-run
  ```

- **`sync group <GROUP>`**: Syncs all transactions associated with a specific Splitwise group.
  - `<GROUP>`: Splitwise group ID or exact group name. Can also be `0` or `"non-group"` to sync non-group transactions.
  - `--dry-run`: Preview changes.
  - `--tag <TAG>`: Apply a custom tag to imported transactions in Lunch Money.
  - `--force-category <CATEGORY>`: Force all transactions to get mapped to this Lunch Money category (ID or name).
  - `--no-ignore`: Bypass check for ignored groups.
  - `--csv [PATH]`: Write operations to a CSV file. If `--csv` is specified without an argument, it defaults to `<group_name>.csv`.
  - `--no-loan-tag`: Bypass applying the configured `loan_tag`.
  ```bash
  cargo run -- sync group "Roommates" --csv --dry-run
  ```

- **`sync balances`**: Syncs global outstanding Splitwise balances to Lunch Money manual accounts.
  - `--dry-run`: Preview balance updates.
  - `--csv [PATH]`: Write balance updates to a CSV file. If `--csv` is specified without an argument, it defaults to `balances.csv`.
  - `--no-loan-tag`: Bypass applying the configured `loan_tag`.
  ```bash
  cargo run -- sync balances --dry-run
  ```

### 3. Queries (`query`)
- **`query splitwise window <WINDOW>`**: Query Splitwise expenses in a time window.
  - `--from <YYYY-MM-DD>`: Offset date.
  - `--no-groups`: Only show non-group transactions.
  ```bash
  cargo run -- query splitwise window "7 days"
  ```
- **`query splitwise group <GROUP>`**: Query Splitwise expenses for a specific group.
  - `<GROUP>`: Splitwise group ID or exact name. Can also be `0` or `"non-group"`.
  ```bash
  cargo run -- query splitwise group "Roommates"
  ```
- **`query splitwise groups`**: List all Splitwise groups you belong to, including IDs, names, and outstanding balances.
  ```bash
  cargo run -- query splitwise groups
  ```
- **`query splitwise categories`**: List all Splitwise transaction categories (parent and subcategories).
  ```bash
  cargo run -- query splitwise categories
  ```
- **`query lunchmoney categories`**: List active categories configured in Lunch Money.
  ```bash
  cargo run -- query lunchmoney categories
  ```
- **`query lunchmoney tags`**: List active tags configured in Lunch Money.
  ```bash
  cargo run -- query lunchmoney tags
  ```
- **`query lunchmoney accounts`**: List active manual accounts configured in Lunch Money.
  ```bash
  cargo run -- query lunchmoney accounts
  ```

---

## ⚙️ Configuration File (`splitwise-lunchmoney.toml`)

Below is an annotated example of `splitwise-lunchmoney.toml`:

```toml
[splitwise]
# Your personal Splitwise API key
api_key = "Zg8TzP..."

# Your Splitwise user ID
user_id = 14417235

# Array of Splitwise group IDs or names to ignore (optional)
ignored_groups = [
    98307552,
    "Roommates"
]

[lunch_money]
# Your Lunch Money developer API key
api_key = "391e53..."

# (Optional) Map currencies to custom manual account IDs in Lunch Money.
# By default, the tool will try to find accounts named "Splitwise {CURRENCY}" (e.g. "Splitwise USD").
# Use this section if you use custom names.
[lunch_money.custom_accounts]
USD = 123456
CAD = 789012

[sync]
# (Optional) Extra tag to associate with transactions representing a loan/liability.
# This makes it easy to spot splitwise transactions to manually group with your credit card transaction in Lunch Money.
loan_tag = "💵 Splitwise"

[categories]
# Map Splitwise category names/IDs to Lunch Money category names/IDs (optional)
"Payment" = "Payment, Transfer"
"Utilities:Electricity" = "Utilities"
"Utilities:Heat/gas" = "Utilities"
"Utilities:TV/Phone/Internet" = "Internet and cable"
"Food and drink:Dining out" = "Restaurants"
"Food and drink:Groceries" = "Groceries"
"Home:Rent" = "Rent"
"Transportation:Taxi" = "Ridesharing"
```

---

## ⏰ Automated Scheduling (Cron)

To keep Lunch Money up-to-date, you can schedule the `sync window` command to run periodically using `cron`.

To avoid unexpected edits to older, posted transactions, only transactions within your selected window will be modified or deleted. Transactions outside that window are considered logically "posted" and will not be retroactively updated (unless explicitly synced via `sync group`).

> NOTE: In the future, it would be good to add extra logic to `sync window` that leverages the splitwise API's `updated_after` API in order to catch backdated transactions / updates to old transactions.
>
> In that case, it could be prudent to add some kind of "soft delete" / "soft update" policy, that doesn't actually modify those old transactions destructively... but somehow signals to the user that they need to be manually looked at (e.g: non-destructively adding tags to those old transactions + importing a "dummy" transaction into lunch money that will alert the user of the backdated modification?)

### Example Crontab Setup

1. Open your user crontab editor:
   ```bash
   crontab -e
   ```

2. Add a cron job to run the sync every day at 3:00 AM with a rolling window of 7 days:
   ```cron
   0 3 * * * cd /path/to/splitwise-lunchmoney && ./target/release/splitwise-lunchmoney sync window "7 days" >> ./sync.log 2>&1
   ```
