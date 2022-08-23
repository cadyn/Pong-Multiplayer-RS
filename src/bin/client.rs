//! This is the client which plays the game.
//! All of the code in this file is related to networking.
//! For actual game code see common_game.rs

use bevy::{
    prelude::*, 
    window::WindowSettings, 
    time::Timer,
};

use bevy_renet::{
    renet::{
        ClientAuthentication, 
        RenetClient, 
        RenetError, ConnectToken, 
    },
    run_if_client_connected, 
    RenetClientPlugin,
};

use std::{time::SystemTime, net::{SocketAddr, TcpStream}, io::Write};
use std::{net::UdpSocket};

const PROTOCOL_ID: u64 = 7;

use pong_multiplayer_rs::{common_net::*, common_game::*};

fn new_renet_client(token: ConnectToken) -> RenetClient {
    //let server_addr = "45.33.33.109:5000".parse().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    //socket.connect(server_addr).unwrap();
    let connection_config = connection_config();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let client_id = current_time.as_millis() as u64;
    let authentication = ClientAuthentication::Secure {
        connect_token: token
    };
    RenetClient::new(current_time, socket, client_id, connection_config, authentication).unwrap()
}

fn main() {
    //Get our token first.
    let sockaddr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let mut stream = TcpStream::connect(sockaddr).unwrap();
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let id = current_time.as_millis() as u64;

    //let auth_request = ClientMessages::AuthenticationRequest { id };
    //let auth_request_bytes = bincode::serialize(&auth_request).unwrap();
    let client_id_bytes: [u8; 8] = id.to_be_bytes();
    stream.write(&client_id_bytes).unwrap();
    let token = ConnectToken::read(&mut stream).unwrap();

    let mut app = App::new();

    // Let us handle the window close, allows us to clean up as needed before the app exits.
    app.insert_resource(WindowSettings{
        close_when_requested:false,
        ..default()
    });

    app.add_plugins(DefaultPlugins);

    app.add_plugin(RenetClientPlugin);
    app.insert_resource(new_renet_client(token));
    app.insert_resource(PlayerInput::default());
    app.insert_resource(SendTimer(Timer::from_seconds(POLL_RATE, true)));
    app.add_system(player_input);
    app.add_system(client_send_input.with_run_criteria(run_if_client_connected));
    app.add_system(client_sync_players.with_run_criteria(run_if_client_connected));
    app.add_system(on_exit);

    // Gets game systems and resources from common_game.rs
    app = add_to_app_client(app);
    app.add_system(panic_on_error_system);
    app.run();
}

/// Recieves information from the server and synchronizes the client.
fn client_sync_players(
    mut client: ResMut<RenetClient>,
    mut ball: Query<(&mut Transform, &mut Velocity), (With<Ball>,Without<Paddle>)>, 
    mut paddles: Query<(&mut Transform,&PaddleSide), With<Paddle>>, 
    mut scoreboard: ResMut<Scoreboard>,
) {
    // Recieving specific messages from the server.
    while let Some(message) = client.receive_message(0) {
        let server_message = bincode::deserialize(&message).unwrap();
        match server_message {
            ServerMessages::PlayerConnected { id } => {
                // Simply relay player connected to the console for debugging.
                println!("Player {} connected.", id);
            }
            ServerMessages::PlayerDisconnected { id } => {
                // Simply relay player disconnected to the console for debugging.
                println!("Player {} disconnected.", id);
            },
            ServerMessages::PlayerCheck => {
                // Server wants to check that we are still here. Send an appropriate response.
                let message = bincode::serialize(&ClientMessages::PlayerCheckResponse { id: client.client_id() }).unwrap();
                client.send_message(2, message);
            },
        }
    }

    // This is where we recieve information pertaining to the actual state of the game.
    // The information is contained within the GameState struct, 
    // and the logic to use that information is in common_game.rs
    while let Some(message) = client.receive_message(1) {
        let gamestate: GameState = bincode::deserialize(&message).unwrap();
        set_gamestate(&mut ball,&mut paddles,&mut scoreboard,gamestate);
    }
}

/// Checks which keys are being pressed and converts that to directional movement.
fn player_input(keyboard_input: Res<Input<KeyCode>>, mut player_input: ResMut<PlayerInput>) {
    player_input.left = keyboard_input.pressed(KeyCode::A) || keyboard_input.pressed(KeyCode::Left);
    player_input.right = keyboard_input.pressed(KeyCode::D) || keyboard_input.pressed(KeyCode::Right);
    player_input.up = keyboard_input.pressed(KeyCode::W) || keyboard_input.pressed(KeyCode::Up);
    player_input.down = keyboard_input.pressed(KeyCode::S) || keyboard_input.pressed(KeyCode::Down);
}

/// We send our input and the server moves us. 
/// Makes things easier since the client does not need to keep track of which paddle it represents.
/// Has potential for issues if packet loss is high.
fn client_send_input(player_input: Res<PlayerInput>, mut client: ResMut<RenetClient>, time:Res<Time>, mut timer: ResMut<SendTimer>) {
    if timer.0.tick(time.delta()).just_finished() {
        let input_message = bincode::serialize(&*player_input).unwrap();

        client.send_message(0, input_message);
    }
}

/// If any error is found we just panic. This could definitely be improved for more robustness.
fn panic_on_error_system(mut renet_error: EventReader<RenetError>) {
    for e in renet_error.iter() {
        //panic!("{:?}",e);
        println!("{:?}",e);
    }
}

/// Checks if user tried to close window, then cleans up and actually closes it once cleanup is finished.
fn on_exit(window_closed: EventReader<bevy::window::WindowCloseRequested>, mut client: ResMut<RenetClient>, mut windows: ResMut<Windows>){
    // User tried to close window. Cleanup first, then actually close it.
    if !window_closed.is_empty(){
        //Disconnect first.
        client.disconnect();
        //Then close the window. App will exit shortly after this.
        windows.primary_mut().close();
    }
}
