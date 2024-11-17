use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct MinecraftServerDescription {
    pub listen: String,
    pub server_names: Vec<String>,
    pub proxy_pass: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_size: Option<u32>
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct MineginxConfig {
    pub handshake_timeout_ms: Option<u64>,
    pub servers: Vec<MinecraftServerDescription>
}
