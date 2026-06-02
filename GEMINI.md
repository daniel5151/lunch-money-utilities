# GEMINI.md - Context & System Prompt Initializer

## Project Overview
`splitwise-lunchmoney` is a Rust CLI application designed to synchronize Splitwise transactions and global outstanding balances into Lunch Money manual accounts. It maps currency codes to specific manual accounts, tracks imported items to prevent duplicate syncs, and facilitates automated runs via `cron`.

---

## Architecture & Module Structure
The current layout is flat, with core logic centered in `main.rs`:
- [src/main.rs](file:///home/daprilik/src/splitwise-lunchmoney/src/main.rs): Entrypoint, Clap CLI definitions, config parsing, sync/query orchestration, and styling definitions.
- [src/config.rs](file:///home/daprilik/src/splitwise-lunchmoney/src/config.rs): Strongly-typed configurations mapping to `splitwise-lunchmoney.toml`.
- [src/api/lunch_money.rs](file:///home/daprilik/src/splitwise-lunchmoney/src/api/lunch_money.rs): Lunch Money API client, payload definitions, and schemas (`TransactionStatus`, `AccountType` enums).
- [src/api/splitwise.rs](file:///home/daprilik/src/splitwise-lunchmoney/src/api/splitwise.rs): Splitwise API client and schemas (`FriendsResponse`, `Friend` definitions).

### Planned Refactor
- Extract CLI argument parsers and subcommands into `src/cli.rs`.
- Decouple execution handlers out of `main.rs` into a separate `src/commands/` module (e.g., `src/commands/sync_window.rs`, `src/commands/sync_balances.rs`, `src/commands/query_categories.rs`).

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

---

## Current Status & Next Steps
- **Last Completed**: Implemented the `sync balances` subcommand (including `--dry-run`), added sign inversion handling for manual liability accounts, and reformatted all single-line print macros to use the curly brace syntax.
- **Immediate Next Task**: Perform the planned refactor, extracting [src/main.rs](file:///home/daprilik/src/splitwise-lunchmoney/src/main.rs)'s clap parser and subcommand dispatch logic into `src/cli.rs` and modularized command files inside `src/commands/`.
