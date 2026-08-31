use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Heartbeat {
    pub alive: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    Heartbeat(Heartbeat),
}

impl NetworkMessage {
    pub fn heartbeat() -> Self {
        Self::Heartbeat(Heartbeat { alive: true })
    }
}
