# GEMINI.md - Context & System Prompt Initializer

## Project Overview
`splitwise-lunchmoney` is a Rust CLI application designed to synchronize Splitwise transactions and global outstanding balances into Lunch Money manual accounts. It maps currency codes to specific manual accounts, tracks imported items to prevent duplicate syncs, and facilitates automated runs via `cron`.

---

## Project Structure
- **`src/main.rs`**: Entry point that parses CLI arguments, loads configuration, and dispatches subcommands.
- **`src/cli.rs`**: CLI command argument parser structures and subcommand definitions (using `clap`).
- **`src/config.rs`**: Structure and deserialization definitions for `splitwise-lunchmoney.toml`.
- **`src/style.rs`**: Terminal style configurations and `STYLE_*` color constants.
- **`src/api/`**: Client endpoints and data schema implementations for external API interactions:
  - [`lunch_money.rs`](file:///home/daprilik/src/splitwise-lunchmoney/src/api/lunch_money.rs): Developer API client and object models (Transactions, Categories, Tags).
  - [`splitwise.rs`](file:///home/daprilik/src/splitwise-lunchmoney/src/api/splitwise.rs): Splitwise API client and object models (Expenses, Groups).
- **`src/commands/`**: Command runners executing business operations:
  - [`init.rs`](file:///home/daprilik/src/splitwise-lunchmoney/src/commands/init.rs): Interactive config setup wizard.
  - [`query.rs`](file:///home/daprilik/src/splitwise-lunchmoney/src/commands/query.rs): Runners for querying and listing data (expenses, groups, categories, tags).
  - [`sync.rs`](file:///home/daprilik/src/splitwise-lunchmoney/src/commands/sync.rs): Synchronizer for individual transaction logs.
  - [`sync_balances.rs`](file:///home/daprilik/src/splitwise-lunchmoney/src/commands/sync_balances.rs): Runner for syncing net outstanding balances.

---

## Tech Stack & Core Dependencies
- **HTTP Client**: `reqwest` (asynchronous, utilizes a shared connection pool).
- **CLI Framework**: `clap` (v4 with `derive` macros).
- **Date/Time Handling**: `jiff` (used exclusively; `chrono` is avoided).
- **Decimals**: `rust_decimal` (exact arithmetic for currency amounts; serializes to strings for Lunch Money payloads).
- **Terminal Styling**: `anstream` and `anstyle` for warning/success color-coded output.
- **De/Serialization**: `serde` / `serde_json` / `toml`.

---

## Domain & Business Logic Rules
- **External ID Mapping**: Splitwise transaction IDs are recorded in Lunch Money as `external_id` (prefixed like `splitwise_{id}`).
- **Transaction Diffs & Updates**:
  - Transactions are fetched with `include_group_children=true` and `include_split_parents=true`.
  - Split parent transactions are ignored in diffing and skipped for deletion to preserve manual splits in Lunch Money.
  - Transactions are only updated in Lunch Money if `amount` or `currency` changes. Edits to `payee`, `notes`, or `date` in Lunch Money are preserved.
- **Manual & CSV Transaction Isolation**: Transactions in manual accounts lacking a `splitwise_` external ID prefix (e.g. manually added, split child transactions without external ID, or CSV-imported transactions) MUST be completely ignored during validation, metadata migration, diff planning, and sync deletions. This ensures custom user modifications/additions inside these manual accounts do not cause regressions or sync errors.
- **Group Matching & Resolution**: Splitwise groups can be resolved by ID or exact name (case-sensitive). A fallback synthetic non-group ID of `0` is resolved if querying `0` or `"non-group"` exactly. Configured ignored groups also match by ID or exact name (case-sensitive).
- **Group Exclusion**: Transactions associated with a group can be excluded by specifying `--no-groups` on `query expenses` and `sync window` commands.
- **CSV Reporting**: Synchronization commands (`sync window`, `sync group`, `sync balances`) support dumping their operations (inserts, updates, deletes) to a CSV file via the `--csv` option.
- **Forced Categories**: The `sync group` command supports a `--force-category` flag to override the default category mappings and map all synchronized transactions to a specific active Lunch Money category (by ID or exact name).
- **Loan Tag Exclusion**: Synchronization commands (`sync window`, `sync group`, `sync balances`) support a `--no-loan-tag` flag to bypass applying the configured `loan_tag` to synchronized transactions (this operates as a no-op if no loan tag is configured or if the command does not sync individual transactions).
- **Transaction Sign Inversion**: In manual accounts of type `Loan` (liability), transaction amount signs are inverted during sync analysis and API inserts/updates to match Lunch Money's double-entry rules.
- **Global Balance Sync**:
  - Net outstanding balances are computed by querying `/get_friends` and summing the `balance` array per currency across all friends.
  - Manual account balances are updated via `PUT /manual_accounts/{id}`.
  - **Liability Account Balance Inversion**: Manual accounts matching liability types (`Credit`, `Loan`, `OtherLiability`) store their outstanding debt as positive numbers in Lunch Money. Therefore, the sign of negative Splitwise balances is inverted to positive when writing to these accounts (e.g. `-100.00 USD` -> `+100.00 USD`). Asset types are updated directly without inversion.
  - **Unmapped warning**: The tool flags and prints non-zero Splitwise balances in currencies not mapped to manual accounts in the configuration.
- **Backdated & Out-of-Window Transaction Sync**:
  - **Dual Query Fetching**: During `sync window`, the tool queries Splitwise both for expenses created/dated within the window, and expenses updated within the window (using `updated_after`), merging them to capture backdated updates.
  - **Tag-Based Pre-fetching**: The tool pre-fetches all Lunch Money transactions tagged with the user-configured `backdated_tag` across the entire history (from `2000-01-01` to prevent N+1 API query limits or dry-run update discrepancies).
  - **Fallback Targeted Date Queries**: If an old transaction is not found in standard or pre-fetched sets, the tool runs a targeted single-day query for that transaction's original date in Lunch Money.
  - **Decision Table (Old Expenses)**:
    - *New backdated inserts*: Posted to the current day, tagged with `backdated_tag`, with notes `(Original Date: YYYY-MM-DD) Description`.
    - *Updates / Deletes (LPP Delta Engine)*:
      - If the latest delta transaction date falls within the sync window (LPP), that latest delta transaction is updated in-place.
      - If no delta exists or the latest delta is older than the sync window, a new delta transaction is inserted on the current day, tagged with `backdated_tag` and notes `(Original Transaction: <original_id>) Description`.
      - When a delta is inserted/updated and the user-configured `updated_tag` is defined, the original transaction is tagged with `updated_tag` and its notes are updated with a pointer to the next delta (e.g. `(See Transaction: <delta_id>)`).
    - *Currency Changes*: Treated as a deletion in the old currency/account (delta engine bringing balance to `0.00`) and a new backdated insertion on the current day in the new currency/account.

---

## Styling & Coding Conventions
- **Macro Delimiter Rules**: All single-line `println!` and `eprintln!` statements must utilize curly brace delimiters (`{}`) and end with a semicolon (e.g. `println! { "message" };`). This prevents `rustfmt` from splitting them across multiple lines
- **anstream macros**: Make sure to use anstream's `println!` and `eprintln!` macros, rather than the std ones.
- **Standard Formatting Output**: Leverage `STYLE_*` constants (`STYLE_HEADER`, `STYLE_SUCCESS`, `STYLE_ERROR`, `STYLE_WARNING`, `STYLE_INFO`, `STYLE_DIM`) for uniform CLI logging.
- **rustfmt nightly**: Use `+nightly` when running rustfmt. make sure to run rustfmt after making a set of changes.
- **Commit format**: run `git log` to check how previous commits were formatted, and use the same style when asked to commit changes. NEVER COMMIT UNLESS EXPLICITLY ASKED TO COMMIT.
