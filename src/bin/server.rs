//! This is the server which hosts the game.
//! All of the code in this file is related to networking.
//! For actual game code see common_game.rs

use rand::{
    thread_rng,
    RngCore
};

use bevy::{
    prelude::*, 
    log::LogPlugin, 
    core::CorePlugin, 
    diagnostic::DiagnosticsPlugin, 
    time::TimePlugin,
    app::ScheduleRunnerPlugin,
};

use bevy_renet::{
    renet::{
        RenetError, 
        RenetServer, 
        ServerAuthentication, 
        ServerConfig, 
        ServerEvent, 
        ConnectToken
    },
    RenetServerPlugin,
};

use threadpool::ThreadPool;

use std::{time::{SystemTime, UNIX_EPOCH}, 
    io::{BufReader, Read}, 
    net::{UdpSocket,TcpListener,TcpStream,SocketAddr},
    thread,
};

use pong_multiplayer_rs::common_net::*;
use pong_multiplayer_rs::common_game::*;


const PUB_IP: &str = "45.33.33.109:5000";
const PROTOCOL_ID: u64 = 7;
struct CheckResponses(Vec<u64>);
struct ReconnectTimer(Timer,bool);

#[derive(Debug, Component)]
struct Player {
}

#[derive(Component)]
struct ResetDue {
    is_reset_due: bool
}

fn new_renet_server(pkey: [u8; 32]) -> RenetServer {
    let server_addr = PUB_IP.parse().unwrap();
    let socket = UdpSocket::bind("0.0.0.0:5000").unwrap();
    let connection_config =  connection_config();
    let server_config = ServerConfig::new(64, PROTOCOL_ID, server_addr, ServerAuthentication::Secure{ private_key:pkey});
    let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    RenetServer::new(current_time, server_config, connection_config, socket).unwrap()
}

fn handle_connection(mut stream: TcpStream, pkey: [u8;32]){
    let mut reader = BufReader::new(&mut stream);
    let mut bytes: [u8; 8] = [0u8; 8];
    reader.read_exact(&mut bytes).unwrap();
    let client_id = u64::from_be_bytes(bytes);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let addr: SocketAddr = PUB_IP.parse().unwrap();
    let token = ConnectToken::generate(
        now,
        PROTOCOL_ID,
        120000,
        client_id,
        30,
        vec![addr],
        None,
        &pkey
    ).unwrap();
    token.write(&mut stream).unwrap();
}

fn tcpserver(pkey: [u8;32]) {
    let listener = TcpListener::bind("0.0.0.0:5000").unwrap();
    let pool = ThreadPool::new(4);
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let key = pkey.clone();
                pool.execute(move|| {
                    handle_connection(s, key);
                });
            }
            Err(e) => panic!("Encountered IO error: {e}")
        }
    }
    pool.join();
}
fn main() {
    let mut rng = thread_rng();
    let mut pkey: [u8; 32] = [0u8;32];

    rng.fill_bytes(&mut pkey);

    let threadkey = pkey.clone();
    thread::spawn(move ||tcpserver(threadkey));

    let mut app = App::new();
    // Since we're a headless server, we don't need a lot of the default plugins.
    // Instead, I picked out the ones we actually use.
    app.add_plugin(LogPlugin)
        .add_plugin(CorePlugin)
        .add_plugin(TimePlugin)
        .add_plugin(TransformPlugin)
        .add_plugin(HierarchyPlugin)
        .add_plugin(DiagnosticsPlugin)
        .add_plugin(ScheduleRunnerPlugin);
    app.insert_resource(Lobby::default());
    app.insert_resource(ResetDue{ is_reset_due: false});
    app.insert_resource(SendTimer(Timer::from_seconds(POLL_RATE, true)));
    app.add_plugin(RenetServerPlugin);
    let mut rtimer = Timer::from_seconds(3.0,false);
    rtimer.pause();
    app.insert_resource(ReconnectTimer(rtimer,false));
    app.insert_resource(CheckResponses(Vec::new()));
    app.insert_resource(new_renet_server(pkey.clone()));
    app.add_system(server_update_system);
    app.add_system(server_sync_players);
    app.add_system(move_players_system);
    app.add_system(panic_on_error_system);
    app.add_system(resetter);

    // All of the actual game systems and resources are added in here. See common_game.rs
    app = add_to_app_server(app);
    app.run();
}


fn resetter(
    mut ball_query: Query<(&mut Velocity, &mut Transform),(With<Ball>,Without<Paddle>)>,
    mut timer: ResMut<RespawnTimer>,
    mut playing: ResMut<Playing>,
    mut paddles: Query<&mut Transform,With<Paddle>>,
    mut resetter: ResMut<ResetDue>,
) {
    if !resetter.is_reset_due {
        return;
    }
    //Make sure system only fires this once
    resetter.is_reset_due = false;

    //Reset the paddles
    for mut paddle in paddles.iter_mut(){
        paddle.translation.y = 0.0;
    }

    //Reset the ball, and then trigger the respawn timer.
    let (mut ball_velocity, mut ball_transform) = ball_query.single_mut();
    ball_velocity.x = 0.0;
    ball_velocity.y = 0.0;
    ball_transform.translation.x = BALL_STARTING_POSITION.x;
    ball_transform.translation.y = BALL_STARTING_POSITION.x;
    timer.0.reset();

    //Allow the game to start.
    playing.0 = true;
}

/// Server update system recieves from all of the clients.
/// Manages users connecting, disconnecting, input, etc.
fn server_update_system(
    mut server_events: EventReader<ServerEvent>,
    mut commands: Commands,
    mut lobby: ResMut<Lobby>,
    mut server: ResMut<RenetServer>,
    mut responses: ResMut<CheckResponses>,
    mut playing: ResMut<Playing>,
    mut scoreboard: ResMut<Scoreboard>,
    paddles: Query<(Entity,&PaddleSide),(With<Paddle>,Without<Player>)>,
    mut resetter: ResMut<ResetDue>,
) {
    for event in server_events.iter() {
        match event {
            ServerEvent::ClientConnected(id, _) => {
                println!("Player {} connected.", id);

                // If there are any paddles without players attached to them already,
                // then attach this new player to the first one we recieve in our query.
                let (player_entity, pside) = match paddles.iter().next() {
                    Some(p) => p,
                    None => {
                        //Otherwise, just disconnect them.
                        server.disconnect(*id);
                        continue;
                    },
                };

                commands.entity(player_entity).insert(Player {}).insert(PlayerInput::default());

                // We could send an InitState with all the players id and positions for the client
                // but this is easier to do.
                for &player_id in lobby.players.keys() {
                    let message = bincode::serialize(&ServerMessages::PlayerConnected { id: player_id }).unwrap();
                    server.send_message(*id, 0, message);
                }

                //Also, let them know which side they're on.
                let message = bincode::serialize(&ServerMessages::PlayerIsSide{ side: pside.0}).unwrap();
                server.send_message(*id, 0, message);

                lobby.players.insert(*id, player_entity);

                if lobby.players.keys().len() >= 2 {
                    //Signals to the reset system to reset and begin the game.
                    //Can't include it here because of the previous use of paddles, so we delegate it to a new system.
                    resetter.is_reset_due = true;
                }

                // Forward the ClientConnected event to the rest of the players.
                let message = bincode::serialize(&ServerMessages::PlayerConnected { id: *id }).unwrap();
                server.broadcast_message(0, message);
            }
            ServerEvent::ClientDisconnected(id) => {
                println!("Player {} disconnected.", id);

                // If they're associated with an entity, remove that association. This frees up paddles for other players who connect.
                if let Some(player_entity) = lobby.players.remove(id) {
                    commands.entity(player_entity).remove::<Player>().remove::<PlayerInput>();
                }

                //If this drops us below 2 players, then pause the game and reset the score
                if lobby.players.keys().len() <= 2 {
                    playing.0 = false;

                    

                    scoreboard.scoreleft = 0;
                    scoreboard.scoreright = 0;
                }

                // Forward the ClientDisconnected event to the rest of the players.
                let message = bincode::serialize(&ServerMessages::PlayerDisconnected { id: *id }).unwrap();
                server.broadcast_message(0, message);
            }
        }
    }

    for client_id in server.clients_id().into_iter() {
        // Recieve input here.
        while let Some(message) = server.receive_message(client_id, 0) {
            // Attach the player inputs to their entity for future use by the movement system.
            let player_input: PlayerInput = bincode::deserialize(&message).unwrap();
            if let Some(player_entity) = lobby.players.get(&client_id) {
                commands.entity(*player_entity).insert(player_input);
            }
        }
        // Recieve ClientMessages here. Currently this is just for player checks.
        while let Some(message) = server.receive_message(client_id,2) {
            let recieved: ClientMessages = bincode::deserialize(&message).unwrap();
            match recieved {
                ClientMessages::PlayerCheckResponse { id } => {
                    //They are responding to a player check. Add them to the list of players who responded if their id checks out.
                    if id == client_id {
                        responses.0.push(id);
                    }
                },
                _ => ()
            }
        }
    }
}

/// So, I decided to put the code that actually gets the gamestate information in the common_game.rs file.
/// It felt fitting to have the code that gets and sets gamestate in the same place.
fn server_sync_players(
    mut server: ResMut<RenetServer>, 
    ball: Query<(&Transform, &Velocity), With<Ball>>, 
    paddles: Query<(&Transform,&PaddleSide), With<Paddle>>, 
    scoreboard: Res<Scoreboard>,
    playing: Res<Playing>,
    time:Res<Time>, 
    mut timer: ResMut<SendTimer>,) {
    if timer.0.tick(time.delta()).just_finished() {
        //Just get gamestate, serialize it, send it.
        let gamestate = get_gamestate(ball,paddles,scoreboard,playing);
        let sync_message = bincode::serialize(&gamestate).unwrap();
        server.broadcast_message(1, sync_message);
    }
}

/// Players just send their input instead of keeping track of their own position.
/// This would cause issues with any significant packet loss.
/// There's better solutions I'm certain which involve letting the user send their position and then checking the validity of that.
/// But this should work fairly well in most situations.
fn move_players_system(mut query: Query<(&mut Transform, &PlayerInput)>, time: Res<Time>) {
    for (mut transform, input) in query.iter_mut() {
        let y = (input.up as i8 - input.down as i8) as f32;
        let bottom_bound = BOTTOM_WALL + WALL_THICKNESS / 2.0 + PADDLE_SIZE.y / 2.0 + PADDLE_PADDING;
        let top_bound = TOP_WALL - WALL_THICKNESS / 2.0 - PADDLE_SIZE.y / 2.0 - PADDLE_PADDING;
        let new_position = transform.translation.y + y * PADDLE_SPEED * time.delta().as_secs_f32();
        transform.translation.y = new_position.clamp(bottom_bound,top_bound);
    }
}

/// I will come out and say, this entire system feels wrong to me.
/// This seems like something that the renet library should handle, or give some method for handling forcequits.
/// Very frustrating that we can't even tell who lost connection, but this is the best we can do with what we have as far as I'm aware.
fn panic_on_error_system(mut renet_error: EventReader<RenetError>,mut server: ResMut<RenetServer>, mut timer: ResMut<ReconnectTimer>, mut responses: ResMut<CheckResponses>, time: Res<Time>,) {
    // Usually these errors are some result of a client forcequitting.
    // There's probably more you can do to actually capture errors not related to this, but I decided against it.
    for _ in renet_error.iter() {
        
        // To be clear, the timer.1 variable is necessary because unpausing seems to have some delay to it.
        // So this ensures that this doesn't fire multiple times. 
        if timer.0.paused() && !timer.1 {
            println!("Network Error encountered, attempted to purge nonpresent players.");
            let message = bincode::serialize(&ServerMessages::PlayerCheck).unwrap();
            // Send players a packet which requests they send a response with their id to verify they are there.
            // No longer able to be impersonated thanks to cryptographic signing of messages. Verify their ID before accepting it.
            server.broadcast_message(0, message);

            timer.0.unpause();
            timer.1 = true;
        } else if timer.0.tick(time.delta()).just_finished() {
            // When we get a response from the clients saying they recieved the packets, we add them to responses.
            // If they didn't respond, we disconnect them, assuming they forcequit or had some connection issue.
            for client_id in server.clients_id() {
                if !responses.0.contains(&client_id){
                    server.disconnect(client_id);
                }
            }
            // Reset everything so future errors can trigger this system again.
            responses.0.clear();
            timer.0.reset();
            timer.0.pause();
            timer.1 = false;
        }
    }
}
