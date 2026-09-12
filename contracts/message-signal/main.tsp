import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace MessagingIntelSignal;

@id("MessageSignalV1")
model MessageSignalV1 {
  @minValue(1)
  @maxValue(1)
  version: integer;

  @minLength(1)
  @maxLength(160)
  message_id: string;

  @minValue(0)
  @maxValue(1)
  confidence: float64;

  @minValue(0)
  @maxValue(100000)
  token_count: integer;

  contains_pii: boolean;
}
