use bevy::{
    prelude::*, 
    time::Timer
};
use std::collections::HashMap;

use bevy_renet::{
    renet::{
        RenetConnectionConfig, 
        ChannelConfig, 
        ReliableChannelConfig, 
        UnreliableChannelConfig
    },
};

/// Controls how often the server and client update each other.
pub const POLL_RATE: f32 = 1.0 / 60.0;

use serde::{Deserialize, Serialize};

/// Default connection config used for both server and client.
pub fn connection_config() -> RenetConnectionConfig {
    RenetConnectionConfig{
        send_channels_config: vec![
            ChannelConfig::Reliable(ReliableChannelConfig{
                channel_id: 0,
                ..default()
            }),
            ChannelConfig::Unreliable(UnreliableChannelConfig{
                channel_id: 1,
                message_send_queue_size: 2048,
                message_receive_queue_size: 2048,
                ..default()
            }),
            ChannelConfig::Reliable(ReliableChannelConfig{
                channel_id: 2,
                ..default()
            }),],
        receive_channels_config: vec![
                ChannelConfig::Reliable(ReliableChannelConfig{
                    channel_id: 0,
                    ..default()
                }),
                ChannelConfig::Unreliable(UnreliableChannelConfig{
                    channel_id: 1,
                    message_send_queue_size: 2048,
                    message_receive_queue_size: 2048,
                    ..default()
                }),
                ChannelConfig::Reliable(ReliableChannelConfig{
                    channel_id: 2,
                    ..default()
                }),],
        ..default()
    }
}

/// This timer controls how often both client and server send information to prevent them from overloading eachother.
#[derive(Component)]
pub struct SendTimer(pub Timer);

/// Struct represents player inputs.
#[derive(Debug, Default, Serialize, Deserialize, Component)]
pub struct PlayerInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

/// Struct containing all of the information about the game which can change over time.
/// Used for updating the client with information from the server.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GameState{
    pub ball_loc: Vec2,
    pub ball_velocity: Vec2,
    pub paddle_l_loc: Vec2,
    pub paddle_r_loc: Vec2,
    pub score_l: i32,
    pub score_r: i32,
}

/// Possible messages the server could send to the player.
#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ServerMessages {
    PlayerConnected { id: u64 },
    PlayerDisconnected { id: u64 },
    PlayerCheck,
}

/// Possible messages the client could send to the server.
#[derive(Debug, Serialize, Deserialize, Component)]
pub enum ClientMessages {
    PlayerCheckResponse { id: u64 },
    AuthenticationRequest { id: u64 },
}

/// Contains a list of the players and their respective entity.
#[derive(Debug, Default)]
pub struct Lobby {
    pub players: HashMap<u64, Entity>,
}