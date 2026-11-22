use crate::ast::TargetLang;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandMode {
    Build,
    Init { dir: PathBuf },
    Inspect,
    Validate,
    ListLanguages,
    Help { topic: Option<String> },
    Version,
}

#[derive(Debug, Clone)]
pub struct CliAction {
    pub mode: CommandMode,
    pub inputs: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub languages: Vec<TargetLang>,
    pub out_dir: PathBuf,
    pub config_file: Option<PathBuf>,
    pub generate_messages: bool,
    pub generate_grpc: bool,
    pub clean_out: bool,
    pub dry_run: bool,
    pub verbose: bool,
    pub quiet: bool,
}

impl Default for CliAction {
    fn default() -> Self {
        Self {
            mode: CommandMode::Build,
            inputs: Vec::new(),
            include_dirs: Vec::new(),
            languages: Vec::new(),
            out_dir: PathBuf::from("./gen"),
            config_file: None,
            generate_messages: true,
            generate_grpc: true,
            clean_out: false,
            dry_run: false,
            verbose: false,
            quiet: false,
        }
    }
}

pub fn parse_cli_args(args: &[String]) -> Result<CliAction, String> {
    let mut act = CliAction::default();
    let mut lang_set: BTreeSet<TargetLang> = BTreeSet::new();

    if args.is_empty() {
        let default_cfg = PathBuf::from("protobuilder.toml");
        if default_cfg.exists() {
            apply_toml_config(&default_cfg, &mut act, &mut lang_set)?;
            if act.languages.is_empty() {
                act.languages = if lang_set.is_empty() {
                    TargetLang::all().to_vec()
                } else {
                    lang_set.into_iter().collect()
                };
            }
            return Ok(act);
        }
        act.mode = CommandMode::Help { topic: None };
        return Ok(act);
    }

    let mut i = 0;
    if !args[0].starts_with('-') {
        match args[0].as_str() {
            "build" | "generate" | "gen" => {
                act.mode = CommandMode::Build;
                i += 1;
            }
            "init" | "new" => {
                let dir = args
                    .get(1)
                    .filter(|s| !s.starts_with('-'))
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                act.mode = CommandMode::Init { dir };
                return Ok(act);
            }
            "inspect" | "info" => {
                act.mode = CommandMode::Inspect;
                i += 1;
            }
            "validate" | "check" => {
                act.mode = CommandMode::Validate;
                i += 1;
            }
            "languages" | "langs" => {
                act.mode = CommandMode::ListLanguages;
                return Ok(act);
            }
            "help" => {
                let topic = args.get(1).cloned();
                act.mode = CommandMode::Help { topic };
                return Ok(act);
            }
            "version" => {
                act.mode = CommandMode::Version;
                return Ok(act);
            }
            _ => {}
        }
    }

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                let topic = args
                    .get(i + 1)
                    .filter(|s| !s.starts_with('-'))
                    .cloned();
                act.mode = CommandMode::Help { topic };
                return Ok(act);
            }
            "-V" | "--version" => {
                act.mode = CommandMode::Version;
                return Ok(act);
            }
            "--languages" => {
                act.mode = CommandMode::ListLanguages;
                return Ok(act);
            }
            "-l" | "--lang" => {
                i += 1;
                let val = args.get(i).ok_or_else(|| {
                    "Option '-l / --lang' requires a language value (e.g. rust,go,all)".to_string()
                })?;
                parse_lang_list(val, &mut lang_set)?;
            }
            "-o" | "--out" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| "Option '-o / --out' requires an output directory path".to_string())?;
                act.out_dir = PathBuf::from(val);
            }
            "-I" | "--include" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| "Option '-I / --include' requires a directory path".to_string())?;
                act.include_dirs.push(PathBuf::from(val));
            }
            "-c" | "--config" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| "Option '-c / --config' requires a path to protobuilder.toml".to_string())?;
                act.config_file = Some(PathBuf::from(val));
            }
            "--grpc-only" => {
                act.generate_messages = false;
                act.generate_grpc = true;
            }
            "--messages-only" => {
                act.generate_messages = true;
                act.generate_grpc = false;
            }
            "--clean" => {
                act.clean_out = true;
            }
            "--dry-run" => {
                act.dry_run = true;
            }
            "-v" | "--verbose" => {
                act.verbose = true;
            }
            "-q" | "--quiet" => {
                act.quiet = true;
            }
            other if other.starts_with("--") && other.contains('=') => {
                let (k, v) = other.split_once('=').unwrap();
                match k {
                    "--lang" => parse_lang_list(v, &mut lang_set)?,
                    "--out" => act.out_dir = PathBuf::from(v),
                    "--include" => act.include_dirs.push(PathBuf::from(v)),
                    "--config" => act.config_file = Some(PathBuf::from(v)),
                    _ => {
                        return Err(format!(
                            "Unknown option '{}'. Run 'protobuilder --help' for usage.",
                            other
                        ));
                    }
                }
            }
            other if other.starts_with("-I") && other.len() > 2 => {
                act.include_dirs.push(PathBuf::from(&other[2..]));
            }
            other if other.starts_with("-l") && other.len() > 2 => {
                parse_lang_list(&other[2..], &mut lang_set)?;
            }
            other if other.starts_with("-o") && other.len() > 2 => {
                act.out_dir = PathBuf::from(&other[2..]);
            }
            other if other.starts_with('-') => {
                return Err(format!(
                    "Unrecognized option '{}'. Run 'protobuilder --help' for usage.",
                    other
                ));
            }
            positional => {
                act.inputs.push(PathBuf::from(positional));
            }
        }
        i += 1;
    }

    if let Some(cfg_path) = act.config_file.clone() {
        apply_toml_config(&cfg_path, &mut act, &mut lang_set)?;
    } else if PathBuf::from("protobuilder.toml").exists() && (act.inputs.is_empty() || lang_set.is_empty()) {
        apply_toml_config(Path::new("protobuilder.toml"), &mut act, &mut lang_set)?;
    }

    if lang_set.is_empty() {
        for &l in TargetLang::all() {
            lang_set.insert(l);
        }
    }
    act.languages = lang_set.into_iter().collect();

    if act.inputs.is_empty()
        && matches!(
            act.mode,
            CommandMode::Build | CommandMode::Inspect | CommandMode::Validate
        )
    {
        for candidate in ["proto", "protos", "examples", "."] {
            let p = PathBuf::from(candidate);
            if p.exists() {
                act.inputs.push(p);
                break;
            }
        }
    }

    Ok(act)
}

fn parse_lang_list(val: &str, set: &mut BTreeSet<TargetLang>) -> Result<(), String> {
    for part in val.split(|c| c == ',' || c == '+' || c == ';') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(langs) = TargetLang::from_token(trimmed) {
            for l in langs {
                set.insert(l);
            }
        } else {
            return Err(format!(
                "Unsupported language '{}'. Supported: all, rust, go, python, ts, js, cpp, java, kotlin, csharp, swift, dart, php, ruby, objc, c",
                trimmed
            ));
        }
    }
    Ok(())
}

fn apply_toml_config(
    path: &Path,
    act: &mut CliAction,
    lang_set: &mut BTreeSet<TargetLang>,
) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read config file '{}': {}", path.display(), e))?;
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            match key {
                "inputs" | "files" | "protos" => {
                    if act.inputs.is_empty() {
                        for item in parse_toml_string_list(val) {
                            act.inputs.push(PathBuf::from(item));
                        }
                    }
                }
                "includes" => {
                    for item in parse_toml_string_list(val) {
                        let p = PathBuf::from(item);
                        if !act.include_dirs.contains(&p) {
                            act.include_dirs.push(p);
                        }
                    }
                }
                "languages" | "targets" | "lang" => {
                    if lang_set.is_empty() {
                        for item in parse_toml_string_list(val) {
                            parse_lang_list(&item, lang_set)?;
                        }
                    }
                }
                "out_dir" | "output" => {
                    if act.out_dir == PathBuf::from("./gen") {
                        let clean = val.trim_matches('"').trim_matches('\'');
                        act.out_dir = PathBuf::from(clean);
                    }
                }
                "grpc" => {
                    if val == "false" {
                        act.generate_grpc = false;
                    }
                }
                "messages" => {
                    if val == "false" {
                        act.generate_messages = false;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn parse_toml_string_list(val: &str) -> Vec<String> {
    let trimmed = val.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        trimmed[1..trimmed.len() - 1]
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![trimmed.trim_matches('"').trim_matches('\'').to_string()]
    }
}

pub fn render_help(topic: Option<&str>) -> String {
    match topic.map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("languages") | Some("langs") => render_languages_table(),
        Some("config") | Some("toml") => render_config_help(),
        _ => render_full_help(),
    }
}

pub fn render_full_help() -> String {
    String::from(
        r#"================================================================================
  PROTOBUILDER v1.0.0 — All-in-One Protocol Buffers & gRPC Code Generator
  Single Executable • Zero Dependencies • 15 Built-In Programming Languages
================================================================================

USAGE
  protobuilder [COMMAND] [OPTIONS] <PROTO_FILES_OR_DIRS>...
  protobuilder <PROTO_FILES_OR_DIRS>... -l <LANGS> -o <OUT_DIR>

QUICK START EXAMPLES
  1. Generate ALL 15 languages (Protobuf messages + gRPC client/server stubs):
     $ protobuilder service.proto -l all -o ./gen
     $ protobuilder ./protos -l all -o ./gen

  2. Generate specific languages (comma-separated or repeated `-l`):
     $ protobuilder service.proto -l rust,go,python,ts -o ./gen

  3. Scaffold a new project and build with zero arguments:
     $ protobuilder init ./my-project
     $ cd ./my-project && protobuilder build

  4. Inspect or validate `.proto` schemas:
     $ protobuilder inspect service.proto
     $ protobuilder validate ./protos

COMMANDS
  build           Compile .proto files or directories into target languages (default)
  init [DIR]      Create a starter .proto schema and protobuilder.toml config
  inspect         Display parsed messages, fields, enums, and gRPC service methods
  validate        Validate .proto syntax, field numbers, imports, and type references
  languages       List all 15 built-in languages, aliases, and output file patterns
  help [TOPIC]    Show help guide (topics: languages, config)

OPTIONS
  -l, --lang <LANGS>       Target language(s): comma-separated list or `all` (default: all)
                           Examples: `-l rust,go,ts`, `-l python`, `-l all`
  -o, --out <DIR>          Output directory (default: `./gen`)
  -I, --include <DIR>      Add directory to import search path (parent folders & standard
                           `google/protobuf/*.proto` types are resolved automatically)
  -c, --config <FILE>      Load project settings from TOML file (default: `./protobuilder.toml`)
      --grpc-only          Generate only gRPC client & server service stubs
      --messages-only      Generate only Protobuf message & enum types (skip gRPC stubs)
      --clean              Clean the output directory before generating new files
      --dry-run            Validate and run code generation in memory without writing files
  -v, --verbose            Print every generated file path and byte size
  -q, --quiet              Suppress non-error output
  -h, --help [TOPIC]       Display this help guide
  -V, --version            Display version information

BUILT-IN LANGUAGES (15 TARGETS — ZERO EXTERNAL DEPENDENCIES)
  ID           ALIASES                  GENERATED FILES (MESSAGES + gRPC)
  -----------  -----------------------  ----------------------------------------------------
  rust         rs, tonic, prost         <name>.rs
  go           golang, go-grpc          <name>.pb.go, <name>_grpc.pb.go
  python       py, pyi                  <name>_pb2.py, <name>_pb2.pyi, <name>_pb2_grpc.py
  typescript   ts, grpc-web             <name>.pb.ts, <name>.grpc.ts
  javascript   js, node                 <name>_pb.js, <name>_grpc_pb.js
  cpp          c++, cxx, cc             <name>.pb.h, <name>.pb.cc, <name>.grpc.pb.h, <name>.grpc.pb.cc
  java         java                     <Name>Proto.java, <Service>Grpc.java
  kotlin       kt                       <Name>Kt.kt, <Name>GrpcKt.kt
  csharp       cs, c#, dotnet           <Name>.pb.cs, <Name>Grpc.cs
  swift        swift                    <Name>.pb.swift, <Name>.grpc.swift
  dart         flutter                  <name>.pb.dart, <name>.pbenum.dart, <name>.pbgrpc.dart
  php          php                      <Name>Pb.php, <Service>GrpcClient.php
  ruby         rb, rbs                  <name>_pb.rb, <name>_services_pb.rb, <name>_pb.rbs
  objc         objective-c, objectivec  <Name>.pbobjc.h, <Name>.pbobjc.m, <Name>.pbrpc.h, <Name>.pbrpc.m
  c            nanopb                   <name>.pb.h, <name>.pb.c
  all          *                        Generates all 15 languages above in a single pass!

BUILT-IN STANDARD IMPORTS
  The following `google/protobuf/*.proto` imports are embedded directly inside `protobuilder`:
  • google/protobuf/empty.proto          • google/protobuf/timestamp.proto
  • google/protobuf/duration.proto       • google/protobuf/wrappers.proto
  • google/protobuf/any.proto            • google/protobuf/field_mask.proto
  • google/protobuf/struct.proto         • google/protobuf/descriptor.proto

CONFIGURATION FILE (`protobuilder.toml`)
  Place a `protobuilder.toml` in your project directory to run `protobuilder` with no flags:
    inputs    = ["examples/ecommerce.proto"]
    includes  = ["examples"]
    languages = ["all"]
    out_dir   = "./gen"
    grpc      = true
    messages  = true
"#,
    )
}

pub fn render_languages_table() -> String {
    let mut s = String::from(
        "BUILT-IN TARGET LANGUAGES (15 Languages + gRPC Stubs Built-In)\n\
         ================================================================================\n",
    );
    for l in TargetLang::all() {
        s.push_str(&format!(
            "• {:<11} | {:<45}\n  Aliases:    {}\n  Outputs:    {}\n\n",
            l.canonical_id(),
            l.display_name(),
            l.aliases(),
            l.output_extensions()
        ));
    }
    s
}

pub fn render_config_help() -> String {
    String::from(
        r#"PROTOBUILDER.TOML CONFIGURATION GUIDE
================================================================================
Place a `protobuilder.toml` file in your project root (or generate one with
`protobuilder init`) to run `protobuilder` with zero command-line arguments:

```toml
# protobuilder.toml
inputs    = ["protos/service.proto"]
includes  = ["protos"]
languages = ["rust", "go", "python", "typescript"] # or ["all"]
out_dir   = "./gen"
grpc      = true
messages  = true
```
"#,
    )
}
