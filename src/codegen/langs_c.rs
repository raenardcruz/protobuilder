use crate::ast::*;
use crate::codegen::*;

fn dart_scalar(sc: ScalarType) -> (&'static str, &'static str) {
    match sc {
        ScalarType::Double | ScalarType::Float => ("double", "0.0"),
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
        ScalarType::Bool => ("bool", "false"),
        ScalarType::String => ("String", "''"),
        ScalarType::Bytes => ("List<int>", "const []"),
    }
}

fn dart_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    let (base_t, base_d) = match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = dart_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                (short.clone(), format!("{}.values.first", short))
            } else {
                (format!("{}?", short), "null".to_string())
            }
        }
        FieldType::Map(k, v) => {
            let (kt, _) = dart_field_type(k, &FieldLabel::Singular);
            let (vt, _) = dart_field_type(v, &FieldLabel::Singular);
            return (
                format!("Map<{}, {}>", kt, vt.trim_end_matches('?')),
                "const {}".to_string(),
            );
        }
    };
    match label {
        FieldLabel::Repeated => (
            format!("List<{}>", base_t.trim_end_matches('?')),
            "const []".to_string(),
        ),
        _ => (base_t, base_d),
    }
}

/// Generates Dart `<stem>.pb.dart`, `<stem>.pbenum.dart`, and `<stem>.pbgrpc.dart`.
pub fn gen_dart(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();

    if cfg.generate_messages {
        let mut en_out = banner_comment(file, TargetLang::Dart, "//");
        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            en_out.push_str(&format!("enum {} {{\n", en_name));
            for (idx, v) in en.values.iter().enumerate() {
                let sep = if idx + 1 < en.values.len() { "," } else { ";" };
                en_out.push_str(&format!("  {}({}){}\n", to_camel_case(&v.name), v.number, sep));
            }
            en_out.push_str(&format!("\n  final int value;\n  const {}(this.value);\n}}\n\n", en_name));
        }
        files.push((format!("{}.pbenum.dart", stem), en_out));

        let mut pb = banner_comment(file, TargetLang::Dart, "//");
        pb.push_str(&format!("import 'dart:convert';\nimport '{}.pbenum.dart';\nexport '{}.pbenum.dart';\n\n", stem, stem));

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            pb.push_str(&format!("class {} {{\n", msg_name));
            for f in msg.all_fields() {
                let (ty, _) = dart_field_type(&f.field_type, &f.label);
                pb.push_str(&format!("  {} {};\n", ty, to_camel_case(&f.name)));
            }
            pb.push_str(&format!("\n  {}({{\n", msg_name));
            for f in msg.all_fields() {
                let (_, def_val) = dart_field_type(&f.field_type, &f.label);
                pb.push_str(&format!("    this.{} = {},\n", to_camel_case(&f.name), def_val));
            }
            pb.push_str("  });\n\n");
            pb.push_str("  Map<String, dynamic> toJson() => {\n");
            for f in msg.all_fields() {
                let cname = to_camel_case(&f.name);
                pb.push_str(&format!("    '{}': {},\n", cname, cname));
            }
            pb.push_str("  };\n\n");
            pb.push_str("  List<int> writeToBuffer() => utf8.encode(jsonEncode(toJson()));\n");
            pb.push_str("}\n\n");
        }
        files.push((format!("{}.pb.dart", stem), pb));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = banner_comment(file, TargetLang::Dart, "//");
        g.push_str(&format!("import 'dart:async';\nimport '{}.pb.dart';\n\n", stem));

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("abstract class {}ServiceBase {{\n", svc_name));
            g.push_str(&format!("  static const String $name = '{}';\n\n", svc.full_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("Stream<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("Stream<{}>", out_ty)
                } else {
                    format!("Future<{}>", out_ty)
                };
                g.push_str(&format!("  {} {}({} request);\n", resp_sig, m_name, req_sig));
            }
            g.push_str("}\n\n");

            g.push_str(&format!("class {}Client {{\n  final {}ServiceBase _service;\n  {}Client(this._service);\n\n", svc_name, svc_name, svc_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    format!("Stream<{}>", in_ty)
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    format!("Stream<{}>", out_ty)
                } else {
                    format!("Future<{}>", out_ty)
                };
                g.push_str(&format!(
                    "  {} {}({} request) => _service.{}(request);\n",
                    resp_sig, m_name, req_sig, m_name
                ));
            }
            g.push_str("}\n\n");
        }
        files.push((format!("{}.pbgrpc.dart", stem), g));
    }

    files
}

fn php_scalar(sc: ScalarType) -> (&'static str, &'static str) {
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
        ScalarType::Bool => ("bool", "false"),
        ScalarType::String | ScalarType::Bytes => ("string", "''"),
    }
}

fn php_field_type(ft: &FieldType, label: &FieldLabel) -> (String, String) {
    if matches!(label, FieldLabel::Repeated) || matches!(ft, FieldType::Map(_, _)) {
        return ("array".to_string(), "[]".to_string());
    }
    match ft {
        FieldType::Scalar(sc) => {
            let (t, d) = php_scalar(*sc);
            (t.to_string(), d.to_string())
        }
        FieldType::Named { raw, kind, .. } => {
            let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
            if *kind == ResolvedKind::Enum {
                ("int".to_string(), "0".to_string())
            } else {
                (format!("?{}", short), "null".to_string())
            }
        }
        FieldType::Map(_, _) => ("array".to_string(), "[]".to_string()),
    }
}

/// Generates PHP `<Name>Pb.php` and `<Service>GrpcClient.php`.
pub fn gen_php(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem_pascal = to_pascal_case(&file.file_stem());
    let php_ns = file
        .option_value("php_namespace")
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if file.package.is_empty() {
                "ProtoBuilder\\Generated".to_string()
            } else {
                file.package
                    .split('.')
                    .map(to_pascal_case)
                    .collect::<Vec<_>>()
                    .join("\\")
            }
        });

    if cfg.generate_messages {
        let mut php = String::from("<?php\ndeclare(strict_types=1);\n");
        php.push_str(&banner_comment(file, TargetLang::Php, "//"));
        php.push_str(&format!("namespace {};\n\n", php_ns));

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            php.push_str(&format!("final class {}\n{{\n", en_name));
            for v in &en.values {
                php.push_str(&format!("    public const {} = {};\n", v.name, v.number));
            }
            php.push_str("}\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            php.push_str(&format!("class {}\n{{\n", msg_name));
            for f in msg.all_fields() {
                let (ty, def_val) = php_field_type(&f.field_type, &f.label);
                php.push_str(&format!(
                    "    public {} ${} = {};\n",
                    ty,
                    to_camel_case(&f.name),
                    def_val
                ));
            }
            php.push_str("\n    public function __construct(array $init = [])\n    {\n        foreach ($init as $k => $v) {\n            if (property_exists($this, $k)) {\n                $this->$k = $v;\n            }\n        }\n    }\n");
            php.push_str("\n    public function serializeToString(): string\n    {\n        return (string)json_encode(get_object_vars($this));\n    }\n");
            php.push_str("}\n\n");
        }
        files.push((format!("{}Pb.php", stem_pascal), php));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            let mut g = String::from("<?php\ndeclare(strict_types=1);\n");
            g.push_str(&banner_comment(file, TargetLang::Php, "//"));
            g.push_str(&format!("namespace {};\n\n", php_ns));

            g.push_str(&format!("interface {}ServiceInterface\n{{\n", svc_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let out_ty = to_pascal_case(rpc.output_short());
                let req_sig = if rpc.client_streaming {
                    "array".to_string()
                } else {
                    in_ty
                };
                let resp_sig = if rpc.server_streaming {
                    "array".to_string()
                } else {
                    out_ty
                };
                g.push_str(&format!(
                    "    public function {}({} $request): {};\n",
                    m_name, req_sig, resp_sig
                ));
            }
            g.push_str("}\n\n");

            g.push_str(&format!("class {}GrpcClient\n{{\n    public const SERVICE_NAME = '{}';\n    private string $hostname;\n\n    public function __construct(string $hostname)\n    {{\n        $this->hostname = $hostname;\n    }}\n\n", svc_name, svc.full_name));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_ty = to_pascal_case(rpc.input_short());
                let req_sig = if rpc.client_streaming {
                    "array".to_string()
                } else {
                    in_ty
                };
                g.push_str(&format!(
                    "    public function {}({} $argument, array $metadata = []): array\n    {{\n        return ['path' => '/{}/{}', 'hostname' => $this->hostname, 'argument' => $argument, 'metadata' => $metadata];\n    }}\n\n",
                    m_name, req_sig, svc.full_name, rpc.name
                ));
            }
            g.push_str("}\n");
            files.push((format!("{}GrpcClient.php", svc_name), g));
        }
    }

    files
}

/// Generates Ruby `<stem>_pb.rb`, `<stem>_services_pb.rb`, and RBS type definitions `<stem>_pb.rbs`.
pub fn gen_ruby(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();
    let mod_name = file
        .option_value("ruby_package")
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if file.package.is_empty() {
                to_pascal_case(&stem)
            } else {
                file.package
                    .split('.')
                    .map(to_pascal_case)
                    .collect::<Vec<_>>()
                    .join("::")
            }
        });

    if cfg.generate_messages {
        let mut rb = String::from("# frozen_string_literal: true\n");
        rb.push_str(&banner_comment(file, TargetLang::Ruby, "#"));
        rb.push_str("require 'json'\n\n");
        rb.push_str(&format!("module {}\n", mod_name));

        let mut rbs = banner_comment(file, TargetLang::Ruby, "#");
        rbs.push_str(&format!("module {}\n", mod_name));

        for en in file.all_enums_flat() {
            let en_name = to_pascal_case(&en.name);
            rb.push_str(&format!("  module {}\n", en_name));
            rbs.push_str(&format!("  module {}\n", en_name));
            for v in &en.values {
                rb.push_str(&format!("    {} = {}\n", v.name, v.number));
                rbs.push_str(&format!("    {}: Integer\n", v.name));
            }
            rb.push_str("  end\n\n");
            rbs.push_str("  end\n\n");
        }

        for msg in file.all_messages_flat() {
            let msg_name = to_pascal_case(&msg.name);
            rb.push_str(&format!("  class {}\n", msg_name));
            rbs.push_str(&format!("  class {}\n", msg_name));
            let all_fields = msg.all_fields();
            if !all_fields.is_empty() {
                let attrs: Vec<String> = all_fields
                    .iter()
                    .map(|f| format!(":{}", to_snake_case(&f.name)))
                    .collect();
                rb.push_str(&format!("    attr_accessor {}\n\n", attrs.join(", ")));
            }
            rb.push_str("    def initialize(**kwargs)\n");
            for f in &all_fields {
                let sname = to_snake_case(&f.name);
                rb.push_str(&format!("      @{} = kwargs[:{}]\n", sname, sname));
                rbs.push_str(&format!("    attr_accessor {}: untyped\n", sname));
            }
            rb.push_str("    end\n\n");
            rb.push_str("    def to_h\n      {\n");
            for f in &all_fields {
                let sname = to_snake_case(&f.name);
                rb.push_str(&format!("        {}: @{},\n", sname, sname));
            }
            rb.push_str("      }\n    end\n\n");
            rb.push_str("    def to_proto\n      JSON.generate(to_h)\n    end\n");
            rb.push_str("  end\n\n");

            rbs.push_str("    def to_h: () -> Hash[Symbol, untyped]\n");
            rbs.push_str("    def to_proto: () -> String\n");
            rbs.push_str("  end\n\n");
        }

        rb.push_str("end\n");
        rbs.push_str("end\n");
        files.push((format!("{}_pb.rb", stem), rb));
        files.push((format!("{}_pb.rbs", stem), rbs));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut g = String::from("# frozen_string_literal: true\n");
        g.push_str(&banner_comment(file, TargetLang::Ruby, "#"));
        g.push_str(&format!("require_relative '{}_pb'\n\nmodule {}\n", stem, mod_name));

        for svc in &file.services {
            let svc_name = to_pascal_case(&svc.name);
            g.push_str(&format!("  module {}\n", svc_name));
            g.push_str(&format!("    SERVICE_NAME = '{}'\n\n", svc.full_name));
            g.push_str("    class Service\n");
            for rpc in &svc.methods {
                let m_rb = to_snake_case(&rpc.name);
                g.push_str(&format!(
                    "      def {}(request, _call = nil)\n        raise NotImplementedError, 'RPC {}.{} is unimplemented'\n      end\n\n",
                    m_rb, svc.full_name, rpc.name
                ));
            }
            g.push_str("    end\n\n");

            g.push_str("    class Stub\n      attr_reader :host\n      def initialize(host)\n        @host = host\n      end\n\n");
            for rpc in &svc.methods {
                let m_rb = to_snake_case(&rpc.name);
                g.push_str(&format!(
                    "      def {}(request, metadata: {{}})\n        {{ service: SERVICE_NAME, rpc: '{}', host: @host, request: request, metadata: metadata }}\n      end\n\n",
                    m_rb, rpc.name
                ));
            }
            g.push_str("    end\n  end\n\n");
        }
        g.push_str("end\n");
        files.push((format!("{}_services_pb.rb", stem), g));
    }

    files
}

/// Generates Objective-C `<Name>.pbobjc.h`, `<Name>.pbobjc.m`, `<Name>.pbrpc.h`, and `<Name>.pbrpc.m`.
pub fn gen_objc(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem_pascal = to_pascal_case(&file.file_stem());
    let prefix = file.option_value("objc_class_prefix").unwrap_or("PB");

    if cfg.generate_messages {
        let mut h = banner_comment(file, TargetLang::ObjC, "//");
        h.push_str("#import <Foundation/Foundation.h>\n\nNS_ASSUME_NONNULL_BEGIN\n\n");

        let mut m = banner_comment(file, TargetLang::ObjC, "//");
        m.push_str(&format!("#import \"{}.pbobjc.h\"\n\n", stem_pascal));

        for en in file.all_enums_flat() {
            let en_name = format!("{}{}", prefix, to_pascal_case(&en.name));
            h.push_str(&format!("typedef NS_ENUM(int32_t, {}) {{\n", en_name));
            for v in &en.values {
                h.push_str(&format!(
                    "  {}_{} = {},\n",
                    en_name,
                    to_pascal_case(&v.name),
                    v.number
                ));
            }
            h.push_str("};\n\n");
        }

        for msg in file.all_messages_flat() {
            let cls_name = format!("{}{}", prefix, to_pascal_case(&msg.name));
            h.push_str(&format!("@interface {} : NSObject\n", cls_name));
            for f in msg.all_fields() {
                let cname = to_camel_case(&f.name);
                match (&f.field_type, &f.label) {
                    (_, FieldLabel::Repeated) => {
                        h.push_str(&format!("@property(nonatomic, strong) NSArray *{};\n", cname));
                    }
                    (FieldType::Map(_, _), _) => {
                        h.push_str(&format!("@property(nonatomic, strong) NSDictionary *{};\n", cname));
                    }
                    (FieldType::Scalar(ScalarType::String), _) => {
                        h.push_str(&format!("@property(nonatomic, copy) NSString *{};\n", cname));
                    }
                    (FieldType::Scalar(ScalarType::Bytes), _) => {
                        h.push_str(&format!("@property(nonatomic, copy) NSData *{};\n", cname));
                    }
                    (FieldType::Scalar(ScalarType::Bool), _) => {
                        h.push_str(&format!("@property(nonatomic, assign) BOOL {};\n", cname));
                    }
                    (FieldType::Scalar(ScalarType::Double), _) => {
                        h.push_str(&format!("@property(nonatomic, assign) double {};\n", cname));
                    }
                    (FieldType::Scalar(ScalarType::Float), _) => {
                        h.push_str(&format!("@property(nonatomic, assign) float {};\n", cname));
                    }
                    (FieldType::Scalar(_), _) => {
                        h.push_str(&format!("@property(nonatomic, assign) int64_t {};\n", cname));
                    }
                    (FieldType::Named { raw, kind, .. }, _) => {
                        let short = to_pascal_case(raw.rsplit('.').next().unwrap_or(raw));
                        if *kind == ResolvedKind::Enum {
                            h.push_str(&format!("@property(nonatomic, assign) {}{} {};\n", prefix, short, cname));
                        } else {
                            h.push_str(&format!("@property(nonatomic, strong, nullable) {}{} *{};\n", prefix, short, cname));
                        }
                    }
                }
            }
            h.push_str("- (NSData *)data;\n@end\n\n");

            m.push_str(&format!(
                "@implementation {}\n- (NSData *)data {{\n  return [@\"{}\" dataUsingEncoding:NSUTF8StringEncoding];\n}}\n@end\n\n",
                cls_name, msg.full_name
            ));
        }

        h.push_str("NS_ASSUME_NONNULL_END\n");
        files.push((format!("{}.pbobjc.h", stem_pascal), h));
        files.push((format!("{}.pbobjc.m", stem_pascal), m));
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        let mut gh = banner_comment(file, TargetLang::ObjC, "//");
        gh.push_str(&format!("#import \"{}.pbobjc.h\"\n\nNS_ASSUME_NONNULL_BEGIN\n\n", stem_pascal));

        let mut gm = banner_comment(file, TargetLang::ObjC, "//");
        gm.push_str(&format!("#import \"{}.pbrpc.h\"\n\n", stem_pascal));

        for svc in &file.services {
            let svc_cls = format!("{}{}", prefix, to_pascal_case(&svc.name));
            gh.push_str(&format!("@protocol {}Service <NSObject>\n", svc_cls));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_cls = format!("{}{}", prefix, to_pascal_case(rpc.input_short()));
                let out_cls = format!("{}{}", prefix, to_pascal_case(rpc.output_short()));
                gh.push_str(&format!(
                    "- (void){}WithRequest:({} *)request handler:(void (^)( {} * _Nullable response, NSError * _Nullable error))handler;\n",
                    m_name, in_cls, out_cls
                ));
            }
            gh.push_str("@end\n\n");

            gh.push_str(&format!(
                "@interface {}Client : NSObject <{}Service>\n- (instancetype)initWithHost:(NSString *)host;\n@end\n\n",
                svc_cls, svc_cls
            ));

            gm.push_str(&format!(
                "@implementation {}Client {{\n  NSString *_host;\n}}\n- (instancetype)initWithHost:(NSString *)host {{\n  if ((self = [super init])) {{ _host = [host copy]; }}\n  return self;\n}}\n",
                svc_cls
            ));
            for rpc in &svc.methods {
                let m_name = to_camel_case(&rpc.name);
                let in_cls = format!("{}{}", prefix, to_pascal_case(rpc.input_short()));
                let out_cls = format!("{}{}", prefix, to_pascal_case(rpc.output_short()));
                gm.push_str(&format!(
                    "- (void){}WithRequest:({} *)request handler:(void (^)( {} * _Nullable, NSError * _Nullable))handler {{\n  if (handler) handler([[{} alloc] init], nil);\n}}\n",
                    m_name, in_cls, out_cls, out_cls
                ));
            }
            gm.push_str("@end\n\n");
        }

        gh.push_str("NS_ASSUME_NONNULL_END\n");
        files.push((format!("{}.pbrpc.h", stem_pascal), gh));
        files.push((format!("{}.pbrpc.m", stem_pascal), gm));
    }

    files
}

fn c_scalar(sc: ScalarType) -> &'static str {
    match sc {
        ScalarType::Double => "double",
        ScalarType::Float => "float",
        ScalarType::Int32 | ScalarType::Sint32 | ScalarType::Sfixed32 => "int32_t",
        ScalarType::Int64 | ScalarType::Sint64 | ScalarType::Sfixed64 => "int64_t",
        ScalarType::Uint32 | ScalarType::Fixed32 => "uint32_t",
        ScalarType::Uint64 | ScalarType::Fixed64 => "uint64_t",
        ScalarType::Bool => "bool",
        ScalarType::String => "const char*",
        ScalarType::Bytes => "struct { const uint8_t* data; size_t len; }",
    }
}

/// Generates ANSI/C11 `<stem>.pb.h` and `<stem>.pb.c` (with gRPC service vtable struct).
pub fn gen_c(file: &ProtoFile, cfg: &GenConfig) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let stem = file.file_stem();
    let guard = format!("PROTOBUILDER_{}_PB_H", to_screaming_snake_case(&stem));
    let pkg_prefix = if file.package.is_empty() {
        "pb".to_string()
    } else {
        to_snake_case(&file.package)
    };

    let mut h = banner_comment(file, TargetLang::C, "//");
    h.push_str(&format!(
        "#ifndef {}\n#define {}\n\n#include <stdint.h>\n#include <stddef.h>\n#include <stdbool.h>\n\n#ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n",
        guard, guard
    ));

    let mut c_src = banner_comment(file, TargetLang::C, "//");
    c_src.push_str(&format!("#include \"{}.pb.h\"\n#include <string.h>\n\n", stem));

    if cfg.generate_messages {
        for en in file.all_enums_flat() {
            let en_name = format!("{}_{}_t", pkg_prefix, to_snake_case(&en.name));
            h.push_str(&format!("typedef enum {{\n"));
            for v in &en.values {
                h.push_str(&format!(
                    "    {}_{} = {},\n",
                    to_screaming_snake_case(&en.name),
                    v.name,
                    v.number
                ));
            }
            h.push_str(&format!("}} {};\n\n", en_name));
        }

        for msg in file.all_messages_flat() {
            let st_name = format!("{}_{}_t", pkg_prefix, to_snake_case(&msg.name));
            h.push_str(&format!("typedef struct {}_{} {{\n", pkg_prefix, to_snake_case(&msg.name)));
            let all_fields = msg.all_fields();
            if all_fields.is_empty() {
                h.push_str("    uint8_t _dummy;\n");
            } else {
                for f in &all_fields {
                    let fname = to_snake_case(&f.name);
                    match (&f.field_type, &f.label) {
                        (FieldType::Scalar(sc), FieldLabel::Repeated) => {
                            h.push_str(&format!("    {}* {};\n    size_t {}_count;\n", c_scalar(*sc), fname, fname));
                        }
                        (FieldType::Scalar(sc), _) => {
                            h.push_str(&format!("    {} {};\n", c_scalar(*sc), fname));
                        }
                        (FieldType::Named { raw, .. }, FieldLabel::Repeated) => {
                            let short = to_snake_case(raw.rsplit('.').next().unwrap_or(raw));
                            h.push_str(&format!(
                                "    struct {}_{}* {};\n    size_t {}_count;\n",
                                pkg_prefix, short, fname, fname
                            ));
                        }
                        (FieldType::Named { raw, kind, .. }, _) => {
                            let short = to_snake_case(raw.rsplit('.').next().unwrap_or(raw));
                            if *kind == ResolvedKind::Enum {
                                h.push_str(&format!("    {}_{}_t {};\n", pkg_prefix, short, fname));
                            } else {
                                h.push_str(&format!("    struct {}_{}* {};\n", pkg_prefix, short, fname));
                            }
                        }
                        (FieldType::Map(_, _), _) => {
                            h.push_str(&format!("    void* {}_entries;\n    size_t {}_count;\n", fname, fname));
                        }
                    }
                }
            }
            h.push_str(&format!("}} {};\n\n", st_name));
            h.push_str(&format!(
                "void {}_{}_init({}* msg);\n\n",
                pkg_prefix,
                to_snake_case(&msg.name),
                st_name
            ));

            c_src.push_str(&format!(
                "void {}_{}_init({}* msg) {{\n    if (msg) memset(msg, 0, sizeof(*msg));\n}}\n\n",
                pkg_prefix,
                to_snake_case(&msg.name),
                st_name
            ));
        }
    }

    if cfg.generate_grpc && !file.services.is_empty() {
        for svc in &file.services {
            let svc_c = format!("{}_{}_service_t", pkg_prefix, to_snake_case(&svc.name));
            h.push_str(&format!("/* gRPC Service VTable for {} */\ntypedef struct {{\n", svc.full_name));
            for rpc in &svc.methods {
                let m_c = to_snake_case(&rpc.name);
                let in_c = format!("{}_{}_t", pkg_prefix, to_snake_case(rpc.input_short()));
                let out_c = format!("{}_{}_t", pkg_prefix, to_snake_case(rpc.output_short()));
                h.push_str(&format!(
                    "    int (*{})(void* ctx, const {}* req, {}* resp);\n",
                    m_c, in_c, out_c
                ));
            }
            h.push_str(&format!("}} {};\n\n", svc_c));
        }
    }

    h.push_str("#ifdef __cplusplus\n}\n#endif\n\n#endif\n");
    files.push((format!("{}.pb.h", stem), h));
    files.push((format!("{}.pb.c", stem), c_src));
    files
}
