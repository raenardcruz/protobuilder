# ProtoBuilder

**ProtoBuilder (`protobuilder`)** is an all-in-one, zero-dependency Protocol Buffers & gRPC compiler and multi-language code generator written in Rust. It compiles `.proto` schemas (both Protobuf data models and gRPC client/server service stubs) for **15 programming languages** from a **single standalone executable**.

---

## 1. Installation

### Quick Install

**macOS & Linux (`curl`)** — installs `protobuilder` and registers `PROTOBUILDER_HOME` and `PATH` in your shell profile (`~/.zshrc`, `~/.bashrc`, `~/.bash_profile`, `~/.profile`, or Fish `config.fish`):

```bash
curl -fsSL https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.sh | sh
```

**Windows (PowerShell)** — installs `protobuilder.exe` and registers `PROTOBUILDER_HOME` and User `PATH`:

```powershell
irm https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.ps1 | iex
```

Optional flags and environment variables:

```bash
# macOS / Linux — pin a specific release version:
curl -fsSL https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.sh | sh -s -- --version v1.0.0

# macOS / Linux — install to a custom directory:
curl -fsSL https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.sh | sh -s -- --dir ~/.local/bin

# macOS / Linux — skip modifying shell profile files:
curl -fsSL https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.sh | sh -s -- --no-modify-path
```

```powershell
# Windows (PowerShell) — pin a version or custom directory via environment variables:
$env:PROTOBUILDER_VERSION = "v1.0.0"
$env:PROTOBUILDER_INSTALL_DIR = "$HOME\.local\bin"
irm https://raw.githubusercontent.com/raenardcruz/protobuilder/main/install.ps1 | iex
```

> Prebuilt release archives are produced for **macOS** (`arm64`, `x86_64`), **Linux** (`x86_64`, `arm64`), and **Windows** (`x86_64`) via GitHub Actions. If a prebuilt archive is unavailable for your OS/architecture, both installers automatically fall back to building from source with `cargo` if Rust is installed.

### Manual Download & Make Executable

1. Download the `protobuilder` binary (or release archive) from the [**GitHub Releases**](https://github.com/raenardcruz/protobuilder/releases) page.
2. Grant execute permission and move it to a directory on your `PATH`:

```bash
# If downloading the raw binary:
chmod +x ./protobuilder
sudo mv ./protobuilder /usr/local/bin/protobuilder

# Or if downloading the release archive (protobuilder-v1.0.0-macos-arm64.tar.gz):
tar -xzf protobuilder-v1.0.0-macos-arm64.tar.gz
chmod +x ./protobuilder
sudo mv ./protobuilder /usr/local/bin/protobuilder
```

> **macOS Gatekeeper Note:** If macOS blocks an unsigned binary downloaded from a browser, remove the quarantine attribute with:
> ```bash
> xattr -d com.apple.quarantine /usr/local/bin/protobuilder
> ```

3. Verify the installation:

```bash
protobuilder --version
protobuilder --help
```

---

## 2. Quick Start Usage

### Generate All 15 Programming Languages at Once
Compiles both Protobuf messages/enums and gRPC client & server stubs for all 15 built-in languages in a single pass:

```bash
protobuilder examples/ecommerce.proto -l all -o ./gen
```

### Generate Specific Languages
Pass a comma-separated list of target languages (or repeat `-l`):

```bash
protobuilder examples/ecommerce.proto -l rust,go,python,ts -o ./gen
```

### Compile an Entire Directory of `.proto` Files
Pass a directory path to automatically discover all `.proto` files and resolve their imports:

```bash
protobuilder ./examples -l rust,go,cpp,java -o ./gen
```

### Initialize a New Project & Build with Zero Arguments
Scaffold a starter `.proto` file and `protobuilder.toml` configuration:

```bash
protobuilder init ./my-service
cd ./my-service
protobuilder build
```

### Inspect or Validate `.proto` Schemas

```bash
# View parsed messages, fields, enums, and gRPC RPC streaming modes:
protobuilder inspect examples/ecommerce.proto

# Validate syntax, field tag numbers, imports, and type references:
protobuilder validate ./examples
```

---

## 3. CLI Commands & Options

### Commands

| Command | Description | Example |
| :--- | :--- | :--- |
| `build` | Compile `.proto` files or directories into target languages (default when omitted). | `protobuilder build service.proto -l rust,go -o ./gen` |
| `init [DIR]` | Create a starter `protos/service.proto` schema and `protobuilder.toml` config. | `protobuilder init ./my-app` |
| `inspect <PROTOS>...` | Display parsed packages, imports, options, enums, messages, fields, and gRPC services. | `protobuilder inspect examples/ecommerce.proto` |
| `validate <PROTOS>...` | Validate `.proto` syntax, field numbers, reserved ranges, imports, and type references. | `protobuilder validate ./examples` |
| `languages` | List all 15 built-in target languages, aliases, and output file patterns. | `protobuilder languages` |
| `help [TOPIC]` | Display help (`languages`, `config`). | `protobuilder --help` |

### Options

| Option | Description |
| :--- | :--- |
| `-l, --lang <LANGS>` | Target language(s): comma-separated list (e.g. `rust,go,python,ts`) or `all` (default: `all`). |
| `-o, --out <DIR>` | Output directory (default: `./gen`). Writes directly to `<DIR>` for a single language or `<DIR>/<lang>/` for multiple languages. |
| `-I, --include <DIR>` | Add directory to import search path (parent folders & standard `google/protobuf/*.proto` types are resolved automatically). |
| `-c, --config <FILE>` | Load project settings from a TOML file (auto-loads `./protobuilder.toml` when present). |
| `--grpc-only` | Generate only gRPC client & server service stubs. |
| `--messages-only` | Generate only Protobuf message & enum types (skip gRPC stubs). |
| `--clean` | Clean the output directory before generating new files. |
| `--dry-run` | Validate and run code generation in memory without writing files to disk. |
| `-v, --verbose` | Print every generated file path and byte size. |
| `-q, --quiet` | Suppress non-error output. |
| `-h, --help` | Display detailed CLI help. |
| `-V, --version` | Display version information. |

---

## 4. All 15 Built-In Programming Languages

No external plugins or dependencies are needed. Every generator below is compiled directly into the `protobuilder` executable:

| Language ID | CLI Aliases | Generated Files (Messages + gRPC) |
| :--- | :--- | :--- |
| `rust` | `rs`, `tonic`, `prost` | `<name>.rs` |
| `go` | `golang`, `go-grpc` | `<name>.pb.go`, `<name>_grpc.pb.go` |
| `python` | `py`, `pyi` | `<name>_pb2.py`, `<name>_pb2.pyi`, `<name>_pb2_grpc.py` |
| `typescript` | `ts`, `grpc-web` | `<name>.pb.ts`, `<name>.grpc.ts` |
| `javascript` | `js`, `node` | `<name>_pb.js`, `<name>_grpc_pb.js` |
| `cpp` | `c++`, `cxx`, `cc` | `<name>.pb.h`, `<name>.pb.cc`, `<name>.grpc.pb.h`, `<name>.grpc.pb.cc` |
| `java` | `java` | `<OuterClass>.java`, `<Service>Grpc.java` |
| `kotlin` | `kt` | `<Name>Kt.kt`, `<Name>GrpcKt.kt` |
| `csharp` | `cs`, `c#`, `dotnet` | `<Name>.pb.cs`, `<Name>Grpc.cs` |
| `swift` | `swift` | `<Name>.pb.swift`, `<Name>.grpc.swift` |
| `dart` | `flutter` | `<name>.pb.dart`, `<name>.pbenum.dart`, `<name>.pbgrpc.dart` |
| `php` | `php` | `<Name>Pb.php`, `<Service>GrpcClient.php` |
| `ruby` | `rb`, `rbs` | `<name>_pb.rb`, `<name>_services_pb.rb`, `<name>_pb.rbs` |
| `objc` | `objective-c`, `objectivec` | `<Name>.pbobjc.h`, `<Name>.pbobjc.m`, `<Name>.pbrpc.h`, `<Name>.pbrpc.m` |
| `c` | `nanopb` | `<name>.pb.h`, `<name>.pb.c` |
| `all` | `*` | Generates all 15 languages above in a single pass |

---

## 5. Project Configuration (`protobuilder.toml`)

When a `protobuilder.toml` file is present in your working directory, you can simply run `protobuilder` or `protobuilder build`:

```toml
# protobuilder.toml
inputs    = ["examples/ecommerce.proto"]
includes  = ["examples"]
languages = ["all"]
out_dir   = "./gen"
grpc      = true
messages  = true
```

---

## 6. Building from Source

```bash
git clone https://github.com/raenardcruz/protobuilder.git
cd protobuilder
cargo build --release
./target/release/protobuilder --help
```

See [readme.html](./readme.html) for the full HTML documentation guide.
