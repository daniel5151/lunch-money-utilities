# Prilik's Lunch Money Utilities

Assorted [Lunch Money](https://lunchmoney.app/) utilities.

Some are pretty generic (e.g., `lm-splitwise-sync`), others are a bit more
specialized (e.g., `lm-payslip-importer`), but given that they all share a common
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

## Tools

| Crate                                                  | Description                                                                                                                      |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------- |
| [`lm-backup`](lm_backup/README.md)                     | Full backup of all Lunch Money data to raw JSON files.                                                                           |
| [`lm-query`](lm_query/README.md)                       | Quick read-only queries (categories, tags, accounts).                                                                            |
| [`lm-splitwise-sync`](lm_splitwise_sync/README.md)     | Sync Splitwise transactions and outstanding balances into (per-currency) Lunch Money accounts.                                   |
| [`lm-payslip-importer`](lm_payslip_importer/README.md) | Break down payslip events (direct deposits, RSU vests) into granular transactions (taxes, imputed income, reimbursements, etc.). |
| [`lm-venmo-plaidfix`](lm_venmo_plaidfix/README.md)     | Fixup various issues caused by Venmo's suboptimal Plaid integration.                                                             |

The user-facing entry point to all tools is the **`lm-utils` binary**, which can be run in one of two modes:

### As a subcommand of `lm-utils`

```console
$ lm-utils backup
$ lm-utils query categories
$ lm-utils splitwise-sync sync window --window "3 days"
$ lm-utils payslip-importer import ./payslip.pdf
$ lm-utils venmo-plaidfix reconcile 30d
```

### Subcommand aliases via `argv[0]`

Like [busybox](https://busybox.net/), `lm-utils` also dispatches based on the
name it was invoked under. Symlink (or hardlink) it to `lm-<tool>` (e.g.
`lm-splitwise-sync`) and it behaves as that tool:

```console
$ ln -s lm-utils lm-splitwise-sync
$ ./lm-splitwise-sync sync window --window "3 days"   # == lm-utils splitwise-sync ...
```

Recognized argv[0] names must be prefixed with `lm-` followed by the tool subcommand names:
`lm-backup`, `lm-query`, `lm-splitwise-sync`, `lm-payslip-importer`, and `lm-venmo-plaidfix`.

## Support Crates

| Crate                         | Description                                         |
| ----------------------------- | --------------------------------------------------- |
| [`lm-utils`](lm_utils/)       | Bin: entrypoint bin, dispatching to every tool      |
| [`lm-common`](lm_common/)     | Lib: shared infrastructure used by all tools        |
| [`lunch_money`](lunch_money/) | Lib: type-safe client around the Lunch Money API v2 |

## Configuration: `lm_utils.toml`

Utilities are configured via `lm_utils.toml`.

```toml
# Shared settings for every Lunch Money utility tool.
[common]
# The single shared Lunch Money developer API key.
lm_api_key = "..."

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

Running any tool's `init` will generate a new file, **or extend an existing file**:

```console
$ lm-utils splitwise-sync init   # writes/updates [common] + [splitwise]
$ lm-utils venmo-plaidfix init   # adds [venmo] to the same file, in place
```

Each `init` **upserts only its own section** into an existing `lm_utils.toml`,
leaving every other tool's section intact.

## License

Everything is MIT. See [LICENSE](LICENSE).

Happy Hacking!
