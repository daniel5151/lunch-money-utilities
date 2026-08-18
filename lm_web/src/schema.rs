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
    pub default_values: Vec<String>,
    pub possible_values: Vec<String>,
    pub value_hint: Option<String>,
}

/// Introspect a `clap::Command` recursively into a [`CommandSchema`].
pub fn introspect_command(cmd: &clap::Command) -> CommandSchema {
    let name = cmd.get_name().to_string();
    let about = cmd.get_about().map(|s| s.to_string());

    let subcommands = cmd
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help" && !sub.is_hide_set())
        .map(introspect_command)
        .collect();

    let args = cmd
        .get_arguments()
        .filter(|arg| {
            let id = arg.get_id().as_str();
            id != "help" && id != "version" && !arg.is_hide_set()
        })
        .map(introspect_arg)
        .collect();

    CommandSchema {
        name,
        about,
        subcommands,
        args,
    }
}

fn introspect_arg(arg: &clap::Arg) -> ArgSchema {
    let id = arg.get_id().as_str().to_string();
    let name = arg.get_id().as_str().to_string();
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

    let takes_value = !is_flag;
    let multiple = matches!(action, clap::ArgAction::Append | clap::ArgAction::Count);

    let default_values = arg
        .get_default_values()
        .iter()
        .map(|v| v.to_string_lossy().into_owned())
        .collect();

    let possible_values = arg
        .get_possible_values()
        .iter()
        .map(|pv| pv.get_name().to_string())
        .collect();

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
                        Arg::new("dry-run")
                            .long("dry-run")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(Arg::new("target").required(true).help("Target name")),
            );

        let schema = introspect_command(&cmd);

        assert_eq!(schema.name, "test-app");
        assert_eq!(schema.about.as_deref(), Some("A test CLI application"));
        assert_eq!(schema.subcommands.len(), 1);

        // Subcommand check
        let sub = &schema.subcommands[0];
        assert_eq!(sub.name, "sync");
        assert_eq!(sub.about.as_deref(), Some("Sync subcommand"));
        assert_eq!(sub.args.len(), 2);

        let dry_run_arg = sub.args.iter().find(|a| a.name == "dry-run").unwrap();
        assert!(dry_run_arg.is_flag);
        assert!(!dry_run_arg.takes_value);

        let target_arg = sub.args.iter().find(|a| a.name == "target").unwrap();
        assert!(target_arg.is_positional);
        assert!(target_arg.is_required);

        // Root args check
        let verbose_arg = schema.args.iter().find(|a| a.name == "verbose").unwrap();
        assert!(verbose_arg.is_flag);
        assert_eq!(verbose_arg.short, Some('v'));

        let config_arg = schema.args.iter().find(|a| a.name == "config").unwrap();
        assert!(!config_arg.is_flag);
        assert_eq!(config_arg.default_values, vec!["config.toml".to_string()]);
        assert_eq!(config_arg.value_hint.as_deref(), Some("filepath"));

        let format_arg = schema.args.iter().find(|a| a.name == "format").unwrap();
        assert_eq!(format_arg.possible_values, vec!["json", "csv", "table"]);
    }
}
