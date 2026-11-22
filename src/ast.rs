use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Syntax {
    Proto2,
    Proto3,
    Edition(String),
}

impl Default for Syntax {
    fn default() -> Self {
        Syntax::Proto3
    }
}

impl Syntax {
    pub fn as_str(&self) -> String {
        match self {
            Syntax::Proto2 => "proto2".to_string(),
            Syntax::Proto3 => "proto3".to_string(),
            Syntax::Edition(ed) => format!("edition({})", ed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    Normal,
    Public,
    Weak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub kind: ImportKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoOption {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldLabel {
    Singular,
    Optional,
    Required,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarType {
    Double,
    Float,
    Int32,
    Int64,
    Uint32,
    Uint64,
    Sint32,
    Sint64,
    Fixed32,
    Fixed64,
    Sfixed32,
    Sfixed64,
    Bool,
    String,
    Bytes,
}

impl ScalarType {
    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "double" => Some(Self::Double),
            "float" => Some(Self::Float),
            "int32" => Some(Self::Int32),
            "int64" => Some(Self::Int64),
            "uint32" => Some(Self::Uint32),
            "uint64" => Some(Self::Uint64),
            "sint32" => Some(Self::Sint32),
            "sint64" => Some(Self::Sint64),
            "fixed32" => Some(Self::Fixed32),
            "fixed64" => Some(Self::Fixed64),
            "sfixed32" => Some(Self::Sfixed32),
            "sfixed64" => Some(Self::Sfixed64),
            "bool" => Some(Self::Bool),
            "string" => Some(Self::String),
            "bytes" => Some(Self::Bytes),
            _ => None,
        }
    }

    pub fn proto_name(&self) -> &'static str {
        match self {
            Self::Double => "double",
            Self::Float => "float",
            Self::Int32 => "int32",
            Self::Int64 => "int64",
            Self::Uint32 => "uint32",
            Self::Uint64 => "uint64",
            Self::Sint32 => "sint32",
            Self::Sint64 => "sint64",
            Self::Fixed32 => "fixed32",
            Self::Fixed64 => "fixed64",
            Self::Sfixed32 => "sfixed32",
            Self::Sfixed64 => "sfixed64",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Bytes => "bytes",
        }
    }

    #[allow(dead_code)]
    pub fn descriptor_type_id(&self) -> u8 {
        match self {
            Self::Double => 1,
            Self::Float => 2,
            Self::Int64 => 3,
            Self::Uint64 => 4,
            Self::Int32 => 5,
            Self::Fixed64 => 6,
            Self::Fixed32 => 7,
            Self::Bool => 8,
            Self::String => 9,
            Self::Bytes => 12,
            Self::Uint32 => 13,
            Self::Sfixed32 => 15,
            Self::Sfixed64 => 16,
            Self::Sint32 => 17,
            Self::Sint64 => 18,
        }
    }

    #[allow(dead_code)]
    pub fn wire_type(&self) -> u8 {
        match self {
            Self::Int32
            | Self::Int64
            | Self::Uint32
            | Self::Uint64
            | Self::Sint32
            | Self::Sint64
            | Self::Bool => 0,
            Self::Fixed64 | Self::Sfixed64 | Self::Double => 1,
            Self::String | Self::Bytes => 2,
            Self::Fixed32 | Self::Sfixed32 | Self::Float => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedKind {
    Message,
    Enum,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Scalar(ScalarType),
    Named {
        raw: String,
        resolved_fqn: String,
        kind: ResolvedKind,
    },
    Map(Box<FieldType>, Box<FieldType>),
}

impl FieldType {
    pub fn short_name(&self) -> String {
        match self {
            FieldType::Scalar(s) => s.proto_name().to_string(),
            FieldType::Named { raw, .. } => raw
                .rsplit('.')
                .next()
                .unwrap_or(raw.as_str())
                .to_string(),
            FieldType::Map(k, v) => format!("map<{}, {}>", k.short_name(), v.short_name()),
        }
    }

    #[allow(dead_code)]
    pub fn is_enum(&self) -> bool {
        matches!(
            self,
            FieldType::Named {
                kind: ResolvedKind::Enum,
                ..
            }
        )
    }

    #[allow(dead_code)]
    pub fn is_message(&self) -> bool {
        matches!(
            self,
            FieldType::Named {
                kind: ResolvedKind::Message | ResolvedKind::Unknown,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub number: i32,
    pub label: FieldLabel,
    pub field_type: FieldType,
    pub options: Vec<ProtoOption>,
    pub comments: Vec<String>,
    pub oneof_name: Option<String>,
}

impl Field {
    pub fn is_repeated(&self) -> bool {
        matches!(self.label, FieldLabel::Repeated)
    }

    pub fn is_optional(&self) -> bool {
        matches!(self.label, FieldLabel::Optional)
    }

    #[allow(dead_code)]
    pub fn is_map(&self) -> bool {
        matches!(self.field_type, FieldType::Map(_, _))
    }

    #[allow(dead_code)]
    pub fn is_deprecated(&self) -> bool {
        self.options
            .iter()
            .any(|o| o.name == "deprecated" && o.value == "true")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Oneof {
    pub name: String,
    pub fields: Vec<Field>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedRange {
    pub start: i32,
    pub end: i32, // inclusive
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumValue {
    pub name: String,
    pub number: i32,
    pub options: Vec<ProtoOption>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDef {
    pub name: String,
    pub full_name: String,
    pub values: Vec<EnumValue>,
    pub options: Vec<ProtoOption>,
    pub reserved_ranges: Vec<ReservedRange>,
    pub reserved_names: Vec<String>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub name: String,
    pub full_name: String,
    pub fields: Vec<Field>,
    pub oneofs: Vec<Oneof>,
    pub nested_messages: Vec<Message>,
    pub nested_enums: Vec<EnumDef>,
    pub options: Vec<ProtoOption>,
    pub reserved_ranges: Vec<ReservedRange>,
    pub reserved_names: Vec<String>,
    pub comments: Vec<String>,
}

impl Message {
    /// Returns all fields including fields inside `oneof` declarations, sorted by declaration order.
    pub fn all_fields(&self) -> Vec<Field> {
        let mut out = self.fields.clone();
        for o in &self.oneofs {
            for f in &o.fields {
                out.push(f.clone());
            }
        }
        out
    }

    /// Computes free field number ranges up to max occupied +headroom.
    #[allow(dead_code)]
    pub fn free_field_ranges(&self) -> Vec<(i32, Option<i32>)> {
        let mut occupied = BTreeSet::new();
        for f in self.all_fields() {
            occupied.insert(f.number);
        }
        for r in &self.reserved_ranges {
            let cap = r.end.min(50_000);
            for n in r.start..=cap {
                occupied.insert(n);
            }
        }
        for n in 19000..=19999 {
            occupied.insert(n);
        }
        let max_num = occupied
            .iter()
            .copied()
            .filter(|&n| !(19000..=19999).contains(&n))
            .max()
            .unwrap_or(0);
        let mut ranges = Vec::new();
        let mut cur_start: Option<i32> = None;
        for n in 1..=max_num {
            if !occupied.contains(&n) {
                if cur_start.is_none() {
                    cur_start = Some(n);
                }
            } else if let Some(s) = cur_start.take() {
                ranges.push((s, Some(n - 1)));
            }
        }
        if let Some(s) = cur_start.take() {
            ranges.push((s, None));
        } else {
            let next_start = if (19000..=19999).contains(&(max_num + 1)) {
                20000
            } else {
                max_num + 1
            };
            ranges.push((next_start, None));
        }
        ranges
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcMethod {
    pub name: String,
    pub input_type: String,
    pub input_resolved_fqn: String,
    pub output_type: String,
    pub output_resolved_fqn: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
    pub options: Vec<ProtoOption>,
    pub comments: Vec<String>,
}

impl RpcMethod {
    pub fn streaming_mode(&self) -> &'static str {
        match (self.client_streaming, self.server_streaming) {
            (false, false) => "Unary",
            (false, true) => "ServerStreaming",
            (true, false) => "ClientStreaming",
            (true, true) => "BidiStreaming",
        }
    }

    pub fn input_short(&self) -> &str {
        self.input_type
            .rsplit('.')
            .next()
            .unwrap_or(&self.input_type)
    }

    pub fn output_short(&self) -> &str {
        self.output_type
            .rsplit('.')
            .next()
            .unwrap_or(&self.output_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub full_name: String,
    pub methods: Vec<RpcMethod>,
    pub options: Vec<ProtoOption>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtoFile {
    pub name: String,
    pub source_path: PathBuf,
    pub is_wkt: bool,
    pub syntax: Syntax,
    pub package: String,
    pub imports: Vec<Import>,
    pub options: Vec<ProtoOption>,
    pub messages: Vec<Message>,
    pub enums: Vec<EnumDef>,
    pub services: Vec<Service>,
    pub comments: Vec<String>,
}

impl ProtoFile {
    pub fn file_stem(&self) -> String {
        self.source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("proto_out")
            .replace('-', "_")
    }

    pub fn option_value(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|o| o.name == key)
            .map(|o| o.value.as_str())
    }

    /// Flattens top-level and nested messages in topological/declaration order.
    pub fn all_messages_flat(&self) -> Vec<Message> {
        let mut out = Vec::new();
        fn visit_msg(m: &Message, prefix: &str, out: &mut Vec<Message>) {
            let mut copy = m.clone();
            if !prefix.is_empty() {
                copy.name = format!("{}_{}", prefix, m.name);
            }
            let next_prefix = copy.name.clone();
            for nested in &m.nested_messages {
                visit_msg(nested, &next_prefix, out);
            }
            out.push(copy);
        }
        for m in &self.messages {
            visit_msg(m, "", &mut out);
        }
        out
    }

    /// Flattens top-level and nested enums.
    pub fn all_enums_flat(&self) -> Vec<EnumDef> {
        let mut out = self.enums.clone();
        fn visit_enums(m: &Message, prefix: &str, out: &mut Vec<EnumDef>) {
            let cur = if prefix.is_empty() {
                m.name.clone()
            } else {
                format!("{}_{}", prefix, m.name)
            };
            for e in &m.nested_enums {
                let mut ec = e.clone();
                ec.name = format!("{}_{}", cur, e.name);
                out.push(ec);
            }
            for nm in &m.nested_messages {
                visit_enums(nm, &cur, out);
            }
        }
        for m in &self.messages {
            visit_enums(m, "", &mut out);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SchemaBundle {
    pub files: Vec<ProtoFile>,
    pub type_registry: BTreeMap<String, ResolvedKind>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetLang {
    Rust,
    Go,
    Python,
    TypeScript,
    JavaScript,
    Cpp,
    Java,
    Kotlin,
    CSharp,
    Swift,
    Dart,
    Php,
    Ruby,
    ObjC,
    C,
}

impl TargetLang {
    pub fn all() -> &'static [TargetLang] {
        &[
            TargetLang::Rust,
            TargetLang::Go,
            TargetLang::Python,
            TargetLang::TypeScript,
            TargetLang::JavaScript,
            TargetLang::Cpp,
            TargetLang::Java,
            TargetLang::Kotlin,
            TargetLang::CSharp,
            TargetLang::Swift,
            TargetLang::Dart,
            TargetLang::Php,
            TargetLang::Ruby,
            TargetLang::ObjC,
            TargetLang::C,
        ]
    }

    pub fn from_token(token: &str) -> Option<Vec<TargetLang>> {
        let lower = token.trim().to_ascii_lowercase();
        match lower.as_str() {
            "all" | "*" => Some(Self::all().to_vec()),
            "rust" | "rs" | "tonic" | "prost" => Some(vec![Self::Rust]),
            "go" | "golang" | "go-grpc" => Some(vec![Self::Go]),
            "python" | "py" | "pyi" => Some(vec![Self::Python]),
            "typescript" | "ts" | "grpc-web" => Some(vec![Self::TypeScript]),
            "javascript" | "js" | "node" => Some(vec![Self::JavaScript]),
            "cpp" | "c++" | "cxx" | "cc" => Some(vec![Self::Cpp]),
            "java" => Some(vec![Self::Java]),
            "kotlin" | "kt" => Some(vec![Self::Kotlin]),
            "csharp" | "cs" | "c#" | "dotnet" => Some(vec![Self::CSharp]),
            "swift" => Some(vec![Self::Swift]),
            "dart" | "flutter" => Some(vec![Self::Dart]),
            "php" => Some(vec![Self::Php]),
            "ruby" | "rb" | "rbs" => Some(vec![Self::Ruby]),
            "objc" | "objective-c" | "objectivec" => Some(vec![Self::ObjC]),
            "c" | "nanopb" => Some(vec![Self::C]),
            _ => None,
        }
    }

    pub fn canonical_id(&self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Cpp => "cpp",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::CSharp => "csharp",
            Self::Swift => "swift",
            Self::Dart => "dart",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::ObjC => "objc",
            Self::C => "c",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Rust => "Rust (Prost/Tonic + Wire Codec)",
            Self::Go => "Go (Protobuf v2 + gRPC-Go)",
            Self::Python => "Python (Dataclasses + gRPC + .pyi Stubs)",
            Self::TypeScript => "TypeScript (Typed Classes + gRPC Client/Server)",
            Self::JavaScript => "JavaScript (CommonJS/ESM + gRPC Service)",
            Self::Cpp => "C++ (.pb.h/.pb.cc + .grpc.pb.h/.grpc.pb.cc)",
            Self::Java => "Java (Builders + gRPC Stubs/ImplBase)",
            Self::Kotlin => "Kotlin (Data Classes + Coroutine gRPC)",
            Self::CSharp => "C# (.NET Records/Classes + gRPC Client/Base)",
            Self::Swift => "Swift (Codable/Sendable + Async gRPC)",
            Self::Dart => "Dart (.pb.dart + .pbenum.dart + .pbgrpc.dart)",
            Self::Php => "PHP 8+ (Typed Messages + gRPC Client/Service)",
            Self::Ruby => "Ruby (DSL Modules + gRPC + .rbs Signatures)",
            Self::ObjC => "Objective-C (.pbobjc.h/.m + .pbrpc.h/.m)",
            Self::C => "C (Zero-Allocation Structs + gRPC Service Table)",
        }
    }

    pub fn aliases(&self) -> &'static str {
        match self {
            Self::Rust => "rust, rs, tonic, prost",
            Self::Go => "go, golang, go-grpc",
            Self::Python => "python, py, pyi",
            Self::TypeScript => "typescript, ts, grpc-web",
            Self::JavaScript => "javascript, js, node",
            Self::Cpp => "cpp, c++, cxx, cc",
            Self::Java => "java",
            Self::Kotlin => "kotlin, kt",
            Self::CSharp => "csharp, cs, c#, dotnet",
            Self::Swift => "swift",
            Self::Dart => "dart, flutter",
            Self::Php => "php",
            Self::Ruby => "ruby, rb, rbs",
            Self::ObjC => "objc, objective-c, objectivec",
            Self::C => "c, nanopb",
        }
    }

    pub fn output_extensions(&self) -> &'static str {
        match self {
            Self::Rust => "<name>.rs",
            Self::Go => "<name>.pb.go, <name>_grpc.pb.go",
            Self::Python => "<name>_pb2.py, <name>_pb2.pyi, <name>_pb2_grpc.py",
            Self::TypeScript => "<name>.pb.ts, <name>.grpc.ts",
            Self::JavaScript => "<name>_pb.js, <name>_grpc_pb.js",
            Self::Cpp => "<name>.pb.h, <name>.pb.cc, <name>.grpc.pb.h, <name>.grpc.pb.cc",
            Self::Java => "<Name>Proto.java, <Service>Grpc.java",
            Self::Kotlin => "<Name>Kt.kt, <Name>GrpcKt.kt",
            Self::CSharp => "<Name>.pb.cs, <Name>Grpc.cs",
            Self::Swift => "<Name>.pb.swift, <Name>.grpc.swift",
            Self::Dart => "<name>.pb.dart, <name>.pbenum.dart, <name>.pbgrpc.dart",
            Self::Php => "<Name>Pb.php, <Service>GrpcClient.php",
            Self::Ruby => "<name>_pb.rb, <name>_services_pb.rb, <name>_pb.rbs",
            Self::ObjC => "<Name>.pbobjc.h, <Name>.pbobjc.m, <Name>.pbrpc.h, <Name>.pbrpc.m",
            Self::C => "<name>.pb.h, <name>.pb.c",
        }
    }
}
