# `lm-splitwise-sync`

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
  - **Rolling Time Window (`sync window`)**: Syncs transactions within a relative timeframe (e.g. `3 days`, `30 days`). Perfect for periodic automation via systemd timers!
  - **Group/Individual Sync (`sync group`)**: Syncs transactions for a specific Splitwise group, or individual non-group transactions.
  - **Global Balance Sync (`sync balances`)**: Syncs net outstanding Splitwise balances to Lunch Money manual accounts.
- **Interactive Configuration Wizard (`init`)**: Walks you through setting up credentials, fetches active manual accounts from your Lunch Money profile, auto-maps them based on name patterns (`Splitwise {CURRENCY}`), and generates a boilerplate category map.
- **Multi-Currency Support**: Automatically maps and syncs transactions and balances to their respective manual accounts based on currency.
- **Dry-Run Mode**: Use `--dry-run` on any sync command to preview modifications safely.
- **CSV Reporting**: Supports exporting synchronization operations (inserts, updates, deletes, and balance updates) to CSV files using `--csv`.
- **Data Preservation**: Only modifies a transaction's `amount` and `currency` in Lunch Money. Any local changes to `payee`, `notes`, or `date` in Lunch Money are preserved.


---

## ⚙️ Core Domain & Sync Rules

- **External ID Mapping**: Splitwise transaction IDs are recorded in Lunch Money as `external_id` (prefixed as `splitwise_{id}`).
- **Transaction Diffs & Updates**:
  - Transactions are fetched with `include_group_children=true` and `include_split_parents=true`.
  - Split parent transactions are ignored in diffing and skipped for deletion to preserve manual splits in Lunch Money.
  - Transactions are only updated in Lunch Money if `amount` or `currency` changes. Edits to `payee`, `notes`, or `date` inside Lunch Money are preserved.
- **Manual & CSV Transaction Isolation**:
  - The sync tool completely ignores any transactions in your manual accounts that do not have a `splitwise_` prefix in their `external_id` field.
  - This allows you to manually add transactions or import CSV files (e.g., for offline tracking or cash adjustments) directly in your Lunch Money manual accounts without the sync tool deleting, modifying, or failing validation on them.
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
  Generates `lm_splitwise_sync.toml` in the current directory.

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

## ⚙️ Configuration File (`lm_splitwise_sync.toml`)

Below is an annotated example of `lm_splitwise_sync.toml`:

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

# Tag applied to newly inserted backdated transactions or delta adjustments posted on the current day
backdated_tag = "🧾🕰️ Splitwise Backdated"

# Tag applied to original/older transactions to flag that they have a newer delta adjustment
updated_tag = "🧾⏫ Splitwise Updated"

# Tag applied to orphaned delta transactions when their corresponding original transaction has been deleted
orphaned_tag = "🧾⚠️ Splitwise Orphaned"

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

## ⏰ Automated Scheduling (Systemd Timers)

To keep Lunch Money up-to-date automatically, you can schedule the synchronization tasks using `systemd` user timers. Running these tasks as user services is highly recommended: they require no root (`sudo`) privileges, run under your local user session, and log directly to `journald`.

I suggest setting up two separate timers:
1. **Balance Sync (`sync balances`)**: Runs every hour to update your global outstanding Splitwise balances in Lunch Money.
2. **Window Sync (`sync window`)**: Runs every 15 minutes with a `1 day` rolling window to pull in recent transactions and backdated updates.

Feel free to update the timings as you see fit. e.g: increase the window size if you expect expenses to remain "unstable" for longer than a day.

### Step-by-Step Installation

#### 1. Create the Systemd User Directory
Ensure your systemd user configuration directory exists:
```bash
mkdir -p ~/.config/systemd/user
```

#### 2. Define the Balance Sync Service & Timer (Every 1 Hour)
This task keeps your manual account balances matching Splitwise's outstanding totals.

Create `~/.config/systemd/user/splitwise-sync-balances.service`:
```ini
[Unit]
Description=Sync Splitwise Balances to Lunch Money
After=network.target

[Service]
Type=oneshot
WorkingDirectory=/path/to/lm-splitwise-sync
ExecStart=/path/to/lm-splitwise-sync/target/release/lm-splitwise-sync sync balances
```
> [!NOTE]
> Replace `/path/to/lm-splitwise-sync` with the absolute path to your cloned repository (where `lm_splitwise_sync.toml` and the compiled binary are located). Do not use `~` in unit files as systemd does not perform shell expansion (though systemd specifiers like `%h` can be used to refer to your home directory).

Create `~/.config/systemd/user/splitwise-sync-balances.timer`:
```ini
[Unit]
Description=Run splitwise-sync-balances every hour

[Timer]
OnCalendar=hourly
Persistent=true

[Install]
WantedBy=timers.target
```

#### 3. Define the Window Sync Service & Timer (Every 15 Minutes)
This task syncs transactions occurring or updating in the last day.

Create `~/.config/systemd/user/splitwise-sync-window.service`:
```ini
[Unit]
Description=Sync Splitwise Window to Lunch Money (1 day window)
After=network.target

[Service]
Type=oneshot
WorkingDirectory=/path/to/lm-splitwise-sync
ExecStart=/path/to/lm-splitwise-sync/target/release/lm-splitwise-sync sync window "1 day"
```

Create `~/.config/systemd/user/splitwise-sync-window.timer`:
```ini
[Unit]
Description=Run splitwise-sync-window every 15 minutes

[Timer]
OnCalendar=*:0/15
Persistent=true

[Install]
WantedBy=timers.target
```

#### 4. Load and Enable the Timers
Tell systemd to reload its configuration, then enable and start both timers:
```bash
# Reload the systemd user daemon to register the new units
systemctl --user daemon-reload

# Enable and start the timers immediately
systemctl --user enable --now splitwise-sync-balances.timer
systemctl --user enable --now splitwise-sync-window.timer
```

#### 5. Verify the Installation
You can verify that the timers are active and see when they are next scheduled to run:
```bash
systemctl --user list-timers
```

#### 6. View Logs and Debugging
To view the output/logs of the sync executions, use `journald`'s query tool:
```bash
# View logs for the balance sync service
journalctl --user -u splitwise-sync-balances.service

# View logs for the transaction window sync service
journalctl --user -u splitwise-sync-window.service

# Stream logs in real-time
journalctl --user -u splitwise-sync-window.service -f
```

---

## How Backdated Sync Works

To handle Splitwise expenses that were updated, deleted, or newly added outside the active `sync window`, the tool implements a **non-destructive backdated synchronization workflow**. This ensures older, logically "posted" months in Lunch Money are not modified retroactively, while still correctly reflecting financial adjustments.

1. **Dual Query Fetching**: When syncing, the tool queries Splitwise both for expenses dated within the window and expenses *updated* within the timeframe (using the `updated_after` filter). This ensures backdated changes are captured.
2. **Tag-Based Pre-fetching**: The tool pre-fetches all Lunch Money transactions carrying the `backdated_tag` (and `orphaned_tag`) across the entire history (from `2000-01-01` to today) to resolve existing delta adjustment chains and orphaned states without performing N+1 API calls.
3. **Partitioning**: Expenses are split by transaction date:
   - **In-Window Expenses**: Synced directly (modifications/deletions applied directly to original transactions).
   - **Out-of-Window (Old) Expenses**: Synced non-destructively using the delta engine:
     - **New Backdated Expenses**: Instead of placing them in the past, a new transaction is inserted on the **current day** (tagged with `backdated_tag`), with notes referencing the original date: `(Original Date: YYYY-MM-DD) Description`.
     - **Updates and Deletions (LPP Delta Engine)**:
       - The tool computes the difference between the target Splitwise balance and the current sum of the original Lunch Money transaction and its previously synced deltas.
       - **Within the Logical Posted Period (LPP)**: If the latest delta transaction falls within the active sync window, that latest delta transaction is updated in-place to adjust the total balance.
       - **Outside the LPP**: A **new** delta transaction is posted to the **current day** (tagged with `backdated_tag` and notes `(Original Transaction: YYYY-MM-DD) Description`, where the date is the posted date of the original transaction in Lunch Money).
       - The original transaction's metadata is updated to link to the new delta transaction, and if `updated_tag` is defined, the original transaction is tagged with `updated_tag` to indicate it has been updated.
     - **Currency Changes**: Handled as a deletion (using the LPP delta engine to zero out the old currency manual account transaction) and a new backdated insertion in the new currency manual account on the current day.
4. **Delta Chain Resilience & Self-Healing**:
   - **Resilient API Error Mapping**: If a transaction in the delta chain was deleted on Lunch Money by the user, querying it via `fetch_transaction_by_id` returns a `404 Not Found` response. The HTTP client intercepts this expected error and returns `None` rather than failing the execution.
   - **Self-Healing References**: When a deleted delta transaction is detected, its ID is pruned from the in-memory delta list. The delta engine automatically recalculates the sync delta to restore the correct target balance and propagates metadata updates (the `delta_transaction_ids` list) to all active transactions in the chain (both the `Import` transaction and all active `Delta` transactions) so that every transaction in the chain contains the exact same list of `delta_transaction_ids`.
   - **Orphaned Delta Tagging & Balancing**: If the original `Import` transaction itself is deleted, the remaining `Delta` transactions are considered "orphaned". The sync tool tags these orphaned deltas with `orphaned_tag` and posts a new current-dated balancing transaction (`kind: "orphan"`) that offsets the total sum of the orphaned deltas. The balancing transaction notes are set to: `"Offsetting orphaned deltas for deleted transaction:<original_id>, splitwise_id:<splitwise_id>"`.
