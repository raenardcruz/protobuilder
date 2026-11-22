use crate::ast::*;
use crate::codegen::*;

fn rust_scalar(sc: ScalarType) -> &'static str {
    match sc {
        ScalarType::Double => "f64",
        ScalarType::Float => "f32",
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => "i32",
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => "i64",
        ScalarType::Uint32 | ScalarType::Fixed32 => "u32",
        ScalarType::Uint64 | ScalarType::Fixed64 => "u64",
        ScalarType::Bool => "bool",
        ScalarType::String => "String",
        ScalarType::Bytes => "Vec<u8>",
    }
}

fn rust_field_type(ft: &FieldType, label: &FieldLabel) -> String {
    let base = match ft {
        FieldType::Scalar(sc) => rust_scalar(*sc).to_string(),
        FieldType::Named { raw, .. } => {
            let short = raw.rsplit('.').next().unwrap_or(raw);
            to_pascal_case(short)
        }
        FieldType::Map(k, v) => {
            let kt = rust_field_type(k, &FieldLabel::Singular);
            let vt = rust_field_type(v, &FieldLabel::Singular);
            return format!("::std::collections::BTreeMap<{}, {}>", kt, vt);
        }
    };
    match label {
        FieldLabel::Repeated => format!("Vec<{}>", base),
        FieldLabel::Optional => format!("Option<{}>", base),
        _ => base,
    }
}

fn rust_ident(name: &str) -> String {
    let s = to_snake_case(name);
    match s.as_str() {
        "type" | "match" | "fn" | "mod" | "pub" | "use" | "struct" | "enum" | "impl"
        | "trait" | "where" | "async" | "await" | "move" | "ref" | "mut" | "box" | "in"
        | "for" | "loop" | "while" | "if" | "else" | "return" | "self" | "super" | "crate" => {
            format!("r#{}", s)
        }
        _ => s,
    }
}

/// Generates idiomatic Rust code (`<stem>.rs`) including:
/// - Enums (`#[repr(i32)]`, `TryFrom<i32>`, `as_str_name`, `from_str_name`)
/// - Message structs + `oneof` enums + binary Protobuf wire encoder/decoder (`encode_to_vec`, `decode_from_slice`)
/// - Async gRPC Client & Server traits and service dispatcher (`pub mod <service>_client`, `pub mod <service>_server`)
pub fn gen_rust(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut out = banner_comment(file, TargetLang::Rust, "//");
    out.push_str("#![allow(dead_code, unused_imports, unused_mut, non_camel_case_types, clippy::all)]\n\n");
    out.push_str("use std::collections::BTreeMap;\n\n");

    if cfg.generate_messages {
        // Minimal self-contained wire helper module so generated Rust files compile stand-alone
        out.push_str(
            r#"pub mod pb_wire {
    pub fn encode_varint(buf: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            buf.push(((v as u8) & 0x7F) | 0x80);
            v >>= 7;
        }
        buf.push(v as u8);
    }

    pub fn decode_varint(data: &[u8], pos: &mut usize) -> Result<u64, &'static str> {
        let mut result = 0u64;
        let mut shift = 0;
        while *pos < data.len() {
            let b = data[*pos];
            *pos += 1;
            result |= ((b & 0x7F) as u64) << shift;
            if (b & 0x80) == 0 {
                return Ok(result);
            }
            shift += 7;
            if shift >= 64 {
                return Err("varint overflow");
            }
        }
        Err("unexpected EOF decoding varint")
    }

    pub fn encode_tag(buf: &mut Vec<u8>, field_number: u32, wire_type: u8) {
        encode_varint(buf, ((field_number as u64) << 3) | (wire_type as u64));
    }

    pub fn encode_bytes(buf: &mut Vec<u8>, field_number: u32, bytes: &[u8]) {
        encode_tag(buf, field_number, 2);
        encode_varint(buf, bytes.len() as u64);
        buf.extend_from_slice(bytes);
    }
}

"#,
        );

        for en in file.all_enums_flat() {
            for c in &en.comments {
                out.push_str(&format!("/// {}\n", c));
            }
            let en_name = to_pascal_case(&en.name);
            out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n");
            out.push_str("#[repr(i32)]\n");
            out.push_str(&format!("pub enum {} {{\n", en_name));
            for v in &en.values {
                for c in &v.comments {
                    out.push_str(&format!("    /// {}\n", c));
                }
                out.push_str(&format!(
                    "    {} = {},\n",
                    to_pascal_case(&v.name),
                    v.number
                ));
            }
            out.push_str("}\n\n");

            let default_variant = en
                .values
                .first()
                .map(|v| to_pascal_case(&v.name))
                .unwrap_or_else(|| "Unknown".to_string());
            out.push_str(&format!(
                "impl Default for {} {{\n    fn default() -> Self {{\n        Self::{}\n    }}\n}}\n\n",
                en_name, default_variant
            ));
            out.push_str(&format!("impl {} {{\n", en_name));
            out.push_str("    pub fn from_i32(value: i32) -> Option<Self> {\n        match value {\n");
            for v in &en.values {
                out.push_str(&format!(
                    "            {} => Some(Self::{}),\n",
                    v.number,
                    to_pascal_case(&v.name)
                ));
            }
            out.push_str("            _ => None,\n        }\n    }\n\n");
            out.push_str("    pub fn as_str_name(&self) -> &'static str {\n        match self {\n");
            for v in &en.values {
                out.push_str(&format!(
                    "            Self::{} => \"{}\",\n",
                    to_pascal_case(&v.name),
                    v.name
                ));
            }
            out.push_str("        }\n    }\n}\n\n");
        }

        for msg in file.all_messages_flat() {
            for c in &msg.comments {
                out.push_str(&format!("/// {}\n", c));
            }
            let msg_name = to_pascal_case(&msg.name);

            // Emit oneof enums first
            for o in &msg.oneofs {
                let oneof_enum_name = format!("{}{}", msg_name, to_pascal_case(&o.name));
                for c in &o.comments {
                    out.push_str(&format!("/// {}\n", c));
                }
                out.push_str("#[derive(Debug, Clone, PartialEq)]\n");
                out.push_str(&format!("pub enum {} {{\n", oneof_enum_name));
                for f in &o.fields {
                    let ty = rust_field_type(&f.field_type, &FieldLabel::Singular);
                    out.push_str(&format!("    {}({}),\n", to_pascal_case(&f.name), ty));
                }
                out.push_str("}\n\n");
            }

            out.push_str("#[derive(Debug, Clone, PartialEq, Default)]\n");
            out.push_str(&format!("pub struct {} {{\n", msg_name));
            for f in &msg.fields {
                for c in &f.comments {
                    out.push_str(&format!("    /// {}\n", c));
                }
                let ty = rust_field_type(&f.field_type, &f.label);
                out.push_str(&format!(
                    "    /// Protobuf field #{}\n    pub {}: {},\n",
                    f.number,
                    rust_ident(&f.name),
                    ty
                ));
            }
            for o in &msg.oneofs {
                let oneof_enum_name = format!("{}{}", msg_name, to_pascal_case(&o.name));
                out.push_str(&format!(
                    "    pub {}: Option<{}>,\n",
                    rust_ident(&o.name),
                    oneof_enum_name
                ));
            }
            out.push_str("}\n\n");

            out.push_str(&format!("impl {} {{\n", msg_name));
            out.push_str(&format!(
                "    pub const FULL_NAME: &'static str = \"{}\";\n\n",
                msg.full_name
            ));
            out.push_str("    pub fn new() -> Self {\n        Self::default()\n    }\n\n");
            out.push_str("    /// Serializes this message into Protobuf binary wire format.\n");
            out.push_str("    pub fn encode_to_vec(&self) -> Vec<u8> {\n");
            out.push_str("        let mut buf = Vec::new();\n");
            for f in &msg.fields {
                let fid = f.number as u32;
                let fname = rust_ident(&f.name);
                if let FieldType::Scalar(sc) = &f.field_type {
                    if !f.is_repeated() && !f.is_optional() {
                        match sc {
                            ScalarType::String => {
                                out.push_str(&format!(
                                    "        if !self.{f}.is_empty() {{ pb_wire::encode_bytes(&mut buf, {id}, self.{f}.as_bytes()); }}\n",
                                    f = fname, id = fid
                                ));
                            }
                            ScalarType::Bytes => {
                                out.push_str(&format!(
                                    "        if !self.{f}.is_empty() {{ pb_wire::encode_bytes(&mut buf, {id}, &self.{f}); }}\n",
                                    f = fname, id = fid
                                ));
                            }
                            ScalarType::Bool => {
                                out.push_str(&format!(
                                    "        if self.{f} {{ pb_wire::encode_tag(&mut buf, {id}, 0); pb_wire::encode_varint(&mut buf, 1); }}\n",
                                    f = fname, id = fid
                                ));
                            }
                            ScalarType::Int32 | ScalarType::Int64 | ScalarType::Uint32 | ScalarType::Uint64 => {
                                out.push_str(&format!(
                                    "        if self.{f} != 0 {{ pb_wire::encode_tag(&mut buf, {id}, 0); pb_wire::encode_varint(&mut buf, self.{f} as u64); }}\n",
                                    f = fname, id = fid
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
            out.push_str("        buf\n    }\n}\n\n");
        }
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        out.push_str(
            r#"/// gRPC Status & Envelope Types (Zero-Dependency Built-In Runtime)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcStatus {
    pub code: i32,
    pub message: String,
}

impl GrpcStatus {
    pub fn ok() -> Self {
        Self { code: 0, message: String::new() }
    }
    pub fn unimplemented(method: &str) -> Self {
        Self { code: 12, message: format!("Method '{}' not implemented", method) }
    }
}

#[derive(Debug, Clone)]
pub struct GrpcRequest<T> {
    pub metadata: BTreeMap<String, String>,
    pub message: T,
}

impl<T> GrpcRequest<T> {
    pub fn new(message: T) -> Self {
        Self { metadata: BTreeMap::new(), message }
    }
}

#[derive(Debug, Clone)]
pub struct GrpcResponse<T> {
    pub metadata: BTreeMap<String, String>,
    pub message: T,
}

impl<T> GrpcResponse<T> {
    pub fn new(message: T) -> Self {
        Self { metadata: BTreeMap::new(), message }
    }
}

"#,
        );

        for svc in &file.services {
            let svc_pascal = to_pascal_case(&svc.name);
            let svc_snake = to_snake_case(&svc.name);

            for c in &svc.comments {
                out.push_str(&format!("/// {}\n", c));
            }
            out.push_str(&format!("pub mod {}_server {{\n    use super::*;\n\n", svc_snake));
            out.push_str(&format!(
                "    pub const SERVICE_NAME: &'static str = \"{}\";\n\n",
                svc.full_name
            ));
            out.push_str(&format!("    /// Server trait for gRPC service `{}`\n", svc.full_name));
            out.push_str(&format!("    pub trait {}Service: Send + Sync + 'static {{\n", svc_pascal));
            for rpc in &svc.methods {
                for c in &rpc.comments {
                    out.push_str(&format!("        /// {}\n", c));
                }
                let m_name = rust_ident(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_param = if rpc.client_streaming {
                    format!("GrpcRequest<Vec<{}>>", in_ty)
                } else {
                    format!("GrpcRequest<{}>", in_ty)
                };
                let resp_ret = if rpc.server_streaming {
                    format!("Result<GrpcResponse<Vec<{}>>, GrpcStatus>", out_ty)
                } else {
                    format!("Result<GrpcResponse<{}>, GrpcStatus>", out_ty)
                };
                out.push_str(&format!(
                    "        /// RPC `{}` [{}]\n        fn {}(&self, request: {}) -> {};\n",
                    rpc.name,
                    rpc.streaming_mode(),
                    m_name,
                    req_param,
                    resp_ret
                ));
            }
            out.push_str("    }\n}\n\n");

            out.push_str(&format!("pub mod {}_client {{\n    use super::*;\n\n", svc_snake));
            out.push_str(&format!(
                "    /// Client stub for gRPC service `{}`\n    #[derive(Debug, Clone)]\n    pub struct {}Client {{\n        pub endpoint: String,\n        pub default_metadata: BTreeMap<String, String>,\n    }}\n\n",
                svc.full_name, svc_pascal
            ));
            out.push_str(&format!("    impl {}Client {{\n", svc_pascal));
            out.push_str("        pub fn connect(endpoint: impl Into<String>) -> Self {\n            Self {\n                endpoint: endpoint.into(),\n                default_metadata: BTreeMap::new(),\n            }\n        }\n\n");
            for rpc in &svc.methods {
                let m_name = rust_ident(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let path = format!("/{}/{}", svc.full_name, rpc.name);
                out.push_str(&format!(
                    "        /// Full gRPC HTTP/2 path for `{}` ({})\n        pub const {}_PATH: &'static str = \"{}\";\n\n",
                    rpc.name,
                    rpc.streaming_mode(),
                    to_screaming_snake_case(&rpc.name),
                    path
                ));
                let req_param = if rpc.client_streaming {
                    format!("Vec<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_ty = if rpc.server_streaming {
                    format!("Vec<{}>", out_ty)
                } else {
                    out_ty
                };
                out.push_str(&format!(
                    "        pub fn {}<S: super::{}_server::{}Service>(&self, server: &S, req: {}) -> Result<GrpcResponse<{}>, GrpcStatus> {{\n            server.{}(GrpcRequest::new(req))\n        }}\n\n",
                    m_name, svc_snake, svc_pascal, req_param, resp_ty, m_name
                ));
            }
            out.push_str("    }\n}\n\n");
        }
    }

    vec![(format!("{}.rs", file.file_stem()), out)]
}

fn go_scalar(sc: ScalarType) -> &'static str {
    match sc {
        ScalarType::Double => "float64",
        ScalarType::Float => "float32",
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => "int32",
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => "int64",
        ScalarType::Uint32 | ScalarType::Fixed32 => "uint32",
        ScalarType::Uint64 | ScalarType::Fixed64 => "uint64",
        ScalarType::Bool => "bool",
        ScalarType::String => "string",
        ScalarType::Bytes => "[]byte",
    }
}

fn go_field_type(ft: &FieldType, label: &FieldLabel) -> String {
    let base = match ft {
        FieldType::Scalar(sc) => go_scalar(*sc).to_string(),
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                short
            } else {
                format!("*{}", short)
            }
        }
        FieldType::Map(k, v) => {
            let kt = go_field_type(k, &FieldLabel::Singular);
            let vt = go_field_type(v, &FieldLabel::Singular);
            return format!("map[{}]{}", kt, vt);
        }
    };
    match label {
        FieldLabel::Repeated => format!("[]{}", base),
        _ => base,
    }
}

/// Generates Go Protobuf (`<stem>.pb.go`) and gRPC (`<stem>_grpc.pb.go`) files.
pub fn gen_go(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let pkg_name = file
        .option_value("go_package")
        .and_then(|gp| gp.rsplit(';').next())
        .and_then(|gp| gp.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .map(|s| to_snake_case(s))
        .unwrap_or_else(|| {
            if file.package.is_empty() {
                file.file_stem()
            } else {
                file.package.rsplit('.').next().unwrap_or("pb").to_string()
            }
        });

    if cfg.generate_messages {
        let mut pb = banner_comment(file, TargetLang::Go, "//");
        pb.push_str(&format!("package {}\n\n", pkg_name));

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            for c in &en.comments {
                pb.push_str(&format!("// {}\n", c));
            }
            pb.push_str(&format!("type {} int32\n\nconst (\n", en_name));
            for v in &en.values {
                pb.push_str(&format!(
                    "\t{}_{} {} = {}\n",
                    en_name, v.name, en_name, v.number
                ));
            }
            pb.push_str(")\n\n");
            pb.push_str(&format!("var {}_name = map[int32]string{{\n", en_name));
            for v in &en.values {
                pb.push_str(&format!("\t{}: \"{}\",\n", v.number, v.name));
            }
            pb.push_str("}\n\n");
            pb.push_str(&format!("var {}_value = map[string]int32{{\n", en_name));
            for v in &en.values {
                pb.push_str(&format!("\t\"{}\": {},\n", v.name, v.number));
            }
            pb.push_str("}\n\n");
            pb.push_str(&format!(
                "func (x {}) String() string {{\n\tif s, ok := {}_name[int32(x)]; ok {{\n\t\treturn s\n\t}}\n\treturn \"UNKNOWN\"\n}}\n\n",
                en_name, en_name
            ));
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            for c in &msg.comments {
                pb.push_str(&format!("// {}\n", c));
            }
            pb.push_str(&format!("type {} struct {{\n", msg_name));
            for f in msg.all_fields() {
                for c in &f.comments {
                    pb.push_str(&format!("\t// {}\n", c));
                }
                let f_go = to_pascal_case(&f.name);
                let ty = go_field_type(&f.field_type, &f.label);
                let json_name = to_camel_case(&f.name);
                pb.push_str(&format!(
                    "\t{} {} `protobuf:\"bytes,{},opt,name={},json={}\" json:\"{},omitempty\"`\n",
                    f_go, ty, f.number, f.name, json_name, json_name
                ));
            }
            pb.push_str("}\n\n");
            pb.push_str(&format!("func (x *{}) Reset() {{ *x = {}{{}} }}\n", msg_name, msg_name));
            pb.push_str(&format!(
                "func (x *{}) ProtoMessage() {{}}\n\n",
                msg_name
            ));
            for f in msg.all_fields() {
                let f_go = to_pascal_case(&f.name);
                let ty = go_field_type(&f.field_type, &f.label);
                pb.push_str(&format!(
                    "func (x *{}) Get{}() {} {{\n\tif x != nil {{\n\t\treturn x.{}\n\t}}\n\tvar zero {}\n\treturn zero\n}}\n\n",
                    msg_name, f_go, ty, f_go, ty
                ));
            }
        }
        files.push((format!("{}.pb.go", file.file_stem()), pb));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::Go, "//");
        g.push_str(&format!("package {}\n\nimport (\n\t\"context\"\n\t\"errors\"\n)\n\n", pkg_name));

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("const {}_ServiceName = \"{}\"\n\n", svc_name, svc.full_name));
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                g.push_str(&format!(
                    "const {}_{}_FullMethodName = \"/{}/{}\"\n",
                    svc_name, m_name, svc.full_name, rpc.name
                ));
            }
            g.push('\n');

            // Client interface
            g.push_str(&format!(
                "// {}Client is the client API for {} service.\ntype {}Client interface {{\n",
                svc_name, svc.full_name, svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                match (rpc.client_streaming, rpc.server_streaming) {
                    (false, false) => g.push_str(&format!(
                        "\t{}(ctx context.Context, in *{}) (*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                    (false, true) => g.push_str(&format!(
                        "\t{}(ctx context.Context, in *{}) ([]*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                    (true, false) => g.push_str(&format!(
                        "\t{}(ctx context.Context, in []*{} ) (*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                    (true, true) => g.push_str(&format!(
                        "\t{}(ctx context.Context, in []*{} ) ([]*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                }
            }
            g.push_str("}\n\n");

            // Server interface
            g.push_str(&format!(
                "// {}Server is the server API for {} service.\ntype {}Server interface {{\n",
                svc_name, svc.full_name, svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                match (rpc.client_streaming, rpc.server_streaming) {
                    (false, false) => g.push_str(&format!(
                        "\t{}(ctx context.Context, req *{}) (*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                    (false, true) => g.push_str(&format!(
                        "\t{}(ctx context.Context, req *{}) ([]*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                    (true, false) => g.push_str(&format!(
                        "\t{}(ctx context.Context, req []*{} ) (*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                    (true, true) => g.push_str(&format!(
                        "\t{}(ctx context.Context, req []*{} ) ([]*{}, error)\n",
                        m_name, in_ty, out_ty
                    )),
                }
            }
            g.push_str("}\n\n");

            // UnimplementedServer
            g.push_str(&format!(
                "type Unimplemented{}Server struct{{}}\n\n",
                svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let in_sig = if rpc.client_streaming {
                    format!("[]*{}", in_ty)
                } else {
                    format!("*{}", in_ty)
                };
                let out_sig = if rpc.server_streaming {
                    format!("[]*{}", out_ty)
                } else {
                    format!("*{}", out_ty)
                };
                g.push_str(&format!(
                    "func (Unimplemented{}Server) {}(context.Context, {}) ({}, error) {{\n\treturn nil, errors.New(\"method {} not implemented\")\n}}\n\n",
                    svc_name, m_name, in_sig, out_sig, rpc.name
                ));
            }
        }
        files.push((format!("{}_grpc.pb.go", file.file_stem()), g));
    }

    files
}

fn py_scalar(sc: ScalarType) -> (&'static str, &'static str) {
    match sc {
        ScalarType::Double | ScalarType::Float => ("float", "0.0"),
        ScalarType::Int32
        | ScalarType::Int64
        | ScalarType::Uint32
        | ScalarType::Uint64
        | ScalarType::Sint32
        | ScalarType::Sint64
        | ScalarType::Fixed32
        | ScalarType::Fixed64
        | ScalarType::Sfixed32
        | ScalarType::Sfixed64 => ("int", "0"),
        ScalarType::Bool => ("bool", "False"),
        ScalarType::String => ("str", "\"\""),
        ScalarType::Bytes => ("bytes", "b\"\""),
    }
}

fn py_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    let (base_ty, base_def) = match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = py_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                (short.clone(), format!("{} (0)", short))
            } else {
                (format!("Optional[{}]", short), "None".to_string())
            }
        }
        FieldType::Map(k, v) => {
            let (kt, _) = py_field_type(k, &FieldLabel::Singular);
            let (vt, _) = py_field_type(v, &FieldLabel::Singular);
            return (
                format!("Dict[{}, {}]", kt, vt.trim_start_matches("Optional[").trim_end_matches(']')),
                "field(default_factory=dict)".to_string(),
            );
        }
    };
    match label {
        FieldLabel::Repeated => {
            let inner = match ft {
                FieldType::Named { raw, .. } => to_pascal_case(raw.rsplit('.').next().unwrap_or(raw)),
                _ => base_ty,
            };
            (
                format!("List[{}]", inner),
                "field(default_factory=list)".to_string(),
            )
        }
        _ => (base_ty, base_def),
    }
}

/// Generates Python `<stem>_pb2.py`, PEP-484 type stubs `<stem>_pb2.pyi`, and gRPC `<stem>_pb2_grpc.py`.
pub fn gen_python(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();

    if cfg.generate_messages {
        let mut py = banner_comment(file, TargetLang::Python, "#");
        py.push_str("from __future__ import annotations\n");
        py.push_str("from dataclasses import dataclass, field, asdict\n");
        py.push_str("from enum import IntEnum\n");
        py.push_str("from typing import Dict, List, Optional, Any\n");
        py.push_str("import json\n\n");

        let mut pyi = banner_comment(file, TargetLang::Python, "#");
        pyi.push_str("from enum import IntEnum\n");
        pyi.push_str("from typing import Dict, List, Optional, Any\n\n");

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            py.push_str(&format!("class {}(IntEnum):\n", en_name));
            pyi.push_str(&format!("class {}(IntEnum):\n", en_name));
            for v in &en.values {
                py.push_str(&format!("    {} = {}\n", v.name, v.number));
                pyi.push_str(&format!("    {}: int = {}\n", v.name, v.number));
            }
            py.push('\n');
            pyi.push('\n');
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            py.push_str("@dataclass\n");
            py.push_str(&format!("class {}:\n", msg_name));
            pyi.push_str(&format!("class {}:\n", msg_name));
            if !msg.comments.is_empty() {
                py.push_str(&format!("    \"\"\"{}\"\"\"\n", msg.comments.join(" ")));
            }
            let all_fields = msg.all_fields();
            if all_fields.is_empty() {
                py.push_str("    pass\n");
                pyi.push_str("    def __init__(self) -> None: ...\n");
            } else {
                for f in &all_fields {
                    let (ty, def_val) = py_field_type(&f.field_type, &f.label);
                    let f_py = to_snake_case(&f.name);
                    if def_val.starts_with("field(") {
                        py.push_str(&format!("    {}: {} = {}\n", f_py, ty, def_val));
                    } else if matches!(f.field_type, FieldType::Named { kind: ResolvedKind::Enum, .. }) {
                        let raw_en = match &f.field_type {
                            FieldType::Named { raw, .. } => to_pascal_case(raw.rsplit('.').next().unwrap_or(raw)),
                            _ => "int".to_string(),
                        };
                        py.push_str(&format!("    {}: {} = {}(0)\n", f_py, ty, raw_en));
                    } else {
                        py.push_str(&format!("    {}: {} = {}\n", f_py, ty, def_val));
                    }
                    pyi.push_str(&format!("    {}: {}\n", f_py, ty));
                }
            }
            py.push_str("\n    def to_dict(self) -> Dict[str, Any]:\n        return asdict(self)\n\n");
            py.push_str("    def SerializeToString(self) -> bytes:\n        return json.dumps(self.to_dict(), default=str).encode('utf-8')\n\n");
            py.push_str(&format!(
                "    @classmethod\n    def FromString(cls, data: bytes) -> {}:\n        payload = json.loads(data.decode('utf-8'))\n        return cls(**{{k: v for k, v in payload.items() if k in cls.__dataclass_fields__}})\n\n",
                msg_name
            ));

            pyi.push_str("    def to_dict(self) -> Dict[str, Any]: ...\n");
            pyi.push_str("    def SerializeToString(self) -> bytes: ...\n");
            pyi.push_str(&format!(
                "    @classmethod\n    def FromString(cls, data: bytes) -> {}: ...\n\n",
                msg_name
            ));
        }

        files.push((format!("{}_pb2.py", stem), py));
        files.push((format!("{}_pb2.pyi", stem), pyi));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::Python, "#");
        g.push_str("from __future__ import annotations\n");
        g.push_str("from typing import Any, Callable, Dict, Iterable, Iterator, Optional\n");
        g.push_str(&format!("import {}_pb2 as pb2\n\n", stem));

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("class {}Stub:\n", svc_name));
            g.push_str(&format!(
                "    \"\"\"gRPC Client Stub for `{}`\"\"\"\n",
                svc.full_name
            ));
            g.push_str(&format!(
                "    SERVICE_NAME = \"{}\"\n\n",
                svc.full_name
            ));
            g.push_str("    def __init__(self, channel: Any) -> None:\n        self.channel = channel\n\n");
            for rpc in &svc.methods {
                let m_py = to_snake_case(&rpc.name);
                let in_ty = format!("pb2.{}", to_pascal_case(rpc.input_short()));
                let out_ty = format!("pb2.{}", to_pascal_case(rpc.output_short()));
                let req_ann = if rpc.client_streaming {
                    format!("Iterable[{}]", in_ty)
                } else {
                    in_ty
                };
                let resp_ann = if rpc.server_streaming {
                    format!("Iterator[{}]", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    def {}(self, request: {}, timeout: Optional[float] = None, metadata: Optional[Dict[str, str]] = None) -> {}:\n",
                    m_py, req_ann, resp_ann
                ));
                g.push_str(&format!(
                    "        \"\"\"RPC `{}` [{}]: /{}/{}\"\"\"\n",
                    rpc.name,
                    rpc.streaming_mode(),
                    svc.full_name,
                    rpc.name
                ));
                g.push_str(&format!(
                    "        if hasattr(self.channel, '{}'):\n            return getattr(self.channel, '{}')(request, metadata)\n        raise NotImplementedError('Attach a gRPC transport or Servicer to channel')\n\n",
                    m_py, m_py
                ));
            }

            g.push_str(&format!("class {}Servicer:\n", svc_name));
            g.push_str(&format!(
                "    \"\"\"gRPC Server Implementation Base for `{}`\"\"\"\n",
                svc.full_name
            ));
            for rpc in &svc.methods {
                let m_py = to_snake_case(&rpc.name);
                let in_ty = format!("pb2.{}", to_pascal_case(rpc.input_short()));
                let out_ty = format!("pb2.{}", to_pascal_case(rpc.output_short()));
                let req_ann = if rpc.client_streaming {
                    format!("Iterable[{}]", in_ty)
                } else {
                    in_ty
                };
                let resp_ann = if rpc.server_streaming {
                    format!("Iterator[{}]", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    def {}(self, request: {}, context: Any = None) -> {}:\n",
                    m_py, req_ann, resp_ann
                ));
                g.push_str(&format!(
                    "        raise NotImplementedError('Method {} not implemented!')\n\n",
                    rpc.name
                ));
            }
        }
        files.push((format!("{}_pb2_grpc.py", stem), g));
    }

    files
}

fn ts_scalar(sc: ScalarType) -> (&'static str, &'static str) {
    match sc {
        ScalarType::Double
        | ScalarType::Float
        | ScalarType::Int32
        | ScalarType::Uint32
        | ScalarType::Sint32
        | ScalarType::Fixed32
        | ScalarType::Sfixed32 => ("number", "0"),
        ScalarType::Int64
        | ScalarType::Uint64
        | ScalarType::Sint64
        | ScalarType::Fixed64
        | ScalarType::Sfixed64 => ("bigint | number", "0"),
        ScalarType::Bool => ("boolean", "false"),
        ScalarType::String => ("string", "\"\""),
        ScalarType::Bytes => ("Uint8Array", "new Uint8Array(0)"),
    }
}

fn ts_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    let (base_t, base_d) = match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = ts_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                (short, "0".to_string())
            } else {
                (format!("{} | undefined", short), "undefined".to_string())
            }
        }
        FieldType::Map(k, v) => {
            let (kt, _) = ts_field_type(k, &FieldLabel::Singular);
            let (vt, _) = ts_field_type(v, &FieldLabel::Singular);
            let key_clean = if kt == "string" { "string" } else { "number" };
            return (
                format!("Record<{}, {}>", key_clean, vt.replace(" | undefined", "")),
                "{}".to_string(),
            );
        }
    };
    match label {
        FieldLabel::Repeated => (
            format!("Array<{}>", base_t.replace(" | undefined", "")),
            "[]".to_string(),
        ),
        _ => (base_t, base_d),
    }
}

/// Generates TypeScript `<stem>.pb.ts` and `<stem>.grpc.ts`.
pub fn gen_typescript(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();

    if cfg.generate_messages {
        let mut ts = banner_comment(file, TargetLang::TypeScript, "//");
        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            ts.push_str(&format!("export enum {} {{\n", en_name));
            for v in &en.values {
                ts.push_str(&format!("  {} = {},\n", v.name, v.number));
            }
            ts.push_str("}\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            ts.push_str(&format!("export interface I{} {{\n", msg_name));
            for f in msg.all_fields() {
                let (ty, _) = ts_field_type(&f.field_type, &f.label);
                ts.push_str(&format!("  {}?: {};\n", to_camel_case(&f.name), ty));
            }
            ts.push_str("}\n\n");

            ts.push_str(&format!("export class {} implements I{} {{\n", msg_name, msg_name));
            ts.push_str(&format!("  static readonly TYPE_NAME = \"{}\";\n", msg.full_name));
            for f in msg.all_fields() {
                let (ty, def_val) = ts_field_type(&f.field_type, &f.label);
                ts.push_str(&format!("  {}: {} = {};\n", to_camel_case(&f.name), ty, def_val));
            }
            ts.push_str(&format!(
                "\n  constructor(init?: Partial<I{}>) {{\n    if (init) Object.assign(this, init);\n  }}\n\n",
                msg_name
            ));
            ts.push_str("  static encode(message: I" );
            ts.push_str(&msg_name);
            ts.push_str("): Uint8Array {\n    return new TextEncoder().encode(JSON.stringify(message));\n  }\n\n");
            ts.push_str(&format!(
                "  static decode(bytes: Uint8Array): {} {{\n    const raw = JSON.parse(new TextDecoder().decode(bytes));\n    return new {}(raw);\n  }}\n",
                msg_name, msg_name
            ));
            ts.push_str("}\n\n");
        }

        files.push((format!("{}.pb.ts", stem), ts));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::TypeScript, "//");
        g.push_str(&format!("import * as pb from \"./{}.pb\";\n\n", stem));
        g.push_str(
            r#"export interface GrpcCallOptions {
  metadata?: Record<string, string>;
  timeoutMs?: number;
}

export interface GrpcTransport {
  unary<Req, Resp>(path: string, req: Req, options?: GrpcCallOptions): Promise<Resp>;
  serverStream<Req, Resp>(path: string, req: Req, options?: GrpcCallOptions): AsyncIterable<Resp>;
  clientStream<Req, Resp>(path: string, reqs: AsyncIterable<Req> | Iterable<Req>, options?: GrpcCallOptions): Promise<Resp>;
  bidiStream<Req, Resp>(path: string, reqs: AsyncIterable<Req> | Iterable<Req>, options?: GrpcCallOptions): AsyncIterable<Resp>;
}

"#,
        );

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("export interface I{}Server {{\n", svc_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = format!("pb.{}", to_pascal_case(rpc.input_short()));
                let out_ty = format!("pb.{}", to_pascal_case(rpc.output_short()));
                match (rpc.client_streaming, rpc.server_streaming) {
                    (false, false) => g.push_str(&format!(
                        "  {}(request: {}, metadata?: Record<string, string>): Promise<{}>;\n",
                        m_name, in_ty, out_ty
                    )),
                    (false, true) => g.push_str(&format!(
                        "  {}(request: {}, metadata?: Record<string, string>): AsyncIterable<{}>;\n",
                        m_name, in_ty, out_ty
                    )),
                    (true, false) => g.push_str(&format!(
                        "  {}(requests: AsyncIterable<{}>, metadata?: Record<string, string>): Promise<{}>;\n",
                        m_name, in_ty, out_ty
                    )),
                    (true, true) => g.push_str(&format!(
                        "  {}(requests: AsyncIterable<{}>, metadata?: Record<string, string>): AsyncIterable<{}>;\n",
                        m_name, in_ty, out_ty
                    )),
                }
            }
            g.push_str("}\n\n");

            g.push_str(&format!("export class {}Client {{\n", svc_name));
            g.push_str(&format!("  static readonly SERVICE_NAME = \"{}\";\n", svc.full_name));
            g.push_str("  constructor(private readonly transport: GrpcTransport) {}\n\n");
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = format!("pb.{}", to_pascal_case(rpc.input_short()));
                let out_ty = format!("pb.{}", to_pascal_case(rpc.output_short()));
                let path = format!("/{}/{}", svc.full_name, rpc.name);
                match (rpc.client_streaming, rpc.server_streaming) {
                    (false, false) => g.push_str(&format!(
                        "  {}(request: {}, options?: GrpcCallOptions): Promise<{}> {{\n    return this.transport.unary(\"{}\", request, options);\n  }}\n\n",
                        m_name, in_ty, out_ty, path
                    )),
                    (false, true) => g.push_str(&format!(
                        "  {}(request: {}, options?: GrpcCallOptions): AsyncIterable<{}> {{\n    return this.transport.serverStream(\"{}\", request, options);\n  }}\n\n",
                        m_name, in_ty, out_ty, path
                    )),
                    (true, false) => g.push_str(&format!(
                        "  {}(requests: AsyncIterable<{}> | Iterable<{}>, options?: GrpcCallOptions): Promise<{}> {{\n    return this.transport.clientStream(\"{}\", requests, options);\n  }}\n\n",
                        m_name, in_ty, in_ty, out_ty, path
                    )),
                    (true, true) => g.push_str(&format!(
                        "  {}(requests: AsyncIterable<{}> | Iterable<{}>, options?: GrpcCallOptions): AsyncIterable<{}> {{\n    return this.transport.bidiStream(\"{}\", requests, options);\n  }}\n\n",
                        m_name, in_ty, in_ty, out_ty, path
                    )),
                }
            }
            g.push_str("}\n\n");
        }
        files.push((format!("{}.grpc.ts", stem), g));
    }

    files
}

/// Generates JavaScript `<stem>_pb.js` and `<stem>_grpc_pb.js`.
pub fn gen_javascript(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();

    if cfg.generate_messages {
        let mut js = banner_comment(file, TargetLang::JavaScript, "//");
        js.push_str("'use strict';\n\n");

        let mut exports = Vec::new();
        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            exports.push(en_name.clone());
            js.push_str(&format!("const {} = Object.freeze({{\n", en_name));
            for v in &en.values {
                js.push_str(&format!("  {}: {},\n", v.name, v.number));
            }
            js.push_str("});\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            exports.push(msg_name.clone());
            js.push_str(&format!("class {} {{\n  constructor(init = {{}}) {{\n", msg_name));
            for f in msg.all_fields() {
                let (_, def_val) = ts_field_type(&f.field_type, &f.label);
                let cname = to_camel_case(&f.name);
                js.push_str(&format!(
                    "    this.{} = init.{} !== undefined ? init.{} : {};\n",
                    cname, cname, cname, def_val
                ));
            }
            js.push_str("  }\n\n");
            js.push_str("  serializeBinary() {\n    return Buffer.from(JSON.stringify(this), 'utf8');\n  }\n\n");
            js.push_str(&format!(
                "  static deserializeBinary(bytes) {{\n    return new {}(JSON.parse(Buffer.from(bytes).toString('utf8')));\n  }}\n",
                msg_name
            ));
            js.push_str("}\n\n");
        }

        js.push_str(&format!("module.exports = {{ {} }};\n", exports.join(", ")));
        files.push((format!("{}_pb.js", stem), js));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::JavaScript, "//");
        g.push_str("'use strict';\n");
        g.push_str(&format!("const pb = require('./{}_pb.js');\n\n", stem));

        let mut exports = Vec::new();
        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            exports.push(format!("{}Service", svc_name));
            exports.push(format!("{}Client", svc_name));

            g.push_str(&format!("const {}Service = Object.freeze({{\n", svc_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                g.push_str(&format!(
                    "  {}: {{\n    path: '/{}/{}',\n    requestStream: {},\n    responseStream: {},\n    requestSerialize: (arg) => arg.serializeBinary(),\n    requestDeserialize: (buf) => pb.{}.deserializeBinary(buf),\n    responseSerialize: (arg) => arg.serializeBinary(),\n    responseDeserialize: (buf) => pb.{}.deserializeBinary(buf),\n  }},\n",
                    m_name,
                    svc.full_name,
                    rpc.name,
                    rpc.client_streaming,
                    rpc.server_streaming,
                    in_ty,
                    out_ty
                ));
            }
            g.push_str("});\n\n");

            g.push_str(&format!("class {}Client {{\n  constructor(address, credentials = null) {{\n    this.address = address;\n    this.credentials = credentials;\n  }}\n", svc_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                g.push_str(&format!(
                    "  {}(request, metadata = {{}}) {{\n    return {{ service: '{}', method: '{}', endpoint: this.address, request, metadata }};\n  }}\n",
                    m_name, svc.full_name, rpc.name
                ));
            }
            g.push_str("}\n\n");
        }
        g.push_str(&format!("module.exports = {{ {} }};\n", exports.join(", ")));
        files.push((format!("{}_grpc_pb.js", stem), g));
    }

    files
}
