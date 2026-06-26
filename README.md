# Prilik's Lunch Money Utilities

Assorted [Lunch Money](https://lunchmoney.app/) utilities.

Some are pretty generic (e.g: `lm-splitwise-sync`), others are a bit more
specialized (e.g: `lm-payslip-importer`), but given that they all share a common
foundation... it makes sense to keep 'em all under one roof.

> [!WARNING]
>
> This repo is nearly 100% free range Gemini Flash 3.5 / Opus 4.8 vibe code.
>
> While The Prompter (Daniel Prilik) _has_ been auditing code as it's generated,
> and trying his darndest to make sure obvious slop gets refactored and
> tightened up... you may wish to audit the code in this repo yourself before
> deploying any of these projects.
>
> That said, The Prompter _is_ actively using this code with his personal
> Lunch Money account... so hey, it's Probably Fine™️

## Crates

Everything now ships as a **single `lm-utils` binary** — a
[busybox](https://www.busybox.net/)-style multiplexer that bundles all three
tools. The tool libraries still live in their own crates (so they stay
independently testable), but only `lm-utils` produces a binary.

| Crate                                         | Path                   | Description                                                                                                                                                                                                           |
| --------------------------------------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`lm-utils`](lm_utils/)                       | `lm_utils/`            | Bin: the single multiplexer binary that dispatches to every tool below.                                                                                                                                              |
| [`lunch_money`](lunch_money/)                 | `lunch_money/`         | Lib: Client for interacting with the Lunch Money v2 API                                                                                                                                                               |
| [`lm_common`](lm_common/)                     | `lm_common/`           | Lib: shared infrastructure — the configurable Lunch Money client, the `lm_utils.toml` config loader/editor, the `Tool` trait + dispatch, shared `init` prompts, terminal styling, and CLI help colors.               |
| [`lm-splitwise-sync`](lm_splitwise_sync/)     | `lm_splitwise_sync/`   | Lib: syncs Splitwise transactions and outstanding balances into Lunch Money manual accounts. See its [README](lm_splitwise_sync/README.md).                                                                           |
| [`lm-payslip-importer`](lm_payslip_importer/) | `lm_payslip_importer/` | Lib: Imports direct deposits and RSU vests from payslips (primarily tested with Workday, but designed to be generic) into granular transaction lines in Lunch Money. See its [README](lm_payslip_importer/README.md). |
| [`lm-venmo-balfixer`](lm_venmo_balfixer/)     | `lm_venmo_balfixer/`   | Lib: Fixup Plaid-synced Venmo accounts to follow proper double-entry accounting principles. See its [README](lm_venmo_balfixer/README.md).                                                                            |

Expect more utilities to appear over time...

## Usage

Invoke any tool as a subcommand of `lm-utils`:

```console
$ lm-utils splitwise-sync sync window --window "3 days"
$ lm-utils payslip-importer import ./payslip.pdf
$ lm-utils venmo-balfixer reconcile 30d
```

### Subcommand aliases via `argv[0]`

Like busybox, `lm-utils` also dispatches based on the name it was invoked under.
Symlink (or hardlink) it to a recognized tool name and it behaves as that tool:

```console
$ ln -s lm-utils splitwise-sync
$ ./splitwise-sync sync window --window "3 days"   # == lm-utils splitwise-sync ...
```

Recognized argv[0] names are the tool subcommand names themselves:
`splitwise-sync`, `payslip-importer`, and `venmo-balfixer`.

`--dry-run` is a shared flag understood by every tool: it previews changes
without writing anything back to Lunch Money.

## Configuration: `lm_utils.toml`

All three tools read a **single, shared `lm_utils.toml`** in the working
directory, rather than a separate `lm_<tool>.toml` per tool. It holds one shared
`[common]` table plus one section per tool:

```toml
# Shared settings for every Lunch Money utility tool.
[common]
# The single shared Lunch Money developer API key.
lm_api_key = "..."
# retry = { max_attempts = 5, initial_delay = "2s" }   # optional 429 backoff

[splitwise]
api_key = "..."
user_id = 123
# ...
[splitwise.sync]
# ...

[payslip]
tag = "payslip"
[payslip.backends.workday]
# ...

[venmo]
venmo_acct = "..."
bank_acct = "..."
```

The Lunch Money API key lives **once** under `[common].lm_api_key`; it is no
longer duplicated into a per-tool `[lunch_money]` table.

Run any tool's `init` to generate or extend the file:

```console
$ lm-utils splitwise-sync init   # writes/updates [common] + [splitwise]
$ lm-utils venmo-balfixer init   # adds [venmo] to the same file, in place
```

Each `init` **upserts only its own section** (and the shared key) into an
existing `lm_utils.toml`, leaving every other tool's section — and all of the
inline comment pointers the wizards author — intact. Running all three `init`
wizards in turn yields one fully-populated, well-commented config file.

## License

Everything is MIT. See [LICENSE](LICENSE).

Have fun!
