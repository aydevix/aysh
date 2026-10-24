use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Pipe,
    Input,
    Output,
    Append,
}

#[derive(Debug, Default)]
struct Stage {
    args: Vec<String>,
    input: Option<String>,
    output: Option<(String, bool)>,
}

enum BuiltinResult {
    Continue(i32, String),
    Exit,
}

#[derive(Debug, Clone, Copy, Default)]
struct TextStyle {
    bold: bool,
    italic: bool,
    color: Option<TextColor>,
}

#[derive(Debug, Clone, Copy)]
enum TextColor {
    Black,
    Red,
    Cyan,
}

#[derive(Debug, Default)]
struct ShellConfig {
    prompt: String,
    autostart: Vec<String>,
    style: TextStyle,
}

fn main() {
    let interactive = io::stdin().is_terminal();
    let config = load_config();
    let mut last_status = 0;
    let stdin = io::stdin();
    let mut line = String::new();

    for command in &config.autostart {
        if process_line(command, &mut last_status, config.style) {
            return;
        }
    }

    loop {
        if interactive {
            print_styled(&config.prompt, config.style, io::stdout().is_terminal());
            let _ = io::stdout().flush();
        }

        line.clear();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("aysh: input error: {error}");
                last_status = 1;
                continue;
            }
        }

        if process_line(&line, &mut last_status, config.style) {
            break;
        }
    }
}

/// Execute one shell input line. Returns true if the shell should exit.
fn process_line(line: &str, last_status: &mut i32, style: TextStyle) -> bool {
    let tokens = match tokenize(line, *last_status) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("aysh: {error}");
            *last_status = 2;
            return false;
        }
    };
    if tokens.is_empty() {
        return false;
    }

    let stages = match parse(tokens) {
        Ok(stages) => stages,
        Err(error) => {
            eprintln!("aysh: {error}");
            *last_status = 2;
            return false;
        }
    };

    if stages.len() == 1 && is_builtin(&stages[0].args) {
        match run_builtin(&stages[0].args, style) {
            BuiltinResult::Exit => true,
            BuiltinResult::Continue(status, output) => {
                let mut command_status = status;
                if !output.is_empty() {
                    if let Some((path, append)) = &stages[0].output {
                        match open_output(path, *append)
                            .and_then(|mut file| file.write_all(output.as_bytes()))
                        {
                            Ok(()) => {}
                            Err(error) => {
                                eprintln!("aysh: {path}: {error}");
                                command_status = 1;
                            }
                        }
                    } else {
                        let output_style = if stages[0].args[0] == "echo" {
                            echo_style(&stages[0].args, style)
                                .map(|(style, _)| style)
                                .unwrap_or(style)
                        } else {
                            style
                        };
                        print_styled(&output, output_style, io::stdout().is_terminal());
                        let _ = io::stdout().flush();
                    }
                }
                *last_status = command_status;
                false
            }
        }
    } else {
        *last_status = run_pipeline(&stages);
        false
    }
}

fn config_path() -> Option<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(config_home);
        if path.is_absolute() {
            return Some(path.join("aysh/config.toml"));
        }
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/aysh/config.toml"))
}

fn load_config() -> ShellConfig {
    let Some(path) = config_path() else {
        return ShellConfig {
            prompt: "# ".into(),
            ..ShellConfig::default()
        };
    };
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ShellConfig {
                prompt: "# ".into(),
                ..ShellConfig::default()
            }
        }
        Err(error) => {
            eprintln!("aysh: cannot read {}: {error}", path.display());
            return ShellConfig {
                prompt: "# ".into(),
                ..ShellConfig::default()
            };
        }
    };

    match parse_config(&contents) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("aysh: invalid config {}: {error}", path.display());
            ShellConfig {
                prompt: "# ".into(),
                ..ShellConfig::default()
            }
        }
    }
}

fn parse_config(contents: &str) -> Result<ShellConfig, String> {
    let config: toml::Table = toml::from_str(contents).map_err(|error| error.to_string())?;
    let prompt = match config.get("prompt") {
        None => "# ".to_owned(),
        Some(toml::Value::String(prompt))
            if !prompt.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0')) =>
        {
            prompt.clone()
        }
        Some(_) => return Err("key `prompt` must be a single-line string".into()),
    };

    let autostart = match config.get("autostart") {
        None => Vec::new(),
        Some(toml::Value::String(command)) => vec![command.clone()],
        Some(toml::Value::Array(commands)) => commands
            .iter()
            .map(|command| match command {
                toml::Value::String(command) => Ok(command.clone()),
                _ => Err("all entries in `autostart` must be strings".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("key `autostart` must be a string or an array of strings".into()),
    };

    let style = match config.get("style") {
        None => TextStyle::default(),
        Some(toml::Value::Table(style)) => {
            let bold = read_bool(style, "bold")?.unwrap_or(false);
            let italic = read_bool(style, "italic")?.unwrap_or(false);
            let color = match style.get("color") {
                None => None,
                Some(toml::Value::String(color)) => Some(parse_color(color)?),
                Some(_) => return Err("`style.color` must be black, red, or cyan".into()),
            };
            TextStyle {
                bold,
                italic,
                color,
            }
        }
        Some(_) => return Err("key `style` must be a TOML table".into()),
    };

    Ok(ShellConfig {
        prompt,
        autostart,
        style,
    })
}

fn read_bool(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
) -> Result<Option<bool>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Boolean(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("`style.{key}` must be true or false")),
    }
}

fn parse_color(color: &str) -> Result<TextColor, String> {
    match color {
        "black" => Ok(TextColor::Black),
        "red" => Ok(TextColor::Red),
        "cyan" | "blue" | "голубой" => Ok(TextColor::Cyan),
        _ => Err("color must be black, red, or cyan (голубой)".into()),
    }
}

fn print_styled(text: &str, style: TextStyle, terminal: bool) {
    if !terminal || (!style.bold && !style.italic && style.color.is_none()) {
        print!("{text}");
        return;
    }

    let mut codes = Vec::new();
    if style.bold {
        codes.push("1");
    }
    if style.italic {
        codes.push("3");
    }
    match style.color {
        Some(TextColor::Black) => codes.push("30"),
        Some(TextColor::Red) => codes.push("31"),
        Some(TextColor::Cyan) => codes.push("36"),
        None => {}
    }
    print!("\x1b[{}m{text}\x1b[0m", codes.join(";"));
}

fn tokenize(line: &str, last_status: i32) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];
        if let Some(q) = quote {
            if ch == q {
                quote = None;
                started = true;
                i += 1;
            } else if ch == '\\' && q == '"' {
                if i + 1 < chars.len() {
                    word.push(chars[i + 1]);
                    i += 2;
                } else {
                    i += 1;
                }
            } else if ch == '$' && q == '"' {
                let (value, next) = expand_variable(&chars, i, last_status);
                word.push_str(&value);
                started = true;
                i = next;
            } else {
                word.push(ch);
                started = true;
                i += 1;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                started = true;
                i += 1;
            }
            '\\' => {
                if i + 1 < chars.len() {
                    word.push(chars[i + 1]);
                    started = true;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            '$' => {
                let (value, next) = expand_variable(&chars, i, last_status);
                word.push_str(&value);
                started = true;
                i = next;
            }
            ' ' | '\t' | '\r' | '\n' => {
                push_word(&mut tokens, &mut word, &mut started);
                i += 1;
            }
            '|' | '<' | '>' => {
                push_word(&mut tokens, &mut word, &mut started);
                if ch == '|' {
                    tokens.push(Token::Pipe);
                    i += 1;
                } else if ch == '<' {
                    tokens.push(Token::Input);
                    i += 1;
                } else if i + 1 < chars.len() && chars[i + 1] == '>' {
                    tokens.push(Token::Append);
                    i += 2;
                } else {
                    tokens.push(Token::Output);
                    i += 1;
                }
            }
            '#' if !started => break,
            _ => {
                word.push(ch);
                started = true;
                i += 1;
            }
        }
    }

    if quote.is_some() {
        return Err("unclosed quote".into());
    }
    push_word(&mut tokens, &mut word, &mut started);
    Ok(tokens)
}

fn push_word(tokens: &mut Vec<Token>, word: &mut String, started: &mut bool) {
    if *started {
        tokens.push(Token::Word(std::mem::take(word)));
        *started = false;
    }
}

fn expand_variable(chars: &[char], start: usize, last_status: i32) -> (String, usize) {
    let mut i = start + 1;
    if i >= chars.len() {
        return ("$".into(), i);
    }
    if chars[i] == '?' {
        return (last_status.to_string(), i + 1);
    }

    let braced = chars[i] == '{';
    if braced {
        i += 1;
    }
    let name_start = i;
    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
        i += 1;
    }
    if name_start == i {
        return ("$".into(), start + 1);
    }
    let name: String = chars[name_start..i].iter().collect();
    if braced && i < chars.len() && chars[i] == '}' {
        i += 1;
    }
    (env::var(&name).unwrap_or_default(), i)
}

fn parse(tokens: Vec<Token>) -> Result<Vec<Stage>, String> {
    let mut stages = vec![Stage::default()];
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Word(word) => {
                stages.last_mut().unwrap().args.push(word.clone());
                i += 1;
            }
            Token::Pipe => {
                if stages.last().unwrap().args.is_empty() {
                    return Err("empty command before pipe".into());
                }
                stages.push(Stage::default());
                i += 1;
            }
            Token::Input | Token::Output | Token::Append => {
                let op = tokens[i].clone();
                let path = match tokens.get(i + 1) {
                    Some(Token::Word(path)) => path.clone(),
                    _ => return Err("expected a file name after redirection".into()),
                };
                let stage = stages.last_mut().unwrap();
                match op {
                    Token::Input if stage.input.is_none() => stage.input = Some(path),
                    Token::Output if stage.output.is_none() => stage.output = Some((path, false)),
                    Token::Append if stage.output.is_none() => stage.output = Some((path, true)),
                    Token::Input => return Err("duplicate input redirection".into()),
                    _ => return Err("duplicate output redirection".into()),
                }
                i += 2;
            }
        }
    }
    if stages.last().unwrap().args.is_empty() {
        return Err("expected a command after pipe".into());
    }
    Ok(stages)
}

fn is_builtin(args: &[String]) -> bool {
    args.first().is_some_and(|name| {
        matches!(
            name.as_str(),
            "cd" | "pwd" | "help" | "exit" | "export" | "unset" | "echo"
        )
    })
}

fn run_builtin(args: &[String], default_style: TextStyle) -> BuiltinResult {
    let name = args.first().map(String::as_str).unwrap_or("");
    match name {
        "exit" => BuiltinResult::Exit,
        "pwd" => match env::current_dir() {
            Ok(path) => BuiltinResult::Continue(0, format!("{}\n", path.display())),
            Err(error) => builtin_error(error),
        },
        "help" => BuiltinResult::Continue(
            0,
            "aysh — a tiny command shell\n\
             Built-ins: cd [DIR], pwd, echo [STYLE] TEXT, export NAME=VALUE, unset NAME, help, exit\n\
             Syntax: commands, quotes, $VARIABLE, $?, pipes (|), <, >, >>\n"
                .into(),
        ),
        "echo" => {
            let (_, start) = match echo_style(args, default_style) {
                Ok(options) => options,
                Err(error) => return builtin_message(2, &format!("aysh: echo: {error}\n")),
            };
            let output = format!("{}\n", args[start..].join(" "));
            BuiltinResult::Continue(0, output)
        }
        "cd" => {
            let target = match args.get(1).map(String::as_str) {
                Some("-") => match env::var("OLDPWD") {
                    Ok(path) => path,
                    Err(_) => return builtin_message(1, "aysh: OLDPWD is not set\n"),
                },
                Some(path) => path.to_owned(),
                None => match env::var("HOME") {
                    Ok(path) => path,
                    Err(_) => return builtin_message(1, "aysh: HOME is not set\n"),
                },
            };
            let old = env::current_dir().ok();
            match env::set_current_dir(&target) {
                Ok(()) => {
                    if let Some(old) = old {
                        env::set_var("OLDPWD", old);
                    }
                    BuiltinResult::Continue(0, String::new())
                }
                Err(error) => builtin_message(1, &format!("aysh: cd: {target}: {error}\n")),
            }
        }
        "export" => {
            if args.len() == 1 {
                let mut vars: Vec<_> = env::vars().collect();
                vars.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                let output = vars
                    .into_iter()
                    .map(|(k, v)| format!("{k}={v}\n"))
                    .collect();
                return BuiltinResult::Continue(0, output);
            }
            for assignment in &args[1..] {
                let (key, value) = assignment
                    .split_once('=')
                    .map(|(k, v)| (k, v))
                    .unwrap_or((assignment, ""));
                if !valid_env_name(key) {
                    return builtin_message(1, &format!("aysh: export: invalid name: {key}\n"));
                }
                env::set_var(key, value);
            }
            BuiltinResult::Continue(0, String::new())
        }
        "unset" => {
            for key in &args[1..] {
                if !valid_env_name(key) {
                    return builtin_message(1, &format!("aysh: unset: invalid name: {key}\n"));
                }
                env::remove_var(key);
            }
            BuiltinResult::Continue(0, String::new())
        }
        _ => BuiltinResult::Continue(1, String::new()),
    }
}

fn echo_style(args: &[String], default: TextStyle) -> Result<(TextStyle, usize), String> {
    let mut style = default;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bold" => style.bold = true,
            "--italic" => style.italic = true,
            "--color" => {
                i += 1;
                let color = args.get(i).ok_or("--color requires black, red, or cyan")?;
                style.color = Some(parse_color(color)?);
            }
            option if option.starts_with("--color=") => {
                style.color = Some(parse_color(&option[8..])?);
            }
            "--" => return Ok((style, i + 1)),
            option if option.starts_with('-') => {
                return Err(format!("unknown option `{option}`"));
            }
            _ => return Ok((style, i)),
        }
        i += 1;
    }
    Ok((style, i))
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn builtin_error(error: io::Error) -> BuiltinResult {
    builtin_message(1, &format!("aysh: {error}\n"))
}

fn builtin_message(status: i32, message: &str) -> BuiltinResult {
    BuiltinResult::Continue(status, message.to_owned())
}

fn open_output(path: &str, append: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(path)
}

fn run_pipeline(stages: &[Stage]) -> i32 {
    if stages.len() == 1 && stages[0].args.is_empty() {
        return 0;
    }

    let mut children: Vec<Child> = Vec::with_capacity(stages.len());
    let mut previous_stdout = None;

    for (index, stage) in stages.iter().enumerate() {
        if stage.args.is_empty() {
            eprintln!("aysh: empty command");
            return 2;
        }
        if is_builtin(&stage.args) {
            eprintln!("aysh: built-in commands cannot be used in a pipeline");
            return 2;
        }

        let mut command = Command::new(&stage.args[0]);
        command.args(&stage.args[1..]);

        if let Some(path) = &stage.input {
            drop(previous_stdout.take());
            match File::open(path) {
                Ok(file) => {
                    command.stdin(Stdio::from(file));
                }
                Err(error) => {
                    eprintln!("aysh: {path}: {error}");
                    return 1;
                }
            }
        } else if let Some(stdout) = previous_stdout.take() {
            command.stdin(Stdio::from(stdout));
        } else if index > 0 {
            // A preceding stage may have redirected its output instead of
            // writing into the pipeline. Give this stage EOF, not the shell's
            // own stdin (which could otherwise make it hang interactively).
            command.stdin(Stdio::null());
        }

        if let Some((path, append)) = &stage.output {
            match open_output(path, *append) {
                Ok(file) => {
                    command.stdout(Stdio::from(file));
                }
                Err(error) => {
                    eprintln!("aysh: {path}: {error}");
                    return 1;
                }
            }
        } else if index + 1 < stages.len() {
            command.stdout(Stdio::piped());
        }

        match command.spawn() {
            Ok(mut child) => {
                previous_stdout = child.stdout.take();
                children.push(child);
            }
            Err(error) => {
                eprintln!("aysh: {}: {error}", stage.args[0]);
                for mut child in children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return 127;
            }
        }
    }

    let mut status = 0;
    for mut child in children {
        match child.wait() {
            Ok(exit) => status = exit.code().unwrap_or(128),
            Err(error) => {
                eprintln!("aysh: wait failed: {error}");
                status = 1;
            }
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::{
        echo_style, parse, parse_color, parse_config, tokenize, TextColor, TextStyle, Token,
    };

    #[test]
    fn tokenizes_quotes_variables_and_operators() {
        let tokens = tokenize("echo 'hello world' \"$HOME\"|cat >>out", 7).unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Word("echo".into()),
                Token::Word("hello world".into()),
                Token::Word(std::env::var("HOME").unwrap_or_default()),
                Token::Pipe,
                Token::Word("cat".into()),
                Token::Append,
                Token::Word("out".into()),
            ]
        );
    }

    #[test]
    fn parses_pipeline_and_redirections() {
        let stages = parse(tokenize("cat <in | grep x >>out", 0).unwrap()).unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].args, ["cat"]);
        assert_eq!(stages[0].input.as_deref(), Some("in"));
        assert_eq!(stages[1].args, ["grep", "x"]);
        assert_eq!(stages[1].output, Some(("out".into(), true)));
    }

    #[test]
    fn rejects_unclosed_quotes_and_trailing_pipes() {
        assert!(tokenize("echo 'oops", 0).is_err());
        assert!(parse(vec![Token::Word("echo".into()), Token::Pipe]).is_err());
    }

    #[test]
    fn empty_quotes_make_an_empty_argument() {
        let tokens = tokenize("echo ''", 0).unwrap();
        assert_eq!(
            tokens,
            vec![Token::Word("echo".into()), Token::Word(String::new())]
        );
        let stages = parse(tokens).unwrap();
        assert_eq!(stages[0].args, ["echo", ""]);
    }

    #[test]
    fn toml_prompt_defaults_and_customizes() {
        assert_eq!(parse_config("").unwrap().prompt, "# ");
        assert_eq!(parse_config("prompt = 'aysh> '").unwrap().prompt, "aysh> ");
    }

    #[test]
    fn toml_prompt_rejects_invalid_values() {
        assert!(parse_config("prompt = 42").is_err());
        assert!(parse_config(r#"prompt = "first\nsecond""#).is_err());
        assert!(parse_config("prompt = [").is_err());
    }

    #[test]
    fn toml_autostart_accepts_a_command_or_command_list() {
        assert_eq!(
            parse_config("autostart = 'echo hello'").unwrap().autostart,
            ["echo hello"]
        );
        assert_eq!(
            parse_config("autostart = ['pwd', 'echo ready']")
                .unwrap()
                .autostart,
            ["pwd", "echo ready"]
        );
        assert!(parse_config("autostart = [1]").is_err());
    }

    #[test]
    fn toml_style_and_echo_options_support_requested_formats() {
        let style = parse_config("[style]\nbold = true\nitalic = true\ncolor = 'cyan'\n")
            .unwrap()
            .style;
        assert!(style.bold && style.italic);
        assert!(matches!(style.color, Some(TextColor::Cyan)));
        assert!(matches!(parse_color("black"), Ok(TextColor::Black)));
        assert!(matches!(parse_color("red"), Ok(TextColor::Red)));
        assert!(matches!(parse_color("голубой"), Ok(TextColor::Cyan)));

        let (style, text_start) = echo_style(
            &[
                "echo".into(),
                "--bold".into(),
                "--color=red".into(),
                "hello".into(),
            ],
            TextStyle::default(),
        )
        .unwrap();
        assert!(style.bold);
        assert!(matches!(style.color, Some(TextColor::Red)));
        assert_eq!(text_start, 3);
    }
}
