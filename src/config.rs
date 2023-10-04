use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct MinecraftServerDescription {
    pub listen: String,
    pub server_name: String,
    pub proxy_pass: String
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
pub struct MineginxConfig {
    pub servers: Vec<MinecraftServerDescription>
}
