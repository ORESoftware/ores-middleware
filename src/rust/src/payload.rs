use serde::{Deserialize, Serialize};

/// Shared payload-policy schema identity owned by `ORESoftware/ores-interfaces`.
pub const RPC_PAYLOAD_POLICY_SCHEMA: &str = "ores.rpc-payload-policy/v1";

/// Rust projection of `Ores.RpcPayload.PayloadCodec`.
///
/// Keep the serialized spellings byte-for-byte aligned with the independent
/// TypeSpec and Draft 2020-12 authorities in `ores-interfaces`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadCodec {
    Json,
    Messagepack,
    Cbor,
    Protobuf,
    Raw,
}

impl PayloadCodec {
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Messagepack => "messagepack",
            Self::Cbor => "cbor",
            Self::Protobuf => "protobuf",
            Self::Raw => "raw",
        }
    }

    #[must_use]
    pub const fn requires_opaque_byte_preservation(self) -> bool {
        matches!(self, Self::Protobuf | Self::Raw)
    }
}

/// Rust projection of `Ores.RpcPayload.TransportFraming`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportFraming {
    HttpBody,
    TcpLengthDelimited,
    WebsocketMessage,
}

impl TransportFraming {
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::HttpBody => "http_body",
            Self::TcpLengthDelimited => "tcp_length_delimited",
            Self::WebsocketMessage => "websocket_message",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PayloadMetadata {
    pub codec: PayloadCodec,
    pub framing: TransportFraming,
    pub media_type: Option<String>,
    pub encoded_bytes: usize,
    pub decoded_bytes: Option<usize>,
    pub preserve_opaque_bytes: bool,
}

impl PayloadMetadata {
    pub fn validate(
        &self,
        max_encoded_bytes: usize,
        max_decoded_bytes: usize,
    ) -> Result<(), &'static str> {
        if max_encoded_bytes == 0 || max_decoded_bytes == 0 {
            return Err("payload limits must be positive");
        }
        if self.encoded_bytes > max_encoded_bytes {
            return Err("encoded payload exceeds configured limit");
        }
        if self
            .decoded_bytes
            .is_some_and(|value| value > max_decoded_bytes)
        {
            return Err("decoded payload exceeds configured limit");
        }
        if self.codec.requires_opaque_byte_preservation() && !self.preserve_opaque_bytes {
            return Err("protobuf/raw payloads require opaque-byte preservation");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_projection_uses_shared_authority_wire_names() {
        let cases = [
            (PayloadCodec::Json, "json"),
            (PayloadCodec::Messagepack, "messagepack"),
            (PayloadCodec::Cbor, "cbor"),
            (PayloadCodec::Protobuf, "protobuf"),
            (PayloadCodec::Raw, "raw"),
        ];

        for (codec, expected) in cases {
            assert_eq!(codec.wire_name(), expected);
            assert_eq!(
                serde_json::to_string(&codec).expect("serialize codec"),
                format!("\"{expected}\"")
            );
            assert_eq!(
                serde_json::from_str::<PayloadCodec>(&format!("\"{expected}\""))
                    .expect("deserialize codec"),
                codec,
            );
        }
    }

    #[test]
    fn framing_projection_uses_shared_authority_wire_names() {
        let cases = [
            (TransportFraming::HttpBody, "http_body"),
            (TransportFraming::TcpLengthDelimited, "tcp_length_delimited"),
            (TransportFraming::WebsocketMessage, "websocket_message"),
        ];

        for (framing, expected) in cases {
            assert_eq!(framing.wire_name(), expected);
            assert_eq!(
                serde_json::to_string(&framing).expect("serialize framing"),
                format!("\"{expected}\"")
            );
            assert_eq!(
                serde_json::from_str::<TransportFraming>(&format!("\"{expected}\""))
                    .expect("deserialize framing"),
                framing,
            );
        }
    }

    #[test]
    fn unknown_codec_and_framing_names_fail_closed() {
        assert!(serde_json::from_str::<PayloadCodec>("\"MessagePack\"").is_err());
        assert!(serde_json::from_str::<PayloadCodec>("\"msgpack\"").is_err());
        assert!(serde_json::from_str::<TransportFraming>("\"ndjson\"").is_err());
    }

    #[test]
    fn opaque_binary_payloads_fail_closed_when_marked_as_text_transformable() {
        for codec in [PayloadCodec::Raw, PayloadCodec::Protobuf] {
            let payload = PayloadMetadata {
                codec,
                framing: TransportFraming::HttpBody,
                media_type: Some("application/octet-stream".into()),
                encoded_bytes: 4,
                decoded_bytes: None,
                preserve_opaque_bytes: false,
            };
            assert!(payload.validate(1024, 4096).is_err());
        }
    }

    #[test]
    fn encoded_and_decoded_limits_are_independent() {
        let mut payload = PayloadMetadata {
            codec: PayloadCodec::Cbor,
            framing: TransportFraming::TcpLengthDelimited,
            media_type: Some("application/cbor".into()),
            encoded_bytes: 100,
            decoded_bytes: Some(200),
            preserve_opaque_bytes: false,
        };
        assert!(payload.validate(100, 200).is_ok());
        payload.decoded_bytes = Some(201);
        assert!(payload.validate(100, 200).is_err());
    }

    #[test]
    fn zero_limits_are_not_valid_policy_inputs() {
        let payload = PayloadMetadata {
            codec: PayloadCodec::Json,
            framing: TransportFraming::HttpBody,
            media_type: Some("application/json".into()),
            encoded_bytes: 0,
            decoded_bytes: Some(0),
            preserve_opaque_bytes: false,
        };

        assert!(payload.validate(0, 1).is_err());
        assert!(payload.validate(1, 0).is_err());
    }
}
