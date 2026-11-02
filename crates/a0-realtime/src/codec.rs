//! Wire codec for the subset of engine.io v4 (EIO=4) and Socket.IO v3 that
//! Agent Zero uses. Framing follows the reference client's serializer
//! (`SocketPacket.createPacketString`): packet type digit, namespace (`/nsp,`
//! when not root), ack id (no separator), then JSON data.
//!
//! Engine-level frames are single characters (`0` open, `1` close, `2` ping,
//! `3` pong); Socket.IO packets are engine messages: a leading `4` followed
//! by the Socket.IO packet (`40/ws,{...}`, `42/ws,[...]`, `43/ws,<id>[...]`).

use serde_json::Value as JSONValue;

use crate::error::{RealtimeError, Result};

/// engine.io `0` open handshake payload (fields Agent Zero consumes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPacket {
    /// Server-assigned engine session id.
    pub sid: String,
    /// Server ping interval in milliseconds (`pingInterval`).
    pub ping_interval_ms: u64,
    /// Server ping timeout in milliseconds (`pingTimeout`).
    pub ping_timeout_ms: u64,
}

/// Socket.IO packet (types 0-4; binary types 5/6 are unsupported).
#[derive(Debug, Clone, PartialEq)]
pub enum SocketPacket {
    /// `40[/<nsp>[,<payload>]]` — namespace connect (client auth object or
    /// server ack payload).
    Connect {
        nsp: String,
        payload: Option<JSONValue>,
    },
    /// `41[/<nsp>]` — namespace disconnect.
    Disconnect { nsp: String },
    /// `42[/<nsp>,<id>][<json array>]` — event.
    Event { nsp: String, data: Vec<JSONValue> },
    /// `43[/<nsp>,]<id>[<json array>]` — ack for a previous emit.
    Ack {
        nsp: String,
        id: u64,
        data: Vec<JSONValue>,
    },
    /// `44[/<nsp>]<payload>` — connect error.
    ConnectError { nsp: String, message: String },
}

/// One engine.io frame (post-TLS, pre-Socket.IO).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineFrame {
    /// `0{...}` open handshake.
    Open(OpenPacket),
    /// `1` close.
    Close,
    /// `2` server ping (EIO=4: server pings, client pongs).
    Ping,
    /// `3` pong.
    Pong,
    /// `4<socket.io packet>` application message.
    Message(SocketPacket),
}

fn parse_socket(rest: &str) -> Result<SocketPacket> {
    let bytes = rest.as_bytes();
    let Some(&type_byte) = bytes.first() else {
        return Err(RealtimeError::Codec("empty socket.io packet".into()));
    };
    let packet_type = (type_byte as char).to_digit(10).ok_or_else(|| {
        RealtimeError::Codec(format!("invalid socket.io type byte {type_byte:#x}"))
    })?;
    let rest = &rest[1..];

    // Namespace: leading `/` up to the first `,` (comma consumed). Without a
    // comma the whole remainder is the namespace (e.g. `41/ws`).
    let (nsp, rest) = if rest.starts_with('/') {
        match rest.find(',') {
            Some(idx) => (rest[..idx].to_string(), &rest[idx + 1..]),
            None => (rest.to_string(), ""),
        }
    } else {
        ("/".to_string(), rest)
    };

    match packet_type {
        0 => {
            let payload = if rest.is_empty() {
                None
            } else {
                Some(
                    serde_json::from_str(rest)
                        .map_err(|e| RealtimeError::Codec(format!("connect payload: {e}")))?,
                )
            };
            Ok(SocketPacket::Connect { nsp, payload })
        }
        1 => Ok(SocketPacket::Disconnect { nsp }),
        2 | 3 => {
            // Leading digits form the ack id (mandatory for ack, optional for
            // event packets); the remainder is a JSON array of data items.
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            let rest = &rest[digits.len()..];
            let id: u64 = digits.parse().unwrap_or(0);
            let data = if rest.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(rest)
                    .map_err(|e| RealtimeError::Codec(format!("packet data: {e}")))?
            };
            if packet_type == 2 {
                Ok(SocketPacket::Event { nsp, data })
            } else {
                Ok(SocketPacket::Ack { nsp, id, data })
            }
        }
        4 => {
            // Connect error: payload may be a JSON object with `message` or a
            // bare string; surface a readable message either way.
            let message = match serde_json::from_str::<JSONValue>(rest) {
                Ok(JSONValue::Object(map)) => map
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(rest)
                    .to_string(),
                Ok(JSONValue::String(s)) => s,
                _ => rest.to_string(),
            };
            Ok(SocketPacket::ConnectError { nsp, message })
        }
        5 | 6 => Err(RealtimeError::Codec(
            "binary socket.io packets unsupported".into(),
        )),
        other => Err(RealtimeError::Codec(format!(
            "invalid socket.io packet type {other}"
        ))),
    }
}

/// Parses one inbound engine.io text frame.
pub fn parse_frame(text: &str) -> Result<EngineFrame> {
    let Some(engine_type) = text.chars().next().and_then(|c| c.to_digit(10)) else {
        return Err(RealtimeError::Codec("empty engine.io frame".into()));
    };
    let rest = &text[1..];
    match engine_type {
        0 => {
            let value: JSONValue = serde_json::from_str(rest)
                .map_err(|e| RealtimeError::Codec(format!("open payload: {e}")))?;
            let sid = value
                .get("sid")
                .and_then(|s| s.as_str())
                .ok_or_else(|| RealtimeError::Codec("open: missing sid".into()))?
                .to_string();
            let ping_interval_ms = value
                .get("pingInterval")
                .and_then(|v| v.as_u64())
                .unwrap_or(25_000);
            let ping_timeout_ms = value
                .get("pingTimeout")
                .and_then(|v| v.as_u64())
                .unwrap_or(5_000);
            Ok(EngineFrame::Open(OpenPacket {
                sid,
                ping_interval_ms,
                ping_timeout_ms,
            }))
        }
        1 => Ok(EngineFrame::Close),
        2 => Ok(EngineFrame::Ping),
        3 => Ok(EngineFrame::Pong),
        4 => parse_socket(rest).map(EngineFrame::Message),
        other => Err(RealtimeError::Codec(format!(
            "invalid engine.io frame type {other}"
        ))),
    }
}

/// Encodes the namespace connect frame with an auth payload object:
/// `40/ws,{...}` (root namespace would be `40{...}`).
pub fn encode_connect(nsp: &str, auth: &JSONValue) -> String {
    if nsp == "/" {
        format!("40{auth}")
    } else {
        format!("40{nsp},{auth}")
    }
}

/// Encodes an event emit carrying an ack id: `42/ws,<id>[<data array>]`.
pub fn encode_event_ack(nsp: &str, id: u64, data: &[JSONValue]) -> Result<String> {
    let array = serde_json::to_string(data)?;
    Ok(if nsp == "/" {
        format!("42{id}{array}")
    } else {
        format!("42{nsp},{id}{array}")
    })
}

/// engine.io pong reply (`3`) to a server ping (`2`).
pub fn encode_pong() -> String {
    "3".to_string()
}

/// engine.io close frame (`1`) for clean client disconnects.
pub fn encode_close() -> String {
    "1".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_open_packet() {
        let frame = parse_frame(
            r#"0{"sid":"abc","upgrades":[],"pingInterval":25000,"pingTimeout":5000,"maxPayload":1000000}"#,
        )
        .unwrap();
        match frame {
            EngineFrame::Open(open) => {
                assert_eq!(open.sid, "abc");
                assert_eq!(open.ping_interval_ms, 25_000);
                assert_eq!(open.ping_timeout_ms, 5_000);
            }
            other => panic!("expected open, got {other:?}"),
        }
    }

    #[test]
    fn parses_server_ping_and_ack() {
        assert_eq!(parse_frame("2").unwrap(), EngineFrame::Ping);
        let ack = parse_frame(
            r#"43/ws,7[{"correlationId":"c1","results":[{"handlerId":"ws_webui.WsWebui","ok":true}]}]"#,
        )
        .unwrap();
        match ack {
            EngineFrame::Message(SocketPacket::Ack { nsp, id, data }) => {
                assert_eq!(nsp, "/ws");
                assert_eq!(id, 7);
                assert_eq!(data.len(), 1);
                assert_eq!(data[0]["correlationId"], "c1");
            }
            other => panic!("expected ack, got {other:?}"),
        }
    }

    #[test]
    fn parses_state_push_event() {
        let frame = parse_frame(
            r#"42/ws,["state_push",{"data":{"runtime_epoch":"e1","seq":3,"snapshot":{"deselect_chat":false,"context":"c","contexts":null,"tasks":null,"logs":[],"log_guid":"g","log_version":1,"log_progress":"","log_progress_active":false,"paused":false,"notifications":[],"notifications_guid":"ng","notifications_version":1}}}]"#,
        )
        .unwrap();
        match frame {
            EngineFrame::Message(SocketPacket::Event { nsp, data }) => {
                assert_eq!(nsp, "/ws");
                assert_eq!(data[0], "state_push");
                assert_eq!(data[1]["data"]["seq"], 3);
            }
            other => panic!("expected event, got {other:?}"),
        }
    }

    #[test]
    fn parses_namespace_only_disconnect_and_connect_without_payload() {
        assert_eq!(
            parse_frame("41/ws").unwrap(),
            EngineFrame::Message(SocketPacket::Disconnect { nsp: "/ws".into() })
        );
        assert_eq!(
            parse_frame("40/ws").unwrap(),
            EngineFrame::Message(SocketPacket::Connect {
                nsp: "/ws".into(),
                payload: None
            })
        );
    }

    #[test]
    fn parses_connect_error_with_object_payload() {
        let frame = parse_frame(r#"44/ws,{"message":"invalid session"}"#).unwrap();
        match frame {
            EngineFrame::Message(SocketPacket::ConnectError { nsp, message }) => {
                assert_eq!(nsp, "/ws");
                assert_eq!(message, "invalid session");
            }
            other => panic!("expected connect error, got {other:?}"),
        }
    }

    #[test]
    fn encodes_connect_event_ack_pong_close() {
        let auth = json!({"csrf_token": "t", "handlers": ["ws_webui"]});
        assert_eq!(
            encode_connect("/ws", &auth),
            r#"40/ws,{"csrf_token":"t","handlers":["ws_webui"]}"#
        );
        assert_eq!(encode_connect("/", &auth), format!("40{auth}"));
        let data = vec![json!({"ts": 1u64, "correlationId": "c"})];
        assert_eq!(
            encode_event_ack("/ws", 0, &data).unwrap(),
            r#"42/ws,0[{"correlationId":"c","ts":1}]"#
        );
        assert_eq!(encode_pong(), "3");
        assert_eq!(encode_close(), "1");
    }

    #[test]
    fn rejects_binary_and_garbage() {
        assert!(parse_frame(r#"51-["foo"]"#).is_err());
        assert!(parse_frame("").is_err());
        assert!(parse_frame("9").is_err());
    }
}
