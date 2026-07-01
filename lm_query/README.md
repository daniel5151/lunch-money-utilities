# `lm-query`

Quick read-only queries against your Lunch Money account.

> Eventually, it would be nice to flesh this out into a comprehensive CLI
> wrapper around the Lunch Money API v2... but at the moment, it's just an
> ad-hoc dumping ground for queries, added on an as-needed basis.

## Usage

```console
$ lm-utils query categories    # list all categories (nested)
$ lm-utils query tags          # list all tags
$ lm-utils query accounts      # list manual accounts
```

Output is a formatted table printed to stdout.

## Configuration

Only needs `[common].lm_api_key` in `lm_utils.toml` — no tool-specific config section.
