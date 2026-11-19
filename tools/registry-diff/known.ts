// The steps where doze answers otherwise than Confluent 7.7.1 on purpose or by a recorded gap: "scenario/step" -> the
// deviation (docs/port/behaviours/registry.md). A difference not listed here fails the run. Run against a fresh stack
// (stack.sh up): Confluent's own answers change once a context subject has been registered.
const words = "REG-D27: the parser's own words after `details:` (Jackson's, Wire's) are doze's, which name the place";
export const known: Record<string, string> = {
  "root-and-unknown-routes/13": "REG-D5: /v1/metadata/id is not served",
  "root-and-unknown-routes/14": "REG-D5: /v1/metadata/version is not served",
  "register-and-read-avro/34": "REG-D6: Confluent answers 500 for /schemas/ids/{unknown}/schema",
  "register-and-read-avro/54": words,
  "register-errors/8": words,
  "metadata-and-rules-in-a-registration/0": "REG-D26: metadata and rule sets are read past, not kept",
  "metadata-and-rules-in-a-registration/1": "REG-D26",
  "metadata-and-rules-in-a-registration/2": "REG-D26",
  "metadata-and-rules-in-a-registration/3": "REG-D26",
  "metadata-and-rules-in-a-registration/4": "REG-D26",
  "metadata-and-rules-in-a-registration/5": "REG-D26: a lookup does not compare metadata",
  "mode/17": "REG-D6: Confluent answers 500 for a mode body without a mode",
  "compatibility-route/13": words,
  "protobuf/11": words,
  "protobuf/12": words,
  "protobuf/13": "REG-D15: a field number used twice is refused, as protoc refuses it (Confluent takes it)",
  "protobuf/24": "REG-D28: ?normalize=true does not normalize a Protobuf schema",
  "json-schema/5": "REG-D6: Confluent answers 500 for format=serialized on a JSON schema",
  "json-schema/8": words,
  "json-schema/9": words,
  "json-schema/12": words,
  "json-schema/17": words,
  "avro-text-forms/7": "REG-D6: Confluent answers 500 for this normalized lookup",
  "subject-names/9": "REG-D3: contexts are not offered; Confluent gives a context subject an id of its context",
  "subject-names/15": "REG-D3: follows from step 9",
};
