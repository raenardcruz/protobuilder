/// Embedded Google Well-Known Types (WKT) virtual filesystem.
/// Allows `import "google/protobuf/timestamp.proto";` etc. to resolve out-of-the-box
/// with zero external dependencies or system `/usr/include` headers installed.
pub fn lookup_wkt_proto(path: &str) -> Option<&'static str> {
    let normalized = path.trim().replace('\\', "/");
    match normalized.as_str() {
        "google/protobuf/empty.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/emptypb";
option java_package = "com.google.protobuf";
option java_outer_classname = "EmptyProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

// A generic empty message that you can re-use to avoid defining duplicated empty messages.
message Empty {}
"#,
        ),
        "google/protobuf/timestamp.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/timestamppb";
option java_package = "com.google.protobuf";
option java_outer_classname = "TimestampProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

// A Timestamp represents a point in time independent of any time zone or local calendar.
message Timestamp {
  int64 seconds = 1;
  int32 nanos = 2;
}
"#,
        ),
        "google/protobuf/duration.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/durationpb";
option java_package = "com.google.protobuf";
option java_outer_classname = "DurationProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

// A Duration represents a signed, fixed-length span of time.
message Duration {
  int64 seconds = 1;
  int32 nanos = 2;
}
"#,
        ),
        "google/protobuf/wrappers.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/wrapperspb";
option java_package = "com.google.protobuf";
option java_outer_classname = "WrappersProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

message DoubleValue { double value = 1; }
message FloatValue { float value = 1; }
message Int64Value { int64 value = 1; }
message UInt64Value { uint64 value = 1; }
message Int32Value { int32 value = 1; }
message UInt32Value { uint32 value = 1; }
message BoolValue { bool value = 1; }
message StringValue { string value = 1; }
message BytesValue { bytes value = 1; }
"#,
        ),
        "google/protobuf/any.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/anypb";
option java_package = "com.google.protobuf";
option java_outer_classname = "AnyProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

message Any {
  string type_url = 1;
  bytes value = 2;
}
"#,
        ),
        "google/protobuf/field_mask.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/fieldmaskpb";
option java_package = "com.google.protobuf";
option java_outer_classname = "FieldMaskProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

message FieldMask {
  repeated string paths = 1;
}
"#,
        ),
        "google/protobuf/struct.proto" => Some(
            r#"
syntax = "proto3";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/known/structpb";
option java_package = "com.google.protobuf";
option java_outer_classname = "StructProto";
option csharp_namespace = "Google.Protobuf.WellKnownTypes";
option objc_class_prefix = "GPB";

enum NullValue {
  NULL_VALUE = 0;
}

message Struct {
  map<string, Value> fields = 1;
}

message Value {
  oneof kind {
    NullValue null_value = 1;
    double number_value = 2;
    string string_value = 3;
    bool bool_value = 4;
    Struct struct_value = 5;
    ListValue list_value = 6;
  }
}

message ListValue {
  repeated Value values = 1;
}
"#,
        ),
        "google/protobuf/descriptor.proto" => Some(
            r#"
syntax = "proto2";
package google.protobuf;
option go_package = "google.golang.org/protobuf/types/descriptorpb";
option java_package = "com.google.protobuf";
option java_outer_classname = "DescriptorProtos";

message FileDescriptorSet {
  repeated FileDescriptorProto file = 1;
}
message FileDescriptorProto {
  optional string name = 1;
  optional string package = 2;
  repeated string dependency = 3;
}
"#,
        ),
        _ => None,
    }
}
