pub const STYLE_HEADER: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlue)));

pub const STYLE_SUCCESS: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Green)));

pub const STYLE_ERROR: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Red)));

pub const STYLE_WARNING: anstyle::Style = anstyle::Style::new()
    .effects(anstyle::Effects::BOLD)
    .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow)));

pub const STYLE_INFO: anstyle::Style =
    anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Cyan)));

pub const STYLE_DIM: anstyle::Style =
    anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::BrightBlack)));
