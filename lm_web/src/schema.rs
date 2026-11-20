use std::collections::BTreeSet;
use std::collections::HashMap;

use serde::Serialize;

/// Introspected schema for a `clap::Command` and its tree of subcommands and arguments.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSchema {
    pub name: String,
    pub about: Option<String>,
    pub subcommands: Vec<CommandSchema>,
    pub args: Vec<ArgSchema>,
}

/// Introspected schema for a `clap::Arg`.
#[derive(Debug, Clone, Serialize)]
pub struct ArgSchema {
    pub id: String,
    pub name: String,
    pub help: Option<String>,
    pub long: Option<String>,
    pub short: Option<char>,
    pub is_positional: bool,
    pub is_required: bool,
    pub is_flag: bool,
    pub is_global: bool,
    pub takes_value: bool,
    pub multiple: bool,
    pub action: String,
    pub num_args_min: Option<usize>,
    pub num_args_max: Option<usize>,
    pub value_name: Option<String>,
    pub conflicts_with: Vec<String>,
    pub default_values: Vec<String>,
    pub possible_values: Vec<String>,
    pub value_hint: Option<String>,
}

/// Introspect a `clap::Command` recursively into a [`CommandSchema`].
pub fn introspect_command(cmd: &clap::Command) -> CommandSchema {
    let mut built = cmd.clone();
    built.build();
    introspect_command_built(&built)
}

fn introspect_command_built(cmd: &clap::Command) -> CommandSchema {
    let name = cmd.get_name().to_string();
    let about = cmd.get_about().map(|s| s.to_string());

    let subcommands = cmd
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help" && !sub.is_hide_set())
        .map(introspect_command_built)
        .collect();

    let visible_args: Vec<&clap::Arg> = cmd
        .get_arguments()
        .filter(|arg| {
            let id = arg.get_id().as_str();
            id != "help" && id != "version" && !arg.is_hide_set()
        })
        .collect();

    // Build symmetric conflict map so both sides know about mutual exclusion.
    let mut conflict_map: HashMap<String, BTreeSet<String>> = HashMap::new();
    for arg in &visible_args {
        let id = arg.get_id().as_str().to_string();
        for conflict in cmd.get_arg_conflicts_with(arg) {
            let cid = conflict.get_id().as_str().to_string();
            if cid != id {
                conflict_map
                    .entry(id.clone())
                    .or_default()
                    .insert(cid.clone());
                conflict_map.entry(cid).or_default().insert(id.clone());
            }
        }
    }

    let args = visible_args
        .into_iter()
        .map(|arg| {
            let id = arg.get_id().as_str();
            let conflicts = conflict_map
                .get(id)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default();
            introspect_arg(arg, conflicts)
        })
        .collect();

    CommandSchema {
        name,
        about,
        subcommands,
        args,
    }
}

fn introspect_arg(arg: &clap::Arg, conflicts_with: Vec<String>) -> ArgSchema {
    let id = arg.get_id().as_str().to_string();
    let name = id.clone();
    let help = arg.get_help().map(|s| s.to_string());
    let long = arg.get_long().map(|s| s.to_string());
    let short = arg.get_short();
    let is_positional = arg.is_positional();
    let is_required = arg.is_required_set();
    let is_global = arg.is_global_set();

    let action = arg.get_action();
    let is_flag = matches!(
        action,
        clap::ArgAction::SetTrue | clap::ArgAction::SetFalse | clap::ArgAction::Count
    );

    let action_str = match action {
        clap::ArgAction::Set => "set",
        clap::ArgAction::Append => "append",
        clap::ArgAction::SetTrue => "set_true",
        clap::ArgAction::SetFalse => "set_false",
        clap::ArgAction::Count => "count",
        _ => "other",
    }
    .to_string();

    let takes_value = !is_flag;
    let num_args = arg.get_num_args();
    let num_args_min = num_args.map(|r| r.min_values());
    let num_args_max = num_args.and_then(|r| {
        let max = r.max_values();
        if max == usize::MAX { None } else { Some(max) }
    });
    let multiple = matches!(action, clap::ArgAction::Append | clap::ArgAction::Count)
        || num_args.is_some_and(|r| r.max_values() > 1);

    let value_name = arg
        .get_value_names()
        .and_then(|names| names.first())
        .map(|s| s.to_string());

    let default_values = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();

    let possible_values = if is_flag {
        Vec::new()
    } else {
        arg.get_possible_values()
            .iter()
            .map(|pv| pv.get_name().to_string())
            .collect()
    };

    let value_hint = match arg.get_value_hint() {
        clap::ValueHint::FilePath => Some("filepath".to_string()),
        clap::ValueHint::DirPath => Some("dirpath".to_string()),
        clap::ValueHint::AnyPath => Some("path".to_string()),
        clap::ValueHint::Url => Some("url".to_string()),
        clap::ValueHint::EmailAddress => Some("email".to_string()),
        clap::ValueHint::Other => None,
        _ => None,
    };

    ArgSchema {
        id,
        name,
        help,
        long,
        short,
        is_positional,
        is_required,
        is_flag,
        is_global,
        takes_value,
        multiple,
        action: action_str,
        num_args_min,
        num_args_max,
        value_name,
        conflicts_with,
        default_values,
        possible_values,
        value_hint,
    }
}

#[cfg(test)]
mod tests {
    use clap::Arg;
    use clap::ArgAction;
    use clap::Command;
    use clap::ValueHint;

    use super::*;

    #[test]
    fn test_introspect_command_tree() {
        let cmd = Command::new("test-app")
            .about("A test CLI application")
            .arg(
                Arg::new("verbose")
                    .long("verbose")
                    .short('v')
                    .action(ArgAction::SetTrue)
                    .help("Increase verbosity"),
            )
            .arg(
                Arg::new("config")
                    .long("config")
                    .value_hint(ValueHint::FilePath)
                    .default_value("config.toml")
                    .help("Path to configuration file"),
            )
            .arg(
                Arg::new("format")
                    .long("format")
                    .value_parser(["json", "csv", "table"])
                    .help("Output format"),
            )
            .subcommand(
                Command::new("sync")
                    .about("Sync subcommand")
                    .arg(
                        Arg::new("api-url")
                            .long("api-url")
                            .global(true)
                            .help("Global on intermediate subcommand"),
                    )
                    .subcommand(
                        Command::new("run")
                            .arg(
                                Arg::new("dry-run")
                                    .long("dry-run")
                                    .action(ArgAction::SetTrue),
                            )
                            .arg(
                                Arg::new("files")
                                    .required(true)
                                    .num_args(1..)
                                    .help("Input files"),
                            )
                            .arg(Arg::new("page").long("page").action(ArgAction::Append))
                            .arg(
                                Arg::new("from-page")
                                    .long("from-page")
                                    .conflicts_with("page"),
                            )
                            .arg(Arg::new("csv").long("csv").num_args(0..=1)),
                    ),
            );

        let schema = introspect_command(&cmd);

        assert_eq!(schema.name, "test-app");
        assert_eq!(schema.about.as_deref(), Some("A test CLI application"));
        assert_eq!(schema.subcommands.len(), 1);

        let sync_sub = &schema.subcommands[0];
        assert_eq!(sync_sub.name, "sync");
        assert_eq!(sync_sub.subcommands.len(), 1);

        let run_sub = &sync_sub.subcommands[0];
        assert_eq!(run_sub.name, "run");

        // Intermediate global arg propagated to leaf
        let api_url = run_sub.args.iter().find(|a| a.id == "api-url").unwrap();
        assert!(api_url.is_global);

        // Multi-arg positional
        let files = run_sub.args.iter().find(|a| a.id == "files").unwrap();
        assert!(files.is_positional);
        assert!(files.is_required);
        assert!(files.multiple);
        assert_eq!(files.num_args_min, Some(1));
        assert_eq!(files.num_args_max, None);

        // Symmetric conflicts
        let page = run_sub.args.iter().find(|a| a.id == "page").unwrap();
        let from_page = run_sub.args.iter().find(|a| a.id == "from-page").unwrap();
        assert!(page.multiple);
        assert_eq!(page.conflicts_with, vec!["from-page".to_string()]);
        assert_eq!(from_page.conflicts_with, vec!["page".to_string()]);

        // Optional-value option (num_args 0..=1)
        let csv = run_sub.args.iter().find(|a| a.id == "csv").unwrap();
        assert_eq!(csv.num_args_min, Some(0));
        assert_eq!(csv.num_args_max, Some(1));
        assert!(!csv.multiple);

        // Root args check
        let verbose_arg = schema.args.iter().find(|a| a.name == "verbose").unwrap();
        assert!(verbose_arg.is_flag);
        assert_eq!(verbose_arg.action, "set_true");
        assert_eq!(verbose_arg.short, Some('v'));

        let config_arg = schema.args.iter().find(|a| a.name == "config").unwrap();
        assert!(!config_arg.is_flag);
        assert_eq!(config_arg.default_values, vec!["config.toml".to_string()]);
        assert_eq!(config_arg.value_hint.as_deref(), Some("filepath"));

        let format_arg = schema.args.iter().find(|a| a.name == "format").unwrap();
        assert_eq!(format_arg.possible_values, vec!["json", "csv", "table"]);
    }
}
