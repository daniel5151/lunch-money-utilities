# `splitwise-lunchmoney`

Sync Splitwise transactions into Lunch Money.

> [!WARNING]
>
> This is 100% free range Gemini Flash 3.5 vibe code.
>
> The author is confident enough in this code to use it with his live splitwise / lunch-money accounts... but YMMV.

---

## ⚡ Current Capabilities

- **Auto-Config & Interactive Wizard (`init`)**: Walks you through setting up Splitwise and Lunch Money credentials, fetches manual accounts from your Lunch Money profile, and maps target accounts based on naming patterns (`Splitwise {CURRENCY}`).
- **Multi-Currency Account Mapping**: Syncs transactions to separate Lunch Money manual accounts based on the transaction currency.
- **Deleted Account Guard Rails**: Prior to any sync operation, the tool validates that all configured manual accounts exist in Lunch Money. If an account has been deleted or is missing, sync halts immediately with a clear error and exit code `1`.
- **Inline Descriptions & Details**: Dry runs and logs include the target Lunch Money manual account name/display name, transaction note/description (from Splitwise expense description), currency, and net balance.
- **Subcommands**:
  - `sync window`: Syncs all transactions within a specified time frame (e.g., `3 days`, `1 week`, `30 days`).
  - `sync group`: Syncs all transactions associated with a specific Splitwise Group ID.
  - `query splitwise get-groups`: Queries and prints all Splitwise groups you belong to, including outstanding balances in all active currencies.
  - `query splitwise window` and `query splitwise group`: Fetches and lists raw Splitwise expenses for review.
- **Dry-Run Operations**: Run any sync command with the `--dry-run` flag to preview changes without modifying Lunch Money.

---

## ⚙️ Expected Install & Setup Flow

### 1. Prerequisites
Ensure you have the Rust toolchain installed. If not, install it via [rustup.rs](https://rustup.rs/):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Build the Project
Clone the repository and build the binary:
```bash
git clone https://github.com/your-username/splitwise-lunchmoney.git
cd splitwise-lunchmoney
cargo build --release
```
The compiled binary will be located at `target/release/splitwise-lunchmoney`.

### 3. Initialize Configuration
Before synchronization can run, you must initialize the configuration. Run the interactive setup wizard:
```bash
cargo run -- init
```
This wizard will prompt you for:
1. **Splitwise API Key**: Obtain a personal developer token from Splitwise.
2. **Splitwise User Selection**: Automatically queries the active user and confirms it with you.
3. **Lunch Money API Key**: Generate a developer API key in your Lunch Money settings.
4. **Supported Currencies**: You will be prompted in a loop to enter the 3-letter currency codes you wish to support (e.g., `USD`, `CAD`).
5. **Auto-Account Resolution**: The tool fetches manual accounts from Lunch Money and looks for exact names matching `Splitwise {CURRENCY}` (case-insensitive).
   > [!IMPORTANT]
   > You must set up these manual accounts (e.g. `Splitwise USD`, `Splitwise CAD`) in your Lunch Money profile prior to completing initialization. If any are missing, the wizard will display an action-required error and exit.

Upon successful completion, the configuration is saved to `splitwise-lunchmoney.toml`.

---

## 🚀 Usage

### Previewing Sync (Dry Run)
Always preview changes first using the `--dry-run` flag.

**Sync last 3 days of transactions:**
```bash
cargo run -- sync window --window "3 days" --dry-run
```

**Sync Stockholm 2025 Group expenses:**
```bash
cargo run -- sync group 82559678 --dry-run
```

### Executing Sync
Remove the `--dry-run` flag to push changes to Lunch Money:
```bash
cargo run -- sync window --window "7 days"
```

### Querying Splitwise Groups
List all Splitwise groups and outstanding multi-currency balances:
```bash
cargo run -- query splitwise get-groups
```

---

## ⏰ Automated Scheduling (Cron)

To keep Lunch Money up-to-date, you can schedule the synchronization command to run periodically using `cron`. Since Splitwise transactions can be added retroactively, it is recommended to run a rolling sync window (e.g., daily sync for a 7-day or 14-day window).

### Example Crontab Setup

1. Open your user crontab editor:
   ```bash
   crontab -e
   ```

2. Add a cron job. For example, to run the sync every day at 3:00 AM with a rolling window of 7 days:
   ```cron
   0 3 * * * cd /path/to/splitwise-lunchmoney && ./target/release/splitwise-lunchmoney sync window --window "7 days" >> ./sync.log 2>&1
   ```

   > [!TIP]
   > Make sure the cron job runs from the directory containing your `splitwise-lunchmoney.toml` configuration file, and use the absolute path to the compiled binary.

