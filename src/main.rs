mod ast;
mod cli;
mod codegen;
mod parser;
mod wkt;

use crate::ast::*;
use crate::cli::*;
use crate::codegen::*;
use crate::parser::*;
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use std::time::Instant;

fn main() {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let action = match parse_cli_args(&raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {}\nRun 'protobuilder --help' for usage information.", e);
            process::exit(1);
        }
    };

    match action.mode {
        CommandMode::Help { ref topic } => {
            println!("{}", render_help(topic.as_deref()));
        }
        CommandMode::Version => {
            println!(
                "protobuilder 1.0.0 (single-executable, 15 built-in language & gRPC generators)"
            );
        }
        CommandMode::ListLanguages => {
            println!("{}", render_languages_table());
        }
        CommandMode::Init { ref dir } => {
            if let Err(e) = run_init(dir) {
                eprintln!("error: {}", e);
                process::exit(2);
            }
        }
        CommandMode::Inspect => {
            if let Err((code, msg)) = run_inspect(&action) {
                eprintln!("error: {}", msg);
                process::exit(code);
            }
        }
        CommandMode::Validate => {
            if let Err((code, msg)) = run_validate(&action) {
                eprintln!("error: {}", msg);
                process::exit(code);
            }
        }
        CommandMode::Build => {
            if let Err((code, msg)) = run_build(&action) {
                eprintln!("error: {}", msg);
                process::exit(code);
            }
        }
    }
}

fn run_init(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create directory '{}': {}", dir.display(), e))?;
    let protos_dir = dir.join("protos");
    fs::create_dir_all(&protos_dir)
        .map_err(|e| format!("Failed to create '{}': {}", protos_dir.display(), e))?;

    let sample_proto = protos_dir.join("service.proto");
    if !sample_proto.exists() {
        fs::write(
            &sample_proto,
            r#"syntax = "proto3";
package acme.orders.v1;

import "google/protobuf/timestamp.proto";

option go_package = "github.com/acme/orders/v1;ordersv1";
option java_package = "com.acme.orders.v1";
option java_outer_classname = "OrdersProto";
option csharp_namespace = "Acme.Orders.V1";

// Order lifecycle status
enum OrderStatus {
  ORDER_STATUS_UNSPECIFIED = 0;
  ORDER_STATUS_PENDING = 1;
  ORDER_STATUS_PAID = 2;
  ORDER_STATUS_SHIPPED = 3;
  ORDER_STATUS_CANCELLED = 4;
}

// Line item inside an order
message OrderItem {
  string sku = 1;
  string title = 2;
  int32 quantity = 3;
  double unit_price = 4;
}

// Request to create a new order
message CreateOrderRequest {
  string customer_id = 1;
  repeated OrderItem items = 2;
  map<string, string> metadata = 3;
  oneof payment_method {
    string card_token = 4;
    string invoice_po = 5;
  }
}

// Order record returned by the service
message OrderResponse {
  string order_id = 1;
  OrderStatus status = 2;
  double total_amount = 3;
  google.protobuf.Timestamp created_at = 4;
}

// gRPC service supporting unary, server-streaming, client-streaming, and bidi-streaming RPCs
service OrderService {
  // Unary RPC: Create a single order
  rpc CreateOrder (CreateOrderRequest) returns (OrderResponse);

  // Server-streaming RPC: Stream real-time order status updates
  rpc WatchOrders (CreateOrderRequest) returns (stream OrderResponse);

  // Client-streaming RPC: Batch upload multiple orders
  rpc BatchCreateOrders (stream CreateOrderRequest) returns (OrderResponse);

  // Bidirectional-streaming RPC: Interactive order fulfillment stream
  rpc LiveOrderStream (stream CreateOrderRequest) returns (stream OrderResponse);
}
"#,
        )
        .map_err(|e| format!("Failed to write sample proto: {}", e))?;
    }

    let toml_path = dir.join("protobuilder.toml");
    if !toml_path.exists() {
        fs::write(
            &toml_path,
            r#"# protobuilder.toml — Project configuration for ProtoBuilder
inputs    = ["protos/service.proto"]
includes  = ["protos"]
languages = ["all"]
out_dir   = "./gen"
grpc      = true
messages  = true
"#,
        )
        .map_err(|e| format!("Failed to write protobuilder.toml: {}", e))?;
    }

    println!(
        "Initialized ProtoBuilder project in '{}':\n  • Created {}\n  • Created {}\n\nRun `protobuilder build` to generate code for all 15 languages!",
        dir.display(),
        sample_proto.display(),
        toml_path.display()
    );
    Ok(())
}

fn load_bundle_from_action(action: &CliAction) -> Result<SchemaBundle, (i32, String)> {
    let files = discover_proto_files(&action.inputs).map_err(|e| (2, e))?;
    load_and_resolve_bundle(&files, &action.include_dirs).map_err(|e| (3, e))
}

fn check_diagnostics(bundle: &SchemaBundle, quiet: bool) -> Result<(), (i32, String)> {
    let mut has_err = false;
    for d in &bundle.diagnostics {
        match d.level {
            DiagnosticLevel::Error => {
                has_err = true;
                eprintln!("[ERROR] {}: {}", d.file, d.message);
            }
            DiagnosticLevel::Warning => {
                if !quiet {
                    eprintln!("[WARN]  {}: {}", d.file, d.message);
                }
            }
        }
    }
    if has_err {
        Err((3, "Schema validation failed with errors".to_string()))
    } else {
        Ok(())
    }
}

fn run_validate(action: &CliAction) -> Result<(), (i32, String)> {
    let start = Instant::now();
    let bundle = load_bundle_from_action(action)?;
    check_diagnostics(&bundle, action.quiet)?;
    let user_files: Vec<&ProtoFile> = bundle.files.iter().filter(|f| !f.is_wkt).collect();
    if !action.quiet {
        println!(
            "✓ Validated {} .proto file(s) ({} total types registered, {} warnings) in {:.2?}",
            user_files.len(),
            bundle.type_registry.len(),
            bundle.diagnostics.len(),
            start.elapsed()
        );
    }
    Ok(())
}

fn run_inspect(action: &CliAction) -> Result<(), (i32, String)> {
    let bundle = load_bundle_from_action(action)?;
    check_diagnostics(&bundle, action.quiet)?;

    for file in bundle.files.iter().filter(|f| !f.is_wkt) {
        println!("================================================================================");
        println!("FILE: {} ({})", file.name, file.source_path.display());
        println!(
            "  Syntax:  {}\n  Package: {}",
            file.syntax.as_str(),
            if file.package.is_empty() {
                "<none>"
            } else {
                &file.package
            }
        );
        if !file.imports.is_empty() {
            let imps: Vec<&str> = file.imports.iter().map(|i| i.path.as_str()).collect();
            println!("  Imports: {}", imps.join(", "));
        }
        if !file.options.is_empty() {
            println!("  Options:");
            for opt in &file.options {
                println!("    • {} = \"{}\"", opt.name, opt.value);
            }
        }
        println!("  Enums ({}):", file.all_enums_flat().len());
        for en in file.all_enums_flat() {
            let vals: Vec<String> = en
                .values
                .iter()
                .map(|v| format!("{}={}", v.name, v.number))
                .collect();
            println!("    • {} [{}]", en.full_name, vals.join(", "));
        }
        println!("  Messages ({}):", file.all_messages_flat().len());
        for msg in file.all_messages_flat() {
            println!(
                "    • {} ({} fields, {} oneofs)",
                msg.full_name,
                msg.fields.len(),
                msg.oneofs.len()
            );
            for f in msg.all_fields() {
                let oneof_tag = f
                    .oneof_name
                    .as_ref()
                    .map(|o| format!(" [oneof: {}]", o))
                    .unwrap_or_default();
                println!(
                    "        #{:<4} {:<18} : {}{}",
                    f.number,
                    f.name,
                    f.field_type.short_name(),
                    oneof_tag
                );
            }
        }
        println!("  gRPC Services ({}):", file.services.len());
        for svc in &file.services {
            println!("    • {} ({} RPCs)", svc.full_name, svc.methods.len());
            for rpc in &svc.methods {
                println!(
                    "        rpc {:<22} ({}{}) returns ({}{}) [{}]",
                    rpc.name,
                    if rpc.client_streaming { "stream " } else { "" },
                    rpc.input_type,
                    if rpc.server_streaming { "stream " } else { "" },
                    rpc.output_type,
                    rpc.streaming_mode()
                );
            }
        }
    }
    Ok(())
}

fn run_build(action: &CliAction) -> Result<(), (i32, String)> {
    let start = Instant::now();
    let bundle = load_bundle_from_action(action)?;
    check_diagnostics(&bundle, action.quiet)?;

    if action.clean_out && !action.dry_run && action.out_dir.exists() {
        fs::remove_dir_all(&action.out_dir).map_err(|e| {
            (
                2,
                format!(
                    "Failed to clean output directory '{}': {}",
                    action.out_dir.display(),
                    e
                ),
            )
        })?;
    }

    let gen_cfg = GenConfig {
        languages: action.languages.clone(),
        out_dir: action.out_dir.clone(),
        generate_messages: action.generate_messages,
        generate_grpc: action.generate_grpc,
        dry_run: action.dry_run,
    };

    let artifacts = run_codegen(&bundle, &gen_cfg).map_err(|e| (2, e))?;
    let total_bytes: usize = artifacts.iter().map(|a| a.bytes).sum();
    let user_proto_count = bundle.files.iter().filter(|f| !f.is_wkt).count();

    if !action.quiet {
        let mode_label = match (action.generate_messages, action.generate_grpc) {
            (true, true) => "Protobuf Messages + gRPC Stubs",
            (true, false) => "Protobuf Messages Only",
            (false, true) => "gRPC Stubs Only",
            (false, false) => "No Output",
        };
        let dry_prefix = if action.dry_run { "[DRY-RUN] " } else { "" };
        println!(
            "✓ {}Generated {} file(s) ({} bytes) across {} language(s) from {} .proto schema(s) [{}] in {:.2?}",
            dry_prefix,
            artifacts.len(),
            total_bytes,
            action.languages.len(),
            user_proto_count,
            mode_label,
            start.elapsed()
        );
        if action.verbose {
            for art in &artifacts {
                println!(
                    "  • [{:<10}] {} ({} bytes)",
                    art.lang.canonical_id(),
                    art.output_path.display(),
                    art.bytes
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const SAMPLE_PROTO: &str = r#"
syntax = "proto3";
package test.payment.v1;

import "google/protobuf/timestamp.proto";
import "google/protobuf/empty.proto";

option go_package = "example.com/payment/v1;paymentv1";

enum Currency {
  CURRENCY_UNSPECIFIED = 0;
  CURRENCY_USD = 1;
  CURRENCY_EUR = 2;
}

message Money {
  Currency currency = 1;
  int64 units = 2;
  int32 nanos = 3;
}

message ChargeRequest {
  string account_id = 1;
  Money amount = 2;
  repeated string tags = 3;
  map<string, string> labels = 4;
  oneof method {
    string card_id = 5;
    string bank_iban = 6;
  }
  google.protobuf.Timestamp requested_at = 7;
}

message ChargeResponse {
  string transaction_id = 1;
  bool approved = 2;
}

service PaymentGateway {
  rpc Charge (ChargeRequest) returns (ChargeResponse);
  rpc StreamCharges (ChargeRequest) returns (stream ChargeResponse);
  rpc BatchCharge (stream ChargeRequest) returns (ChargeResponse);
  rpc BidiCharge (stream ChargeRequest) returns (stream ChargeResponse);
}
"#;

    #[test]
    fn test_parse_and_generate_all_15_languages() {
        let pf = parse_proto_str(
            SAMPLE_PROTO,
            "payment.proto",
            PathBuf::from("payment.proto"),
            false,
        )
        .expect("should parse sample proto");
        assert_eq!(pf.package, "test.payment.v1");
        assert_eq!(pf.enums.len(), 1);
        assert_eq!(pf.messages.len(), 3);
        assert_eq!(pf.services.len(), 1);
        assert_eq!(pf.services[0].methods.len(), 4);
        assert_eq!(pf.services[0].methods[0].streaming_mode(), "Unary");
        assert_eq!(pf.services[0].methods[1].streaming_mode(), "ServerStreaming");
        assert_eq!(pf.services[0].methods[2].streaming_mode(), "ClientStreaming");
        assert_eq!(pf.services[0].methods[3].streaming_mode(), "BidiStreaming");

        let cfg = GenConfig {
            languages: TargetLang::all().to_vec(),
            out_dir: PathBuf::from("./test_out"),
            generate_messages: true,
            generate_grpc: true,
            dry_run: true,
        };

        let bundle = SchemaBundle {
            files: vec![pf],
            type_registry: std::collections::BTreeMap::new(),
            diagnostics: Vec::new(),
        };

        let artifacts = run_codegen(&bundle, &cfg).expect("codegen should succeed");
        assert!(
            artifacts.len() >= 25,
            "expected at least 25 generated artifacts across 15 languages, got {}",
            artifacts.len()
        );
        for lang in TargetLang::all() {
            let has_lang = artifacts.iter().any(|a| a.lang == *lang);
            assert!(has_lang, "missing generated output for {:?}", lang);
        }
    }

    #[test]
    fn test_simplified_cli_parsing() {
        let args = vec![
            "service.proto".to_string(),
            "-l".to_string(),
            "rust,go,ts".to_string(),
            "-o".to_string(),
            "./out".to_string(),
        ];
        let act = parse_cli_args(&args).expect("parse cli");
        assert_eq!(act.languages.len(), 3);
        assert_eq!(act.out_dir, PathBuf::from("./out"));
    }
}
