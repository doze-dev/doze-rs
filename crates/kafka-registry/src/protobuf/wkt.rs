//! The well-known types, which a schema imports without a reference (`import "google/protobuf/timestamp.proto";`), as
//! Confluent's registry and the Go edition's compiler provide them. Written compactly: every message, enum and field
//! of the files, except `descriptor.proto`, whose messages are declared without their fields (enough to name them
//! and to extend the options messages, which is what schemas do with it).

/// A proto3 file of package google.protobuf.
macro_rules! p3 {
    ($($s:literal)+) => {
        concat!("syntax=\"proto3\";package google.protobuf;", $($s),+)
    };
}

const FILES: &[(&str, &str)] = &[
    ("any", p3!("message Any{string type_url=1;bytes value=2;}")),
    ("duration", p3!("message Duration{int64 seconds=1;int32 nanos=2;}")),
    ("timestamp", p3!("message Timestamp{int64 seconds=1;int32 nanos=2;}")),
    ("empty", p3!("message Empty{}")),
    ("field_mask", p3!("message FieldMask{repeated string paths=1;}")),
    ("source_context", p3!("message SourceContext{string file_name=1;}")),
    (
        "struct",
        p3!("message Struct{map<string,Value> fields=1;}message Value{oneof kind{NullValue null_value=1;\
double number_value=2;string string_value=3;bool bool_value=4;Struct struct_value=5;ListValue list_value=6;}}\
enum NullValue{NULL_VALUE=0;}message ListValue{repeated Value values=1;}"),
    ),
    (
        "wrappers",
        p3!("message DoubleValue{double value=1;}message FloatValue{float value=1;}message Int64Value{int64 value=1;}\
message UInt64Value{uint64 value=1;}message Int32Value{int32 value=1;}message UInt32Value{uint32 value=1;}\
message BoolValue{bool value=1;}message StringValue{string value=1;}message BytesValue{bytes value=1;}"),
    ),
    (
        "type",
        p3!("import \"google/protobuf/any.proto\";import \"google/protobuf/source_context.proto\";\
message Type{string name=1;repeated Field fields=2;repeated string oneofs=3;repeated Option options=4;\
SourceContext source_context=5;Syntax syntax=6;string edition=7;}\
message Field{enum Kind{TYPE_UNKNOWN=0;TYPE_DOUBLE=1;TYPE_FLOAT=2;TYPE_INT64=3;TYPE_UINT64=4;TYPE_INT32=5;\
TYPE_FIXED64=6;TYPE_FIXED32=7;TYPE_BOOL=8;TYPE_STRING=9;TYPE_GROUP=10;TYPE_MESSAGE=11;TYPE_BYTES=12;\
TYPE_UINT32=13;TYPE_ENUM=14;TYPE_SFIXED32=15;TYPE_SFIXED64=16;TYPE_SINT32=17;TYPE_SINT64=18;}\
enum Cardinality{CARDINALITY_UNKNOWN=0;CARDINALITY_OPTIONAL=1;CARDINALITY_REQUIRED=2;CARDINALITY_REPEATED=3;}\
Kind kind=1;Cardinality cardinality=2;int32 number=3;string name=4;string type_url=6;int32 oneof_index=7;\
bool packed=8;repeated Option options=9;string json_name=10;string default_value=11;}\
message Enum{string name=1;repeated EnumValue enumvalue=2;repeated Option options=3;\
SourceContext source_context=4;Syntax syntax=5;string edition=6;}\
message EnumValue{string name=1;int32 number=2;repeated Option options=3;}\
message Option{string name=1;Any value=2;}enum Syntax{SYNTAX_PROTO2=0;SYNTAX_PROTO3=1;SYNTAX_EDITIONS=2;}"),
    ),
    (
        "api",
        p3!("import \"google/protobuf/source_context.proto\";import \"google/protobuf/type.proto\";\
message Api{string name=1;repeated Method methods=2;repeated Option options=3;string version=4;\
SourceContext source_context=5;repeated Mixin mixins=6;Syntax syntax=7;}\
message Method{string name=1;string request_type_url=2;bool request_streaming=3;string response_type_url=4;\
bool response_streaming=5;repeated Option options=6;Syntax syntax=7;}message Mixin{string name=1;string root=2;}"),
    ),
];

const DESCRIPTOR: &str = "package google.protobuf;message FileDescriptorSet{}message FileDescriptorProto{}\
message DescriptorProto{}message FieldDescriptorProto{}message OneofDescriptorProto{}\
message EnumDescriptorProto{}message EnumValueDescriptorProto{}message ServiceDescriptorProto{}\
message MethodDescriptorProto{}message UninterpretedOption{}message FeatureSet{}message FeatureSetDefaults{}\
message SourceCodeInfo{}message GeneratedCodeInfo{}\
message FileOptions{extensions 1000 to max;}message MessageOptions{extensions 1000 to max;}\
message FieldOptions{extensions 1000 to max;}message OneofOptions{extensions 1000 to max;}\
message EnumOptions{extensions 1000 to max;}message EnumValueOptions{extensions 1000 to max;}\
message ServiceOptions{extensions 1000 to max;}message MethodOptions{extensions 1000 to max;}\
message ExtensionRangeOptions{extensions 1000 to max;}";

/// The text of a well-known file, by its import path.
pub(super) fn source(path: &str) -> Option<&'static str> {
    let name = path.strip_prefix("google/protobuf/")?.strip_suffix(".proto")?;
    if name == "descriptor" {
        return Some(DESCRIPTOR);
    }
    FILES.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}
