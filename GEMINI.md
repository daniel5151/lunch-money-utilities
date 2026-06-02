# GEMINI.md - Context & System Prompt Initializer

## Project Overview
`splitwise-lunchmoney` is a Rust CLI application designed to synchronize Splitwise transactions and global outstanding balances into Lunch Money manual accounts. It maps currency codes to specific manual accounts, tracks imported items to prevent duplicate syncs, and facilitates automated runs via `cron`.

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
- **External ID Mapping**: Splitwise transaction IDs are recorded in Lunch Money as `external_id` (prefixed like `splitwise:{id}`).
- **Transaction Diffs & Updates**:
  - Transactions are fetched with `include_group_children=true` and `include_split_parents=true`.
  - Split parent transactions are ignored in diffing and skipped for deletion to preserve manual splits in Lunch Money.
  - Transactions are only updated in Lunch Money if `amount` or `currency` changes. Edits to `payee`, `notes`, or `date` in Lunch Money are preserved.
- **Transaction Sign Inversion**: In manual accounts of type `Loan` (liability), transaction amount signs are inverted during sync analysis and API inserts/updates to match Lunch Money's double-entry rules.
- **Global Balance Sync**:
  - Net outstanding balances are computed by querying `/get_friends` and summing the `balance` array per currency across all friends.
  - Manual account balances are updated via `PUT /manual_accounts/{id}`.
  - **Liability Account Balance Inversion**: Manual accounts matching liability types (`Credit`, `Loan`, `OtherLiability`) store their outstanding debt as positive numbers in Lunch Money. Therefore, the sign of negative Splitwise balances is inverted to positive when writing to these accounts (e.g. `-100.00 USD` -> `+100.00 USD`). Asset types are updated directly without inversion.
  - **Unmapped warning**: The tool flags and prints non-zero Splitwise balances in currencies not mapped to manual accounts in the configuration.

---

## Styling & Coding Conventions
- **Macro Delimiter Rules**: All single-line `println!` and `eprintln!` statements must utilize curly brace delimiters (`{}`) and end with a semicolon (e.g. `println! { "message" };`). This prevents `rustfmt` from splitting them across multiple lines (under the `ignore_macros = ["println", "eprintln"]` rule in `rustfmt.toml`).
- **Standard Formatting Output**: Leverage `STYLE_*` constants (`STYLE_HEADER`, `STYLE_SUCCESS`, `STYLE_ERROR`, `STYLE_WARNING`, `STYLE_INFO`, `STYLE_DIM`) for uniform CLI logging.
