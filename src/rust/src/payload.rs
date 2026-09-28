#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadCodec {
    Json,
    Messagepack,
    Cbor,
    Protobuf,
    Raw,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportFraming {
    HttpBody,
    TcpLengthDelimited,
    WebsocketMessage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadMetadata {
    pub codec: PayloadCodec,
    pub framing: TransportFraming,
    pub media_type: Option<String>,
    pub encoded_bytes: usize,
    pub decoded_bytes: Option<usize>,
    pub preserve_opaque_bytes: bool,
}

impl PayloadMetadata {
    pub fn validate(&self, max_encoded_bytes: usize, max_decoded_bytes: usize) -> Result<(), &'static str> {
        if self.encoded_bytes > max_encoded_bytes {
            return Err("encoded payload exceeds configured limit");
        }
        if self.decoded_bytes.is_some_and(|value| value > max_decoded_bytes) {
            return Err("decoded payload exceeds configured limit");
        }
        if matches!(self.codec, PayloadCodec::Protobuf | PayloadCodec::Raw)
            && !self.preserve_opaque_bytes
        {
            return Err("protobuf/raw payloads require opaque-byte preservation");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_binary_payloads_fail_closed_when_marked_as_text_transformable() {
        let payload = PayloadMetadata {
            codec: PayloadCodec::Raw,
            framing: TransportFraming::HttpBody,
            media_type: Some("application/octet-stream".into()),
            encoded_bytes: 4,
            decoded_bytes: None,
            preserve_opaque_bytes: false,
        };
        assert!(payload.validate(1024, 4096).is_err());
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
}
