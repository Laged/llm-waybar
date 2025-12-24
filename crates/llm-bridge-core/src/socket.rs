use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum DaemonMessage {
    Event { event_type: String, tool: Option<String> },
    Status { payload: String },
}

impl DaemonMessage {
    pub fn encode(&self) -> String {
        match self {
            DaemonMessage::Event { event_type, tool } => {
                match tool {
                    Some(t) => format!("EVENT:{}:{}", event_type, t),
                    None => format!("EVENT:{}", event_type),
                }
            }
            DaemonMessage::Status { payload } => format!("STATUS:{}", payload),
        }
    }

    pub fn decode(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix("EVENT:") {
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            Some(DaemonMessage::Event {
                event_type: parts[0].to_string(),
                tool: parts.get(1).map(|s| s.to_string()),
            })
        } else if let Some(rest) = s.strip_prefix("STATUS:") {
            Some(DaemonMessage::Status { payload: rest.to_string() })
        } else {
            None
        }
    }
}

/// Try to send message to daemon. Returns Ok(true) if sent, Ok(false) if daemon not available.
pub fn send_to_daemon(socket_path: &Path, message: &DaemonMessage) -> io::Result<bool> {
    let socket = match UnixDatagram::unbound() {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    // Non-blocking connect attempt
    socket.set_write_timeout(Some(Duration::from_millis(1)))?;

    let encoded = message.encode();
    match socket.send_to(encoded.as_bytes(), socket_path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
