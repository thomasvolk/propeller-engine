use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum IpcMessage {
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_message_stop_serialises_correctly() {
        let msg = IpcMessage::Stop;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"stop"}"#);
    }

    #[test]
    fn ipc_message_stop_deserialises_correctly() {
        let json = r#"{"type":"stop"}"#;
        let msg: IpcMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, IpcMessage::Stop));
    }
}
