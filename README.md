# Grpc example to test new field presence

since protobuf 3.15 it is not experimental anymore to use the modifier `optional` in fields of messages, which allows field presence checks for basic numeric types and strings, in addition to sub-messages (which already worked before)