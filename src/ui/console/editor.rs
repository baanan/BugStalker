use crate::ui::command::parser::{
    ARG_ALL_KEY, ARG_COMMAND, ARG_DEBUG_COMMAND, ASYNC_COMMAND, ASYNC_COMMAND_BACKTRACE_SUBCOMMAND,
    ASYNC_COMMAND_BACKTRACE_SUBCOMMAND_SHORT, ASYNC_COMMAND_STEP_OUT_SUBCOMMAND,
    ASYNC_COMMAND_STEP_OUT_SUBCOMMAND_SHORT, ASYNC_COMMAND_STEP_OVER_SUBCOMMAND,
    ASYNC_COMMAND_STEP_OVER_SUBCOMMAND_SHORT, ASYNC_COMMAND_TASK_SUBCOMMAND,
    BACKTRACE_ALL_SUBCOMMAND, BACKTRACE_COMMAND, BACKTRACE_COMMAND_SHORT, BREAK_COMMAND,
    BREAK_COMMAND_SHORT, CALL_COMMAND, CONTINUE_COMMAND, CONTINUE_COMMAND_SHORT, FRAME_COMMAND,
    FRAME_COMMAND_INFO_SUBCOMMAND, FRAME_COMMAND_SWITCH_SUBCOMMAND, HELP_COMMAND,
    HELP_COMMAND_SHORT, MEMORY_COMMAND, MEMORY_COMMAND_READ_SUBCOMMAND, MEMORY_COMMAND_SHORT,
    MEMORY_COMMAND_WRITE_SUBCOMMAND, ORACLE_COMMAND, REGISTER_COMMAND,
    REGISTER_COMMAND_INFO_SUBCOMMAND, REGISTER_COMMAND_READ_SUBCOMMAND, REGISTER_COMMAND_SHORT,
    REGISTER_COMMAND_WRITE_SUBCOMMAND, RUN_COMMAND, RUN_COMMAND_SHORT, SHARED_LIB_COMMAND,
    SHARED_LIB_COMMAND_INFO_SUBCOMMAND, SOURCE_COMMAND, SOURCE_COMMAND_DISASM_SUBCOMMAND,
    SOURCE_COMMAND_FUNCTION_SUBCOMMAND, STEP_INSTRUCTION_COMMAND, STEP_INTO_COMMAND,
    STEP_INTO_COMMAND_SHORT, STEP_OUT_COMMAND, STEP_OUT_COMMAND_SHORT, STEP_OVER_COMMAND,
    STEP_OVER_COMMAND_SHORT, SYMBOL_COMMAND, THREAD_COMMAND, THREAD_COMMAND_CURRENT_SUBCOMMAND,
    THREAD_COMMAND_INFO_SUBCOMMAND, THREAD_COMMAND_SWITCH_SUBCOMMAND, TRIGGER_COMMAND,
    TRIGGER_COMMAND_ANY_TRIGGER_SUBCOMMAND, TRIGGER_COMMAND_BRKPT_TRIGGER_SUBCOMMAND,
    TRIGGER_COMMAND_INFO_SUBCOMMAND, TRIGGER_COMMAND_WP_TRIGGER_SUBCOMMAND, VAR_COMMAND,
    VAR_DEBUG_COMMAND, VAR_LOCAL_KEY, WATCH_COMMAND, WATCH_COMMAND_SHORT, WATCH_INFO_SUBCOMMAND,
    WATCH_REMOVE_SUBCOMMAND, WATCH_REMOVE_SUBCOMMAND_SHORT,
};
use chumsky::prelude::{any, choice, just};
use chumsky::text::whitespace;
use chumsky::{Parser, extra, text};
use crossterm::style::{Color, Stylize};
use log::warn;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::HistoryHinter;
use rustyline::history::{FileHistory, MemHistory};
use rustyline::line_buffer::LineBuffer;
use rustyline::{Changeset, CompletionType, Config, Context, Editor, ExternalPrinter};
use rustyline_derive::{Helper, Hinter, Validator};
use std::borrow::Cow;
use std::borrow::Cow::{Borrowed, Owned};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use trie_rs::{Trie, TrieBuilder};

struct CommandHint {
    short: Option<String>,
    long: String,
    subcommands: Vec<String>,
}

impl CommandHint {
    fn long(&self) -> String {
        self.long.clone()
    }

    fn display_with_short(&self) -> String {
        if let Some(ref short) = self.short {
            if self.long.starts_with(short) {
                format!(
                    "{}{}",
                    short.clone().bold().underlined(),
                    &self.long[short.len()..]
                )
            } else {
                format!("{}|{}", &self.long, short.clone().bold().underlined())
            }
        } else {
            self.long()
        }
    }
}

impl From<&str> for CommandHint {
    fn from(value: &str) -> Self {
        CommandHint {
            short: None,
            long: value.to_string(),
            subcommands: vec![],
        }
    }
}

impl From<(&str, &str)> for CommandHint {
    fn from((short, long): (&str, &str)) -> Self {
        CommandHint {
            short: Some(short.to_string()),
            long: long.to_string(),
            subcommands: vec![],
        }
    }
}

pub struct CommandCompleter {
    commands: Vec<CommandHint>,
    subcommand_hints: HashMap<String, Vec<String>>,
    file_hints: Trie<u8>,
    var_hints: Trie<u8>,
    vars: Vec<String>,
    arg_hints: Trie<u8>,
    args: Vec<String>,
}

impl CommandCompleter {
    fn new(commands: impl IntoIterator<Item = CommandHint>) -> Self {
        let commands: Vec<CommandHint> = commands.into_iter().collect();
        let subcommand_hints = commands
            .iter()
            .flat_map(|cmd| {
                let mut hints = vec![(cmd.long.clone(), cmd.subcommands.clone())];
                if let Some(ref short) = cmd.short {
                    hints.push((short.clone(), cmd.subcommands.clone()));
                }
                hints
            })
            .collect::<HashMap<String, Vec<String>>>();

        Self {
            commands,
            subcommand_hints,
            file_hints: TrieBuilder::new().build(),
            var_hints: TrieBuilder::new().build(),
            arg_hints: TrieBuilder::new().build(),
            args: vec![],
            vars: vec![],
        }
    }

    pub fn replace_file_hints(&mut self, files: impl IntoIterator<Item = PathBuf>) {
        let mut builder = TrieBuilder::new();
        files.into_iter().for_each(|path: PathBuf| {
            let file_name = path
                .file_name()
                .and_then(|oss| oss.to_str())
                .unwrap_or_default()
                .to_owned();
            builder.push(file_name);
        });
        self.file_hints = builder.build();
    }

    pub fn replace_local_var_hints(&mut self, variables: impl IntoIterator<Item = String>) {
        let mut builder = TrieBuilder::new();
        self.vars = variables.into_iter().collect();
        self.vars.iter().for_each(|var| {
            builder.push(var);
        });
        self.vars.push(VAR_LOCAL_KEY.underlined().to_string());
        builder.push(VAR_LOCAL_KEY);
        self.var_hints = builder.build();
    }

    pub fn replace_arg_hints(&mut self, args: impl IntoIterator<Item = String>) {
        let mut builder = TrieBuilder::new();
        self.args = args.into_iter().collect();
        self.args.iter().for_each(|arg| {
            builder.push(arg);
        });
        self.args.push(ARG_ALL_KEY.underlined().to_string());
        builder.push(ARG_ALL_KEY);
        self.arg_hints = builder.build();
    }
}

#[derive(Debug)]
enum CompletableCommand<'a> {
    Breakpoint(&'a str),
    PrintVariables(&'a str),
    PrintArguments(&'a str),
    Unrecognized(&'a str, Option<&'a str>),
}

impl<'a> CompletableCommand<'a> {
    fn recognize(line: &'a str) -> Option<CompletableCommand<'a>> {
        let op = just::<_, _, extra::Default>;

        let bp = op(BREAK_COMMAND)
            .or(op(BREAK_COMMAND_SHORT))
            .then(whitespace().at_least(1))
            .ignore_then(any().repeated().to_slice())
            .map(CompletableCommand::Breakpoint);

        let var = op(VAR_DEBUG_COMMAND)
            .or(op(VAR_COMMAND))
            .then(whitespace().at_least(1))
            .ignore_then(any().repeated().to_slice())
            .map(CompletableCommand::PrintVariables);

        let arg = op(ARG_DEBUG_COMMAND)
            .or(op(ARG_COMMAND))
            .then(whitespace().at_least(1))
            .ignore_then(any().repeated().to_slice())
            .map(CompletableCommand::PrintArguments);

        let other = text::ident()
            .then_ignore(whitespace().at_least(1))
            .then(text::ident().or_not())
            .map(|(s1, s2): (&str, Option<&str>)| CompletableCommand::Unrecognized(s1.trim(), s2))
            .padded();

        let r = choice((bp, var, arg, other)).parse(line);
        r.into_result().ok()
    }
}

impl Completer for CommandCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        fn pairs_from_variants(
            variants: impl Iterator<Item = impl ToString>,
            line: &str,
            tpl: &str,
            replacement_suffix: &str,
        ) -> (usize, Vec<Pair>) {
            let pos = line.len() - tpl.len();
            let pairs = variants.map(|v| Pair {
                display: v.to_string(),
                replacement: v.to_string() + replacement_suffix,
            });
            (pos, pairs.collect())
        }

        match CompletableCommand::recognize(line) {
            Some(CompletableCommand::Breakpoint(maybe_file)) => {
                if maybe_file.trim().is_empty() {
                    return Ok((0, vec![]));
                }

                let variants = self.file_hints.predictive_search(maybe_file);
                if !variants.is_empty() {
                    let variants_iter = variants.iter().map(|var| {
                        std::str::from_utf8(var.as_slice()).expect("invalid utf-8 string")
                    });
                    return Ok(pairs_from_variants(variants_iter, line, maybe_file, ":"));
                }
            }
            Some(CompletableCommand::PrintVariables(maybe_var)) => {
                if maybe_var.trim().is_empty() {
                    return Ok(pairs_from_variants(self.vars.iter(), line, maybe_var, ""));
                }

                let variants = self.var_hints.predictive_search(maybe_var);
                if !variants.is_empty() {
                    let variants_iter = variants.iter().map(|var| {
                        std::str::from_utf8(var.as_slice()).expect("invalid utf-8 string")
                    });
                    return Ok(pairs_from_variants(variants_iter, line, maybe_var, ""));
                }
            }
            Some(CompletableCommand::PrintArguments(maybe_arg)) => {
                if maybe_arg.trim().is_empty() {
                    return Ok(pairs_from_variants(self.args.iter(), line, maybe_arg, ""));
                }

                let variants = self.arg_hints.predictive_search(maybe_arg);
                if !variants.is_empty() {
                    let variants_iter = variants.iter().map(|var| {
                        std::str::from_utf8(var.as_slice()).expect("invalid utf-8 string")
                    });
                    return Ok(pairs_from_variants(variants_iter, line, maybe_arg, ""));
                }
            }
            Some(CompletableCommand::Unrecognized(cmd, mb_subcmd_part)) => {
                if let Some(subcommands) = self.subcommand_hints.get(cmd) {
                    let pos = cmd.len() + 1;
                    let subcmd_part = mb_subcmd_part.unwrap_or_default();
                    let subcommands = subcommands
                        .iter()
                        .filter(|&subcmd| subcmd.starts_with(subcmd_part))
                        .map(|subcmd| Pair {
                            display: subcmd.to_string(),
                            replacement: subcmd.to_string(),
                        })
                        .collect();

                    return Ok((pos, subcommands));
                }
            }
            _ => {}
        }

        let pairs = self
            .commands
            .iter()
            .filter(|&cmd| cmd.long.starts_with(line))
            .map(|cmd| Pair {
                display: cmd.display_with_short(),
                replacement: cmd.long(),
            })
            .collect();
        Ok((0, pairs))
    }
}

#[derive(Helper, Hinter, Validator)]
pub struct RLHelper {
    pub completer: Arc<Mutex<CommandCompleter>>,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
    pub colored_prompt: String,
}

impl Completer for RLHelper {
    type Candidate = <CommandCompleter as Completer>::Candidate;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        self.completer.lock().unwrap().complete(line, pos, ctx)
    }

    fn update(&self, line: &mut LineBuffer, start: usize, elected: &str, cl: &mut Changeset) {
        self.completer
            .lock()
            .unwrap()
            .update(line, start, elected, cl)
    }
}

impl Highlighter for RLHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        default: bool,
    ) -> Cow<'b, str> {
        if default {
            Borrowed(&self.colored_prompt)
        } else {
            Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Owned(format!("{}", hint.with(Color::Grey)))
    }
}

pub enum BSEditor {
    InMem(Editor<RLHelper, MemHistory>),
    InFile(Editor<RLHelper, FileHistory>),
}

macro_rules! call_editor {
    ($self: expr, $fn_name: tt, $($arg: expr),*) => {
        match $self {
            BSEditor::InMem(editor) => editor.$fn_name($($arg),*),
            BSEditor::InFile(editor) => editor.$fn_name($($arg),*),
        }
    };
}

impl BSEditor {
    const HISTORY_FILE_NAME: &str = ".config/bs/history";

    fn history_file() -> anyhow::Result<PathBuf> {
        let path = home::home_dir().ok_or(anyhow::anyhow!("home dir not found"))?;
        Ok(path.join(Self::HISTORY_FILE_NAME))
    }

    pub fn helper_mut(&mut self) -> Option<&mut RLHelper> {
        call_editor!(self, helper_mut,)
    }

    pub fn readline(&mut self, prompt: &str) -> rustyline::Result<String> {
        call_editor!(self, readline, prompt)
    }

    pub fn add_history_entry<S>(&mut self, line: S) -> rustyline::Result<bool>
    where
        S: AsRef<str> + Into<String>,
    {
        call_editor!(self, add_history_entry, line)
    }

    pub fn create_external_printer(&mut self) -> rustyline::Result<impl ExternalPrinter + 'static> {
        call_editor!(self, create_external_printer,)
    }

    pub fn new(promt: &str, oracles: &[&str], save_history_in_file: bool) -> anyhow::Result<Self> {
        let config: Config = Config::builder()
            .history_ignore_space(true)
            .completion_type(CompletionType::List)
            .build();

        let commands = [
            VAR_COMMAND.into(),
            ARG_COMMAND.into(),
            VAR_DEBUG_COMMAND.into(),
            ARG_DEBUG_COMMAND.into(),
            (CONTINUE_COMMAND_SHORT, CONTINUE_COMMAND).into(),
            CommandHint {
                short: None,
                long: FRAME_COMMAND.to_string(),
                subcommands: vec![
                    FRAME_COMMAND_INFO_SUBCOMMAND.to_string(),
                    FRAME_COMMAND_SWITCH_SUBCOMMAND.to_string(),
                ],
            },
            (RUN_COMMAND_SHORT, RUN_COMMAND).into(),
            STEP_INSTRUCTION_COMMAND.into(),
            (STEP_INTO_COMMAND_SHORT, STEP_INTO_COMMAND).into(),
            (STEP_OUT_COMMAND_SHORT, STEP_OUT_COMMAND).into(),
            (STEP_OVER_COMMAND_SHORT, STEP_OVER_COMMAND).into(),
            SYMBOL_COMMAND.into(),
            (BREAK_COMMAND_SHORT, BREAK_COMMAND).into(),
            CommandHint {
                short: Some(WATCH_COMMAND_SHORT.to_string()),
                long: WATCH_COMMAND.to_string(),
                subcommands: vec![
                    WATCH_REMOVE_SUBCOMMAND.to_string(),
                    WATCH_REMOVE_SUBCOMMAND_SHORT.to_string(),
                    WATCH_INFO_SUBCOMMAND.to_string(),
                ],
            },
            CommandHint {
                short: Some(BACKTRACE_COMMAND_SHORT.to_string()),
                long: BACKTRACE_COMMAND.to_string(),
                subcommands: vec![BACKTRACE_ALL_SUBCOMMAND.to_string()],
            },
            CommandHint {
                short: Some(MEMORY_COMMAND_SHORT.to_string()),
                long: MEMORY_COMMAND.to_string(),
                subcommands: vec![
                    MEMORY_COMMAND_READ_SUBCOMMAND.to_string(),
                    MEMORY_COMMAND_WRITE_SUBCOMMAND.to_string(),
                ],
            },
            CommandHint {
                short: Some(REGISTER_COMMAND_SHORT.to_string()),
                long: REGISTER_COMMAND.to_string(),
                subcommands: vec![
                    REGISTER_COMMAND_READ_SUBCOMMAND.to_string(),
                    REGISTER_COMMAND_WRITE_SUBCOMMAND.to_string(),
                    REGISTER_COMMAND_INFO_SUBCOMMAND.to_string(),
                ],
            },
            (HELP_COMMAND_SHORT, HELP_COMMAND).into(),
            CommandHint {
                short: None,
                long: THREAD_COMMAND.to_string(),
                subcommands: vec![
                    THREAD_COMMAND_INFO_SUBCOMMAND.to_string(),
                    THREAD_COMMAND_SWITCH_SUBCOMMAND.to_string(),
                    THREAD_COMMAND_CURRENT_SUBCOMMAND.to_string(),
                ],
            },
            CommandHint {
                short: None,
                long: SHARED_LIB_COMMAND.to_string(),
                subcommands: vec![SHARED_LIB_COMMAND_INFO_SUBCOMMAND.to_string()],
            },
            CommandHint {
                short: None,
                long: SOURCE_COMMAND.to_string(),
                subcommands: vec![
                    SOURCE_COMMAND_DISASM_SUBCOMMAND.to_string(),
                    SOURCE_COMMAND_FUNCTION_SUBCOMMAND.to_string(),
                ],
            },
            CommandHint {
                short: None,
                long: TRIGGER_COMMAND.to_string(),
                subcommands: vec![
                    TRIGGER_COMMAND_INFO_SUBCOMMAND.to_string(),
                    TRIGGER_COMMAND_ANY_TRIGGER_SUBCOMMAND.to_string(),
                    TRIGGER_COMMAND_BRKPT_TRIGGER_SUBCOMMAND.to_string(),
                    TRIGGER_COMMAND_WP_TRIGGER_SUBCOMMAND.to_string(),
                ],
            },
            CommandHint {
                short: None,
                long: ASYNC_COMMAND.to_string(),
                subcommands: vec![
                    ASYNC_COMMAND_BACKTRACE_SUBCOMMAND.to_string(),
                    ASYNC_COMMAND_BACKTRACE_SUBCOMMAND_SHORT.to_string(),
                    ASYNC_COMMAND_TASK_SUBCOMMAND.to_string(),
                    ASYNC_COMMAND_BACKTRACE_SUBCOMMAND.to_string() + " all",
                    ASYNC_COMMAND_BACKTRACE_SUBCOMMAND_SHORT.to_string() + " all",
                    ASYNC_COMMAND_STEP_OVER_SUBCOMMAND.to_string(),
                    ASYNC_COMMAND_STEP_OVER_SUBCOMMAND_SHORT.to_string(),
                    ASYNC_COMMAND_STEP_OUT_SUBCOMMAND.to_string(),
                    ASYNC_COMMAND_STEP_OUT_SUBCOMMAND_SHORT.to_string(),
                ],
            },
            CALL_COMMAND.into(),
            CommandHint {
                short: None,
                long: ORACLE_COMMAND.to_string(),
                subcommands: oracles.iter().map(ToString::to_string).collect(),
            },
            ("q", "quit").into(),
        ];

        let h = RLHelper {
            completer: Arc::new(Mutex::new(CommandCompleter::new(commands))),
            hinter: HistoryHinter {},
            colored_prompt: format!("{}", promt.with(Color::DarkGreen)),
        };

        if save_history_in_file {
            let mut editor = Editor::with_history(config, FileHistory::with_config(config))?;
            editor.set_helper(Some(h));
            _ = editor.load_history(&Self::history_file()?);

            Ok(Self::InFile(editor))
        } else {
            let mut editor = Editor::with_history(config, MemHistory::new())?;
            editor.set_helper(Some(h));
            Ok(Self::InMem(editor))
        }
    }
}

impl Drop for BSEditor {
    fn drop(&mut self) {
        if let Self::InFile(editor) = self {
            if let Err(e) = Self::history_file()
                .and_then(|file| editor.save_history(&file).map_err(anyhow::Error::from))
            {
                warn!("Failed to save command history: {e}");
            }
        }
    }
}
