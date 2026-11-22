use crate::ast::*;
use crate::codegen::*;

fn cpp_scalar(sc: ScalarType) -> &'static str {
    match sc {
        ScalarType::Double => "double",
        ScalarType::Float => "float",
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => "int32_t",
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => "int64_t",
        ScalarType::Uint32 | ScalarType::Fixed32 => "uint32_t",
        ScalarType::Uint64 | ScalarType::Fixed64 => "uint64_t",
        ScalarType::Bool => "bool",
        ScalarType::String | ScalarType::Bytes => "std::string",
    }
}

fn cpp_field_type(ft: &FieldType, label: &FieldLabel) -> String {
    let base = match ft {
        FieldType::Scalar(sc) => cpp_scalar(*sc).to_string(),
        FieldType::Named { raw, .. } => to_pascal_case(raw.rsplit('.').next().unwrap_or(raw)),
        FieldType::Map(k, v) => {
            let kt = cpp_field_type(k, &FieldLabel::Singular);
            let vt = cpp_field_type(v, &FieldLabel::Singular);
            return format!("std::map<{}, {}>", kt, vt);
        }
    };
    match label {
        FieldLabel::Repeated => format!("std::vector<{}>", base),
        _ => base,
    }
}

/// Generates C++ `<stem>.pb.h`, `<stem>.pb.cc`, `<stem>.grpc.pb.h`, `<stem>.grpc.pb.cc`.
pub fn gen_cpp(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();
    let ns_parts: Vec<&str> = if file.package.is_empty() {
        vec!["protobuilder"]
    } else {
        file.package.split('.').collect()
    };
    let ns_open: String = ns_parts
        .iter()
        .map(|p| format!("namespace {} {{\n", p))
        .collect();
    let ns_close: String = ns_parts
        .iter()
        .rev()
        .map(|p| format!("}} // namespace {}\n", p))
        .collect();

    if cfg.generate_messages {
        let mut h = banner_comment(file, TargetLang::Cpp, "//");
        h.push_str("#pragma once\n\n#include <cstdint>\n#include <string>\n#include <vector>\n#include <map>\n\n");
        h.push_str(&ns_open);
        h.push('\n');

        let mut cc = banner_comment(file, TargetLang::Cpp, "//");
        cc.push_str(&format!("#include \"{}.pb.h\"\n#include <sstream>\n\n", stem));
        cc.push_str(&ns_open);
        cc.push('\n');

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            h.push_str(&format!("enum class {} : int32_t {{\n", en_name));
            for v in &en.values {
                h.push_str(&format!("  {} = {},\n", v.name, v.number));
            }
            h.push_str("};\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            h.push_str(&format!("class {} {{\npublic:\n  {}() = default;\n", msg_name, msg_name));
            for f in msg.all_fields() {
                let fname = to_snake_case(&f.name);
                let ty = cpp_field_type(&f.field_type, &f.label);
                h.push_str(&format!("  const {}& {}() const {{ return {}_{}; }}\n", ty, fname, fname, "val"));
                h.push_str(&format!("  void set_{}(const {}& v) {{ {}_{} = v; }}\n", fname, ty, fname, "val"));
                h.push_str(&format!("  {}* mutable_{}() {{ return &{}_{}; }}\n", ty, fname, fname, "val"));
            }
            h.push_str("\n  std::string SerializeAsString() const;\n");
            h.push_str("  bool ParseFromString(const std::string& data);\n\nprivate:\n");
            for f in msg.all_fields() {
                let fname = to_snake_case(&f.name);
                let ty = cpp_field_type(&f.field_type, &f.label);
                let init = match (&f.field_type, &f.label) {
                    (_, FieldLabel::Repeated) | (FieldType::Map(_, _), _) => "",
                    (FieldType::Scalar(ScalarType::Bool), _) => " = false",
                    (FieldType::Scalar(ScalarType::String | ScalarType::Bytes), _) => "",
                    (FieldType::Scalar(_), _) => " = 0",
                    _ => "",
                };
                h.push_str(&format!("  {} {}_{}{};\n", ty, fname, "val", init));
            }
            h.push_str("};\n\n");

            cc.push_str(&format!(
                "std::string {}::SerializeAsString() const {{\n  return \"{}\";\n}}\n\n",
                msg_name, msg.full_name
            ));
            cc.push_str(&format!(
                "bool {}::ParseFromString(const std::string& data) {{\n  return !data.empty();\n}}\n\n",
                msg_name
            ));
        }

        h.push_str(&ns_close);
        cc.push_str(&ns_close);
        files.push((format!("{}.pb.h", stem), h));
        files.push((format!("{}.pb.cc", stem), cc));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut gh = banner_comment(file, TargetLang::Cpp, "//");
        gh.push_str(&format!("#pragma once\n\n#include \"{}.pb.h\"\n#include <memory>\n#include <string>\n#include <vector>\n\n", stem));
        gh.push_str(&ns_open);
        gh.push_str("\nstruct GrpcStatus {\n  int code = 0;\n  std::string error_message;\n  bool ok() const { return code == 0; }\n  static GrpcStatus OK() { return {0, \"\"}; }\n};\n\nstruct ServerContext {};\nstruct ClientContext {};\n\n");

        let mut gcc = banner_comment(file, TargetLang::Cpp, "//");
        gcc.push_str(&format!("#include \"{}.grpc.pb.h\"\n\n", stem));
        gcc.push_str(&ns_open);
        gcc.push('\n');

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            gh.push_str(&format!("class {} final {{\npublic:\n  static constexpr const char* service_full_name() {{ return \"{}\"; }}\n\n", svc_name, svc.full_name));

            // Service interface
            gh.push_str("  class Service {\n  public:\n    virtual ~Service() = default;\n");
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("const std::vector<{}>&", in_ty)
                } else {
                    format!("const {}&", in_ty)
                };
                let resp_sig = if rpc.server_streaming {
                    format!("std::vector<{}>*", out_ty)
                } else {
                    format!("{}*", out_ty)
                };
                gh.push_str(&format!(
                    "    virtual GrpcStatus {}(ServerContext* context, {} request, {} response);\n",
                    m_name, req_sig, resp_sig
                ));
                gcc.push_str(&format!(
                    "GrpcStatus {}::Service::{}(ServerContext*, {}, {}) {{\n  return {{12, \"Method {} not implemented\"}};\n}}\n\n",
                    svc_name, m_name, req_sig, resp_sig, rpc.name
                ));
            }
            gh.push_str("  };\n\n");

            // Stub class
            gh.push_str("  class Stub {\n  public:\n    explicit Stub(std::string endpoint) : endpoint_(std::move(endpoint)) {}\n");
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("const std::vector<{}>&", in_ty)
                } else {
                    format!("const {}&", in_ty)
                };
                let resp_sig = if rpc.server_streaming {
                    format!("std::vector<{}>*", out_ty)
                } else {
                    format!("{}*", out_ty)
                };
                gh.push_str(&format!(
                    "    GrpcStatus {}(ClientContext* context, {} request, {} response);\n",
                    m_name, req_sig, resp_sig
                ));
                gcc.push_str(&format!(
                    "GrpcStatus {}::Stub::{}(ClientContext*, {}, {}) {{\n  return GrpcStatus::OK();\n}}\n\n",
                    svc_name, m_name, req_sig, resp_sig
                ));
            }
            gh.push_str("  private:\n    std::string endpoint_;\n  };\n};\n\n");
        }

        gh.push_str(&ns_close);
        gcc.push_str(&ns_close);
        files.push((format!("{}.grpc.pb.h", stem), gh));
        files.push((format!("{}.grpc.pb.cc", stem), gcc));
    }

    files
}

fn java_scalar(sc: ScalarType, boxed: bool) -> &'static str {
    match (sc, boxed) {
        (ScalarType::Double, false) => "double",
        (ScalarType::Double, true) => "Double",
        (ScalarType::Float, false) => "float",
        (ScalarType::Float, true) => "Float",
        (ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 | ScalarType::Uint32 | ScalarType::Fixed32, false) => "int",
        (ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 | ScalarType::Uint32 | ScalarType::Fixed32, true) => "Integer",
        (ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 | ScalarType::Uint64 | ScalarType::Fixed64, false) => "long",
        (ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 | ScalarType::Uint64 | ScalarType::Fixed64, true) => "Long",
        (ScalarType::Bool, false) => "boolean",
        (ScalarType::Bool, true) => "Boolean",
        (ScalarType::String, _) => "String",
        (ScalarType::Bytes, _) => "byte[]",
    }
}

fn java_field_type(ft: &FieldType, label: &FieldLabel) -> String {
    match label {
        FieldLabel::Repeated => {
            let elem = match ft {
                FieldType::Scalar(sc) => java_scalar(*sc, true).to_string(),
                FieldType::Named { raw, .. } => to_pascal_case(raw.rsplit('.').next().unwrap_or(raw)),
                FieldType::Map(k, v) => return java_field_type(&FieldType::Map(k.clone(), v.clone()), &FieldLabel::Singular),
            };
            format!("java.util.List<{}>", elem)
        }
        _ => match ft {
            FieldType::Scalar(sc) => java_scalar(*sc, false).to_string(),
            FieldType::Named { raw, .. } => to_pascal_case(raw.rsplit('.').next().unwrap_or(raw)),
            FieldType::Map(k, v) => {
                let kt = match &**k {
                    FieldType::Scalar(sc) => java_scalar(*sc, true).to_string(),
                    other => java_field_type(other, &FieldLabel::Singular),
                };
                let vt = match &**v {
                    FieldType::Scalar(sc) => java_scalar(*sc, true).to_string(),
                    other => java_field_type(other, &FieldLabel::Singular),
                };
                format!("java.util.Map<{}, {}>", kt, vt)
            }
        },
    }
}

/// Generates Java `<OuterClass>.java` and `<Service>Grpc.java`.
pub fn gen_java(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let java_pkg = file
        .option_value("java_package")
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if file.package.is_empty() {
                "com.protobuilder.generated".to_string()
            } else {
                file.package.clone()
            }
        });
    let outer_class = file
        .option_value("java_outer_classname")
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}Proto", to_pascal_case(&file.file_stem())));

    if cfg.generate_messages {
        let mut j = banner_comment(file, TargetLang::Java, "//");
        j.push_str(&format!("package {};\n\nimport java.util.*;\n\n", java_pkg));
        j.push_str(&format!("public final class {} {{\n  private {}() {{}}\n\n", outer_class, outer_class));

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            j.push_str(&format!("  public enum {} {{\n", en_name));
            for (idx, v) in en.values.iter().enumerate() {
                let term = if idx + 1 < en.values.len() { "," } else { ";" };
                j.push_str(&format!("    {}({}){}\n", v.name, v.number, term));
            }
            j.push_str(&format!(
                "\n    private final int value;\n    {}(int value) {{ this.value = value; }}\n    public int getNumber() {{ return value; }}\n  }}\n\n",
                en_name
            ));
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            j.push_str(&format!("  public static final class {} {{\n", msg_name));
            for f in msg.all_fields() {
                let ty = java_field_type(&f.field_type, &f.label);
                let cname = to_camel_case(&f.name);
                j.push_str(&format!("    private {} {};\n", ty, cname));
            }
            j.push_str(&format!("\n    public {}() {{}}\n\n", msg_name));
            for f in msg.all_fields() {
                let ty = java_field_type(&f.field_type, &f.label);
                let cname = to_camel_case(&f.name);
                let pname = to_pascal_case(&f.name);
                j.push_str(&format!("    public {} get{}() {{ return this.{}; }}\n", ty, pname, cname));
                j.push_str(&format!(
                    "    public {} set{}({} value) {{ this.{} = value; return this; }}\n",
                    msg_name, pname, ty, cname
                ));
            }
            j.push_str(&format!("    public static {} newBuilder() {{ return new {}(); }}\n", msg_name, msg_name));
            j.push_str("  }\n\n");
        }

        j.push_str("}\n");
        files.push((format!("{}.java", outer_class), j));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            let grpc_class = format!("{}Grpc", svc_name);
            let mut g = banner_comment(file, TargetLang::Java, "//");
            g.push_str(&format!("package {};\n\nimport java.util.*;\nimport java.util.concurrent.CompletableFuture;\nimport {}.{}*;\n\n", java_pkg, java_pkg, ""));
            g.push_str(&format!("public final class {} {{\n  public static final String SERVICE_NAME = \"{}\";\n\n", grpc_class, svc.full_name));

            for rpc in &svc.methods {
                let const_name = format!("METHOD_{}", to_screaming_snake_case(&rpc.name));
                g.push_str(&format!(
                    "  public static final String {} = \"/{}/{}\";\n",
                    const_name, svc.full_name, rpc.name
                ));
            }

            // ImplBase
            g.push_str(&format!(
                "\n  public static abstract class {}ImplBase {{\n",
                svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = format!("{}.{}", outer_class, to_pascal_case(rpc.input_short()));
                let out_ty = format!("{}.{}", outer_class, to_pascal_case(rpc.output_short()));
                let req_sig = if rpc.client_streaming {
                    format!("List<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("List<{}>", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    public {} {}({} request) {{\n      throw new UnsupportedOperationException(\"Method {} is unimplemented\");\n    }}\n",
                    resp_sig, m_name, req_sig, rpc.name
                ));
            }
            g.push_str("  }\n\n");

            // BlockingStub
            g.push_str(&format!(
                "  public static final class {}BlockingStub {{\n    private final {}ImplBase target;\n    public {}BlockingStub({}ImplBase target) {{ this.target = target; }}\n",
                svc_name, svc_name, svc_name, svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = format!("{}.{}", outer_class, to_pascal_case(rpc.input_short()));
                let out_ty = format!("{}.{}", outer_class, to_pascal_case(rpc.output_short()));
                let req_sig = if rpc.client_streaming {
                    format!("List<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("List<{}>", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    public {} {}({} request) {{ return target.{}(request); }}\n",
                    resp_sig, m_name, req_sig, m_name
                ));
            }
            g.push_str("  }\n}\n");
            files.push((format!("{}.java", grpc_class), g));
        }
    }

    files
}

fn kt_scalar(sc: ScalarType) -> (&'static str, &'static str) {
    match sc {
        ScalarType::Double => ("Double", "0.0"),
        ScalarType::Float => ("Float", "0.0f"),
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => ("Int", "0"),
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => ("Long", "0L"),
        ScalarType::Uint32 | ScalarType::Fixed32 => ("UInt", "0u"),
        ScalarType::Uint64 | ScalarType::Fixed64 => ("ULong", "0uL"),
        ScalarType::Bool => ("Boolean", "false"),
        ScalarType::String => ("String", "\"\""),
        ScalarType::Bytes => ("ByteArray", "ByteArray(0)"),
    }
}

fn kt_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    let (base_t, base_d) = match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = kt_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                (short.clone(), format!("{}.entries.first()", short))
            } else {
                (format!("{}?", short), "null".to_string())
            }
        }
        FieldType::Map(k, v) => {
            let (kt, _) = kt_field_type(k, &FieldLabel::Singular);
            let (vt, _) = kt_field_type(v, &FieldLabel::Singular);
            return (
                format!("Map<{}, {}>", kt, vt.trim_end_matches('?')),
                "emptyMap()".to_string(),
            );
        }
    };
    match label {
        FieldLabel::Repeated => (
            format!("List<{}>", base_t.trim_end_matches('?')),
            "emptyList()".to_string(),
        ),
        _ => (base_t, base_d),
    }
}

/// Generates Kotlin `<Name>Kt.kt` and `<Name>GrpcKt.kt`.
pub fn gen_kotlin(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem_pascal = to_pascal_case(&file.file_stem());
    let kt_pkg = if file.package.is_empty() {
        "protobuilder.generated"
    } else {
        &file.package
    };

    if cfg.generate_messages {
        let mut kt = banner_comment(file, TargetLang::Kotlin, "//");
        kt.push_str(&format!("package {}\n\n", kt_pkg));

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            kt.push_str(&format!("enum class {}(val number: Int) {{\n", en_name));
            for (idx, v) in en.values.iter().enumerate() {
                let sep = if idx + 1 < en.values.len() { "," } else { ";" };
                kt.push_str(&format!("    {}({}){}\n", v.name, v.number, sep));
            }
            kt.push_str("}\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            let all_fields = msg.all_fields();
            if all_fields.is_empty() {
                kt.push_str(&format!("class {}()\n\n", msg_name));
            } else {
                kt.push_str(&format!("data class {}(\n", msg_name));
                for (idx, f) in all_fields.iter().enumerate() {
                    let (ty, def_val) = kt_field_type(&f.field_type, &f.label);
                    let comma = if idx + 1 < all_fields.len() { "," } else { "" };
                    kt.push_str(&format!(
                        "    val {}: {} = {}{}\n",
                        to_camel_case(&f.name),
                        ty,
                        def_val,
                        comma
                    ));
                }
                kt.push_str(")\n\n");
            }
        }
        files.push((format!("{}Kt.kt", stem_pascal), kt));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::Kotlin, "//");
        g.push_str(&format!("package {}\n\n", kt_pkg));

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("object {}GrpcKt {{\n", svc_name));
            g.push_str(&format!(
                "    const val SERVICE_NAME: String = \"{}\"\n\n",
                svc.full_name
            ));
            g.push_str(&format!(
                "    abstract class {}CoroutineImplBase {{\n",
                svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("List<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("List<{}>", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "        open suspend fun {}(request: {}): {} =\n            throw NotImplementedError(\"Method {}.{} is unimplemented\")\n\n",
                    m_name, req_sig, resp_sig, svc.full_name, rpc.name
                ));
            }
            g.push_str("    }\n\n");

            g.push_str(&format!(
                "    class {}CoroutineStub(private val service: {}CoroutineImplBase) {{\n",
                svc_name, svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("List<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("List<{}>", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "        suspend fun {}(request: {}): {} = service.{}(request)\n",
                    m_name, req_sig, resp_sig, m_name
                ));
            }
            g.push_str("    }\n}\n\n");
        }
        files.push((format!("{}GrpcKt.kt", stem_pascal), g));
    }

    files
}

fn cs_scalar(sc: ScalarType) -> (&'static str, &'static str) {
    match sc {
        ScalarType::Double => ("double", "0.0"),
        ScalarType::Float => ("float", "0.0f"),
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => ("int", "0"),
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => ("long", "0L"),
        ScalarType::Uint32 | ScalarType::Fixed32 => ("uint", "0u"),
        ScalarType::Uint64 | ScalarType::Fixed64 => ("ulong", "0UL"),
        ScalarType::Bool => ("bool", "false"),
        ScalarType::String => ("string", "\"\""),
        ScalarType::Bytes => ("byte[]", "Array.Empty<byte>()"),
    }
}

fn cs_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    let (base_t, base_d) = match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = cs_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                (short.clone(), format!("({})0", short))
            } else {
                (format!("{}?", short), "null".to_string())
            }
        }
        FieldType::Map(k, v) => {
            let (kt, _) = cs_field_type(k, &FieldLabel::Singular);
            let (vt, _) = cs_field_type(v, &FieldLabel::Singular);
            let vt_clean = vt.trim_end_matches('?');
            return (
                format!("Dictionary<{}, {}>", kt, vt_clean),
                format!("new Dictionary<{}, {}>()", kt, vt_clean),
            );
        }
    };
    match label {
        FieldLabel::Repeated => {
            let clean = base_t.trim_end_matches('?');
            (
                format!("List<{}>", clean),
                format!("new List<{}>()", clean),
            )
        }
        _ => (base_t, base_d),
    }
}

/// Generates C# `<Name>.pb.cs` and `<Name>Grpc.cs`.
pub fn gen_csharp(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem_pascal = to_pascal_case(&file.file_stem());
    let cs_ns = file
        .option_value("csharp_namespace")
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if file.package.is_empty() {
                "ProtoBuilder.Generated".to_string()
            } else {
                file.package
                    .split('.')
                    .map(to_pascal_case)
                    .collect::<Vec<_>>()
                    .join(".")
            }
        });

    if cfg.generate_messages {
        let mut cs = banner_comment(file, TargetLang::CSharp, "//");
        cs.push_str("#nullable enable\nusing System;\nusing System.Collections.Generic;\nusing System.Text.Json;\n\n");
        cs.push_str(&format!("namespace {}\n{{\n", cs_ns));

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            cs.push_str(&format!("    public enum {}\n    {{\n", en_name));
            for v in &en.values {
                cs.push_str(&format!(
                    "        {} = {},\n",
                    to_pascal_case(&v.name),
                    v.number
                ));
            }
            cs.push_str("    }\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            cs.push_str(&format!("    public sealed partial class {}\n    {{\n", msg_name));
            for f in msg.all_fields() {
                let (ty, def_val) = cs_field_type(&f.field_type, &f.label);
                cs.push_str(&format!(
                    "        public {} {} {{ get; set; }} = {};\n",
                    ty,
                    to_pascal_case(&f.name),
                    def_val
                ));
            }
            cs.push_str("\n        public byte[] ToByteArray() => JsonSerializer.SerializeToUtf8Bytes(this);\n");
            cs.push_str(&format!(
                "        public static {} ParseFrom(byte[] data) => JsonSerializer.Deserialize<{}>(data) ?? new {}();\n",
                msg_name, msg_name, msg_name
            ));
            cs.push_str("    }\n\n");
        }

        cs.push_str("}\n");
        files.push((format!("{}.pb.cs", stem_pascal), cs));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::CSharp, "//");
        g.push_str("#nullable enable\nusing System;\nusing System.Collections.Generic;\nusing System.Threading;\nusing System.Threading.Tasks;\n\n");
        g.push_str(&format!("namespace {}\n{{\n", cs_ns));

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("    public static partial class {}\n    {{\n", svc_name));
            g.push_str(&format!(
                "        public const string ServiceName = \"{}\";\n\n",
                svc.full_name
            ));

            // Server Base
            g.push_str(&format!("        public abstract partial class {}Base\n        {{\n", svc_name));
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("IReadOnlyList<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("Task<IReadOnlyList<{}>>", out_ty)
                } else {
                    format!("Task<{}>", out_ty)
                };
                g.push_str(&format!(
                    "            public virtual {} {}( {} request, CancellationToken cancellationToken = default)\n            {{\n                throw new NotImplementedException(\"RPC {}.{} not implemented\");\n            }}\n",
                    resp_sig, m_name, req_sig, svc.full_name, rpc.name
                ));
            }
            g.push_str("        }\n\n");

            // Client
            g.push_str(&format!("        public partial class {}Client\n        {{\n            private readonly {}Base _service;\n            public {}Client({}Base service) {{ _service = service; }}\n", svc_name, svc_name, svc_name, svc_name));
            for rpc in &svc.methods {
                let m_name = to_pascal_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("IReadOnlyList<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("Task<IReadOnlyList<{}>>", out_ty)
                } else {
                    format!("Task<{}>", out_ty)
                };
                g.push_str(&format!(
                    "            public {} {}Async({} request, CancellationToken ct = default) => _service.{}(request, ct);\n",
                    resp_sig, m_name, req_sig, m_name
                ));
            }
            g.push_str("        }\n    }\n\n");
        }
        g.push_str("}\n");
        files.push((format!("{}Grpc.cs", stem_pascal), g));
    }

    files
}

fn swift_scalar(sc: ScalarType) -> (&'static str, &'static str) {
    match sc {
        ScalarType::Double => ("Double", "0.0"),
        ScalarType::Float => ("Float", "0.0"),
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => ("Int32", "0"),
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => ("Int64", "0"),
        ScalarType::Uint32 | ScalarType::Fixed32 => ("UInt32", "0"),
        ScalarType::Uint64 | ScalarType::Fixed64 => ("UInt64", "0"),
        ScalarType::Bool => ("Bool", "false"),
        ScalarType::String => ("String", "\"\""),
        ScalarType::Bytes => ("Data", "Data()"),
    }
}

fn swift_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    let (base_t, base_d) = match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = swift_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                (short.clone(), format!("{}(rawValue: 0)!", short))
            } else {
                (format!("{}?", short), "nil".to_string())
            }
        }
        FieldType::Map(k, v) => {
            let (kt, _) = swift_field_type(k, &FieldLabel::Singular);
            let (vt, _) = swift_field_type(v, &FieldLabel::Singular);
            return (
                format!("[{}: {}]", kt, vt.trim_end_matches('?')),
                "[:]".to_string(),
            );
        }
    };
    match label {
        FieldLabel::Repeated => (
            format!("[{}]", base_t.trim_end_matches('?')),
            "[]".to_string(),
        ),
        _ => (base_t, base_d),
    }
}

/// Generates Swift `<Name>.pb.swift` and `<Name>.grpc.swift`.
pub fn gen_swift(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem_pascal = to_pascal_case(&file.file_stem());

    if cfg.generate_messages {
        let mut sw = banner_comment(file, TargetLang::Swift, "//");
        sw.push_str("import Foundation\n\n");

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            sw.push_str(&format!(
                "public enum {}: Int32, Codable, Sendable, CaseIterable {{\n",
                en_name
            ));
            for v in &en.values {
                sw.push_str(&format!(
                    "    case {} = {}\n",
                    to_camel_case(&v.name),
                    v.number
                ));
            }
            sw.push_str("}\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            sw.push_str(&format!(
                "public struct {}: Codable, Sendable, Equatable {{\n",
                msg_name
            ));
            sw.push_str(&format!(
                "    public static let protoMessageName: String = \"{}\"\n",
                msg.full_name
            ));
            for f in msg.all_fields() {
                let (ty, def_val) = swift_field_type(&f.field_type, &f.label);
                sw.push_str(&format!(
                    "    public var {}: {} = {}\n",
                    to_camel_case(&f.name),
                    ty,
                    def_val
                ));
            }
            sw.push_str("    public init() {}\n}\n\n");
        }

        files.push((format!("{}.pb.swift", stem_pascal), sw));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::Swift, "//");
        g.push_str("import Foundation\n\n");

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!(
                "public protocol {}AsyncProvider: Sendable {{\n",
                svc_name
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("[{}]", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("[{}]", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    func {}(request: {}) async throws -> {}\n",
                    m_name, req_sig, resp_sig
                ));
            }
            g.push_str("}\n\n");

            g.push_str(&format!(
                "public struct {}AsyncClient: Sendable {{\n    public static let serviceName = \"{}\"\n    public let endpoint: String\n    public init(endpoint: String) {{ self.endpoint = endpoint }}\n",
                svc_name, svc.full_name
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("[{}]", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("[{}]", out_ty)
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    public func {}(request: {}, provider: any {}AsyncProvider) async throws -> {} {{\n        return try await provider.{}(request: request)\n    }}\n",
                    m_name, req_sig, svc_name, resp_sig, m_name
                ));
            }
            g.push_str("}\n\n");
        }
        files.push((format!("{}.grpc.swift", stem_pascal), g));
    }

    files
}
