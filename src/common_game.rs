//! The actual game code which both server and client use.
//! Currently it is Pong.

use num::signum;

use rand::prelude::random;
use serde::{Deserialize,Serialize};

use std::time::Duration;

use bevy::{
    prelude::*,
    sprite::{collide_aabb::{collide, Collision}},
};

use iyes_loopless::prelude::*;

use crate::common_net::GameState;

// Defines the amount of time that should elapse between each physics step.
const TIME_STEP: f32 = 1.0 / 120.0;

const DEG_TO_RAD: f32 = std::f32::consts::PI / 180.0;

// These constants are defined in `Transform` units.
// Using the default 2D camera they correspond 1:1 with screen pixels.
pub const PADDLE_SIZE: Vec3 = Vec3::new(20.0, 120.0, 0.0);
const GAP_BETWEEN_PADDLE_AND_WALL: f32 = 60.0;
pub const PADDLE_SPEED: f32 = 500.0;
// How close can the paddle get to the wall
pub const PADDLE_PADDING: f32 = 10.0;

// We set the z-value of the ball to 1 so it renders on top in the case of overlapping sprites.
pub const BALL_STARTING_POSITION: Vec3 = Vec3::new(0.0, -50.0, 1.0);
const BALL_SIZE: Vec3 = Vec3::new(30.0, 30.0, 0.0);
const BALL_SPEED: f32 = 400.0;
const INITIAL_BALL_DIRECTION: Vec2 = Vec2::new(0.5, -0.5);
const BALL_SPEED_INCREASE: f32 = 1.1;
const MAX_BALL_SPEED: f32 = 5000.0;

const TRAIL_DECAY_MS: i32 = 500;
const TRAIL_MAX_ALPHA: f32 = 0.5;

pub const WALL_THICKNESS: f32 = 10.0;
// x coordinates
const LEFT_WALL: f32 = -450.;
const RIGHT_WALL: f32 = 450.;
// y coordinates
pub const BOTTOM_WALL: f32 = -300.;
pub const TOP_WALL: f32 = 300.;

const SCOREBOARD_FONT_SIZE: f32 = 40.0;
const SCOREBOARD_TEXT_PADDING: Val = Val::Px(5.0);

const BACKGROUND_COLOR: Color = Color::rgb(0.9, 0.9, 0.9);
const PADDLE_COLOR: Color = Color::rgb(0.3, 0.3, 0.7);
const BALL_COLOR: Color = Color::rgb(1.0, 0.5, 0.5);
const WALL_COLOR: Color = Color::rgb(0.8, 0.8, 0.8);
const TEXT_COLOR: Color = Color::rgb(0.5, 0.5, 1.0);
const SCORE_COLOR: Color = Color::rgb(1.0, 0.5, 0.5);

//Tells systems whether to run or not.
pub fn is_game_active(playing: Res<Playing>) -> bool {
    playing.0
}

/// Add game resources and systems to the client.
pub fn add_to_app_client(mut app: App) -> App {
    let fixed_update_stage = SystemStage::parallel()
    .with_system(check_for_collisions.run_if(is_game_active).label("Collision check"))
    .with_system(apply_velocity.run_if(is_game_active).before("Collision check"))
    .with_system(play_collision_sound.run_if(is_game_active).after("Collision check"));
    
        
    app.insert_resource(Scoreboard { scoreleft: 0, scoreright: 0 })
        .insert_resource(Playing(false))
        .insert_resource(ClearColor(BACKGROUND_COLOR))
        .insert_resource(RespawnTimer(Timer::from_seconds(3.0,false)))
        .add_startup_system(setup_client)
        .add_event::<CollisionEvent>()
        .add_stage(
            "fixed_update",
            FixedTimestepStage::new(Duration::from_secs_f32(TIME_STEP))
                .with_stage(fixed_update_stage)
        )
        .add_system(update_scoreboard)
        .add_system(handle_trails)
        .add_system(bevy::window::close_on_esc);
        //.add_system(respawn_ball); Removing respawn system from the client as it's inherently random and could lead to desync.
        // Let the server handle respawning and update the client.
    return app;
}


/// Adds game resources and systems to the server, excluding the systems only the client needs.
pub fn add_to_app_server(mut app: App) -> App {
    let fixed_update_stage = SystemStage::parallel()
    .with_system(check_for_collisions.run_if(is_game_active).label("Collision check"))
    .with_system(apply_velocity.run_if(is_game_active).before("Collision check"));

    app.insert_resource(Scoreboard { scoreleft: 0, scoreright: 0 })
        .insert_resource(Playing(false))
        .insert_resource(RespawnTimer(Timer::from_seconds(3.0,false)))
        .add_startup_system(setup_server)
        .add_event::<CollisionEvent>()
        .add_stage(
            "fixed_update",
            FixedTimestepStage::new(Duration::from_secs_f32(TIME_STEP))
                .with_stage(fixed_update_stage)
        )
        .add_system(respawn_ball);
    return app;
}

/// This just tells us which entities are paddles.
#[derive(Component)]
pub struct Paddle;

/// This tells us which side the paddle is supposed to represent.
#[derive(Component)]
pub struct PaddleSide(pub PlayerSide);

/// Either left or right, used in the PaddleSide tuple struct
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerSide {
    Left,
    Right,
}

#[derive(Component)]
pub struct Playing(pub bool);

/// Ball component.
#[derive(Component)]
pub struct Ball{
    /// Keeps track of which side scored last to decide which way the ball will go.
    pub lastpointleft: bool
}

/// Keeps track of how long we need to wait to let the ball start moving again.
pub struct RespawnTimer(pub Timer);

/// Velocity just stores a Vec2, used to calculate movement.
#[derive(Component, Deref, DerefMut)]
pub struct Velocity(pub Vec2);

#[derive(Component)]
pub struct Collider;

#[derive(Default)]
pub struct CollisionEvent;

#[derive(Component)]
pub struct Trail{
    timeleft: i32,
    startalpha: f32,
}

/// Keeps track of which side the wall is on for easy checking later.
#[derive(Component)]
pub struct WallLoc(pub WallLocation);

#[derive(Component)]
pub struct Wall;

#[derive(Component)]
pub struct Movable;

//struct RandomGen(ThreadRng);

pub struct CollisionSound(Handle<AudioSource>);

/// This bundle is a collection of the components that define a "wall" in our game
#[derive(Bundle)]
pub struct WallBundle {
    // You can nest bundles inside of other bundles like this
    // Allowing you to compose their functionality
    #[bundle]
    pub sprite_bundle: SpriteBundle,
    pub collider: Collider,
    pub location: WallLoc,
}

/// The same bundle that defines a wall, but it has no sprite, so it can be used on the server.
#[derive(Bundle)]
pub struct WallBundleServer {
    pub transform: Transform,
    pub collider: Collider,
    pub location: WallLoc,
}

/// Which side of the arena is this wall located on?
pub enum WallLocation {
    Left,
    Right,
    Bottom,
    Top,
}

impl WallLocation {
    /// Uses which side it's on to get actual coordinates
    pub fn position(&self) -> Vec2 {
        match self {
            WallLocation::Left => Vec2::new(LEFT_WALL, 0.),
            WallLocation::Right => Vec2::new(RIGHT_WALL, 0.),
            WallLocation::Bottom => Vec2::new(0., BOTTOM_WALL),
            WallLocation::Top => Vec2::new(0., TOP_WALL),
        }
    }

    /// Uses which side it's on to get its own size.
    pub fn size(&self) -> Vec2 {
        let arena_height = TOP_WALL - BOTTOM_WALL;
        let arena_width = RIGHT_WALL - LEFT_WALL;
        // Make sure we haven't messed up our constants
        assert!(arena_height > 0.0);
        assert!(arena_width > 0.0);

        match self {
            WallLocation::Left | WallLocation::Right => {
                Vec2::new(WALL_THICKNESS, arena_height + WALL_THICKNESS)
            }
            WallLocation::Bottom | WallLocation::Top => {
                Vec2::new(arena_width + WALL_THICKNESS, WALL_THICKNESS)
            }
        }
    }
}

impl WallBundleServer {
    /// This "builder method" allows us to reuse logic across our wall entities,
    /// making our code easier to read and less prone to bugs when we change the logic
    pub fn new(location: WallLocation) -> WallBundleServer {
        WallBundleServer {
            transform: Transform {
                // We need to convert our Vec2 into a Vec3, by giving it a z-coordinate
                // This is used to determine the order of our sprites
                translation: location.position().extend(0.0),
                // The z-scale of 2D objects must always be 1.0,
                // or their ordering will be affected in surprising ways.
                // See https://github.com/bevyengine/bevy/issues/4149
                scale: location.size().extend(1.0),
                ..default()
            },
            collider: Collider,
            location: WallLoc(location),
        }
    }
}

impl WallBundle {
    /// This "builder method" allows us to reuse logic across our wall entities,
    /// making our code easier to read and less prone to bugs when we change the logic
    pub fn new(location: WallLocation) -> WallBundle {
        WallBundle {
            sprite_bundle: SpriteBundle {
                transform: Transform {
                    // We need to convert our Vec2 into a Vec3, by giving it a z-coordinate
                    // This is used to determine the order of our sprites
                    translation: location.position().extend(0.0),
                    // The z-scale of 2D objects must always be 1.0,
                    // or their ordering will be affected in surprising ways.
                    // See https://github.com/bevyengine/bevy/issues/4149
                    scale: location.size().extend(1.0),
                    ..default()
                },
                sprite: Sprite {
                    color: WALL_COLOR,
                    ..default()
                },
                ..default()
            },
            collider: Collider,
            location: WallLoc(location),
        }
    }
}

/// This resource tracks the game's score
pub struct Scoreboard {
    pub scoreleft: usize,
    pub scoreright: usize,
}

/// Creates nice looking trails for the ball.
fn handle_trails(
    mut trails: Query<(Entity,&mut Trail, &mut Sprite)>,
    ball: Query<(&Transform,&Velocity),With<Ball>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let ms_passed = time.delta().as_millis() as i32;
    let (ball_transform, ball_velocity) = ball.single();

    for (trail_ent, mut trail, mut trail_sprite) in trails.iter_mut(){
        trail.timeleft -= ms_passed;
        if trail.timeleft < 0{
            commands.entity(trail_ent).despawn();
            continue;
        }
        let alpha = trail.timeleft as f32 / (TRAIL_DECAY_MS as f32 / trail.startalpha);
        trail_sprite.color = Color::rgba(BALL_COLOR.r(),BALL_COLOR.g(),BALL_COLOR.b(),alpha);
    }

    let ball_dist = ball_velocity.0.length() * time.delta().as_secs_f32();
    let ball_lengths_passed = ball_dist / BALL_SIZE.x;
    let starting_alpha = (ball_lengths_passed * TRAIL_MAX_ALPHA).clamp(0.0,TRAIL_MAX_ALPHA);

    
    let ball_pos = ball_transform.translation;

    commands.spawn()
        .insert(Trail{ timeleft: TRAIL_DECAY_MS, startalpha: starting_alpha })
        .insert_bundle(SpriteBundle {
            transform: Transform {
                scale: BALL_SIZE,
                translation: ball_pos,
                ..default()
            },
            sprite: Sprite {
                color: Color::rgba(BALL_COLOR.r(),BALL_COLOR.g(),BALL_COLOR.b(),TRAIL_MAX_ALPHA),
                ..default()
            },
            ..default()
        });
}

/// This takes information from all of the parts of the game that change over time and puts it into a struct
/// Which is easier to send over network and read.
pub fn get_gamestate(
    ball: Query<(&Transform, &Velocity), With<Ball>>, 
    paddles: Query<(&Transform,&PaddleSide), With<Paddle>>, 
    scoreboard: Res<Scoreboard>,
    playing: Res<Playing>
) -> GameState {
    let ball = ball.single();
    let mut paddle_l = Vec2::new(LEFT_WALL + GAP_BETWEEN_PADDLE_AND_WALL,0.0);
    let mut paddle_r = Vec2::new(RIGHT_WALL - GAP_BETWEEN_PADDLE_AND_WALL,0.0);
    for (paddle, paddleside) in paddles.iter() {
        match paddleside.0 {
            PlayerSide::Left => {
                paddle_l.x = paddle.translation.x;
                paddle_l.y = paddle.translation.y;
            }
            PlayerSide::Right => {
                paddle_r.x = paddle.translation.x;
                paddle_r.y = paddle.translation.y;
            }
        }
    }
    GameState{
        ball_loc: Vec2::new(ball.0.translation.x,ball.0.translation.y),
        ball_velocity: **ball.1,
        paddle_l_loc: paddle_l,
        paddle_r_loc: paddle_r,
        score_l: scoreboard.scoreleft as i32,
        score_r: scoreboard.scoreright as i32,
        playing: playing.0,
    }
}

/// Takes the GameState struct and actually applies it to the various changing objects throughout the game.
/// Used to update the client with information from the server.
pub fn set_gamestate(
    ball: &mut Query<(&mut Transform, &mut Velocity), (With<Ball>,Without<Paddle>)>,
    paddles: &mut Query<(&mut Transform,&PaddleSide), With<Paddle>>, 
    scoreboard: &mut ResMut<Scoreboard>,
    playing: &mut ResMut<Playing>,
    gamestate: GameState) {
    let (mut ball_loc, mut ball_vel) = ball.single_mut();
    ball_loc.translation.x = gamestate.ball_loc.x;
    ball_loc.translation.y = gamestate.ball_loc.y;
    ball_vel.x = gamestate.ball_velocity.x;
    ball_vel.y = gamestate.ball_velocity.y;
    for (mut paddle, paddleside) in paddles.iter_mut() {
        match paddleside.0 {
            PlayerSide::Left => {
                paddle.translation.x = gamestate.paddle_l_loc.x;
                paddle.translation.y = gamestate.paddle_l_loc.y;
            }
            PlayerSide::Right => {
                paddle.translation.x = gamestate.paddle_r_loc.x;
                paddle.translation.y = gamestate.paddle_r_loc.y;
            }
        }
    }
    scoreboard.scoreleft = gamestate.score_l as usize;
    scoreboard.scoreright = gamestate.score_r as usize;
    playing.0 = gamestate.playing;
}


/// Add the game's entities to our world
/// Specific to the client as it uses sprites and assets.
fn setup_client(mut commands: Commands, asset_server: Res<AssetServer>) {
    // Camera
    commands.spawn_bundle(Camera2dBundle::default());

    // Sound
    let ball_collision_sound = asset_server.load("sounds/breakout_collision.ogg");
    commands.insert_resource(CollisionSound(ball_collision_sound));

    // Paddle
    let paddle_x_left = LEFT_WALL + GAP_BETWEEN_PADDLE_AND_WALL;
    let paddle_x_right = RIGHT_WALL - GAP_BETWEEN_PADDLE_AND_WALL;

    commands
        .spawn()
        .insert(Paddle)
        .insert(PaddleSide(PlayerSide::Left))
        .insert(Movable)
        .insert_bundle(SpriteBundle {
            transform: Transform {
                translation: Vec3::new(paddle_x_left, 0.0, 0.0),
                scale: PADDLE_SIZE,
                ..default()
            },
            sprite: Sprite {
                color: PADDLE_COLOR,
                ..default()
            },
            ..default()
        })
        .insert(Collider);

    commands
        .spawn()
        .insert(Paddle)
        .insert(PaddleSide(PlayerSide::Right))
        .insert(Movable)
        .insert_bundle(SpriteBundle {
            transform: Transform {
                translation: Vec3::new(paddle_x_right, 0.0, 0.0),
                scale: PADDLE_SIZE,
                ..default()
            },
            sprite: Sprite {
                color: PADDLE_COLOR,
                ..default()
            },
            ..default()
        })
        .insert(Collider);

    // Ball
    commands
        .spawn()
        .insert(Ball{lastpointleft: false})
        .insert(Movable)
        .insert_bundle(SpriteBundle {
            transform: Transform {
                scale: BALL_SIZE,
                translation: BALL_STARTING_POSITION,
                ..default()
            },
            sprite: Sprite {
                color: BALL_COLOR,
                ..default()
            },
            ..default()
        })
        .insert(Velocity(INITIAL_BALL_DIRECTION.normalize() * BALL_SPEED));

    // Scoreboard
    commands.spawn_bundle(
        TextBundle::from_sections([
            TextSection::new(
                "Score p1: ",
                TextStyle {
                    font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                    font_size: SCOREBOARD_FONT_SIZE,
                    color: TEXT_COLOR,
                },
            ),
            TextSection::from_style(TextStyle {
                font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                font_size: SCOREBOARD_FONT_SIZE,
                color: SCORE_COLOR,
            }),
            TextSection::new(
                "Score p2: ",
                TextStyle {
                    font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                    font_size: SCOREBOARD_FONT_SIZE,
                    color: TEXT_COLOR,
                },
            ),
            TextSection::from_style(TextStyle {
                font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                font_size: SCOREBOARD_FONT_SIZE,
                color: SCORE_COLOR,
            }),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            position: UiRect {
                top: SCOREBOARD_TEXT_PADDING,
                left: SCOREBOARD_TEXT_PADDING,
                ..default()
            },
            ..default()
        }),
    );

    // Walls
    commands.spawn_bundle(WallBundle::new(WallLocation::Left)).insert(Wall);
    commands.spawn_bundle(WallBundle::new(WallLocation::Right)).insert(Wall);
    commands.spawn_bundle(WallBundle::new(WallLocation::Bottom)).insert(Wall);
    commands.spawn_bundle(WallBundle::new(WallLocation::Top)).insert(Wall);
}

/// Adds the game's entities to the world.
/// Specific to the server as it strips all of the sprites and assets used in the client setup.
fn setup_server(mut commands: Commands) {

    // Paddle
    let paddle_x_left = LEFT_WALL + GAP_BETWEEN_PADDLE_AND_WALL;
    let paddle_x_right = RIGHT_WALL - GAP_BETWEEN_PADDLE_AND_WALL;

    commands
        .spawn()
        .insert(Paddle)
        .insert(PaddleSide(PlayerSide::Left))
        .insert(Movable)
        .insert(Transform {
            translation: Vec3::new(paddle_x_left, 0.0, 0.0),
            scale: PADDLE_SIZE,
            ..default()
        })
        .insert(Collider);

    commands
        .spawn()
        .insert(Paddle)
        .insert(PaddleSide(PlayerSide::Right))
        .insert(Movable)
        .insert(Transform {
            translation: Vec3::new(paddle_x_right, 0.0, 0.0),
            scale: PADDLE_SIZE,
            ..default()
        })
        .insert(Collider);

    // Ball
    commands
        .spawn()
        .insert(Ball{lastpointleft: false})
        .insert(Movable)
        .insert(Transform {
            scale: BALL_SIZE,
            translation: BALL_STARTING_POSITION,
            ..default()
        })
        .insert(Velocity(INITIAL_BALL_DIRECTION.normalize() * BALL_SPEED));

    // Walls
    commands.spawn_bundle(WallBundleServer::new(WallLocation::Left)).insert(Wall);
    commands.spawn_bundle(WallBundleServer::new(WallLocation::Right)).insert(Wall);
    commands.spawn_bundle(WallBundleServer::new(WallLocation::Bottom)).insert(Wall);
    commands.spawn_bundle(WallBundleServer::new(WallLocation::Top)).insert(Wall);
}

/// Applies velocity and makes sure we aren't passing through any objects.
fn apply_velocity(
    mut query: Query<(&mut Transform, &Velocity), (Without<Paddle>,Without<Wall>)>, 
    query_paddles: Query<&Transform, With<Paddle>>, 
    query_walls: Query<(&Transform, &WallLoc), With<Wall>>
) {
    for (mut transform, velocity) in &mut query {
        let (pastx, pasty) = (transform.translation.x,transform.translation.y);
        let opp_dir_x = -1.0 * signum(velocity.x);
        let opp_dir_y = -1.0 * signum(velocity.y);
        transform.translation.x += velocity.x * TIME_STEP;
        transform.translation.y += velocity.y * TIME_STEP;
        
        // Check if paddles are between here and our next position.
        for tr in query_paddles.iter() {
            let towardsy = (2 * ((signum(transform.translation.y - pasty) == signum(velocity.y)) as i32) - 1) as f32;
            if (pasty - tr.translation.y).abs() < towardsy*velocity.y*TIME_STEP + PADDLE_SIZE.y/2.0 
            && tr.translation.x < pastx.max(transform.translation.x)
            && tr.translation.x > pastx.min(transform.translation.x) 
            {
                // They are. Set our position so that it just collides with the paddle instead of going through.
                // Collision system should pick it up from here.
                transform.translation.x = tr.translation.x + (BALL_SIZE.x * 0.5 * opp_dir_x);
                let distx = transform.translation.x - pastx;
                let dist_t = distx / velocity.x;
                transform.translation.y = pasty + velocity.y * dist_t;
            }
        }
        // Check if walls are between here and our next position.
        for (tr, wl) in query_walls.iter() {
            let is_horizontal = match wl.0 {
                WallLocation::Left => false,
                WallLocation::Right => false,
                _ => true
            };

            // Check Y against top and bottom walls.
            if is_horizontal
            && tr.translation.y < pasty.max(transform.translation.y)
            && tr.translation.y > pasty.min(transform.translation.y) 
            {
                // Prevent ball from passing through the wall by setting it to just collide with wall. 
                // Collision system should pick it up from here.
                transform.translation.y = tr.translation.y + (BALL_SIZE.y * 0.5 * opp_dir_y);
                let disty = transform.translation.y - pasty;
                let dist_t = disty / velocity.y;
                transform.translation.x = (pastx + velocity.x * dist_t).clamp(LEFT_WALL,RIGHT_WALL);
            }

            // Check X against left and right walls.
            if !is_horizontal
            && tr.translation.x < pastx.max(transform.translation.x)
            && tr.translation.x > pastx.min(transform.translation.x) 
            {
                // Prevent ball from passing through the wall by setting it to just collide with wall. 
                // Collision system should pick it up from here.
                transform.translation.x = tr.translation.x + (BALL_SIZE.x * 0.5 * opp_dir_x);
                let distx = transform.translation.x - pastx;
                let dist_t = distx / velocity.x;
                transform.translation.y = (pasty + velocity.y * dist_t).clamp(TOP_WALL,BOTTOM_WALL);
            }
        }
    }
}

fn update_scoreboard(scoreboard: Res<Scoreboard>, mut query: Query<&mut Text>) {
    if let Some(mut text) = query.iter_mut().next(){
        text.sections[1].value = scoreboard.scoreleft.to_string();
        text.sections[3].value = scoreboard.scoreright.to_string();
    }
}

fn check_for_collisions(
    mut scoreboard: ResMut<Scoreboard>,
    mut ball_query: Query<(&mut Velocity, &mut Transform, &mut Ball), With<Ball>>,
    collider_query: Query<(Entity, &Transform, Option<&Paddle>), (With<Collider>,Without<Ball>)>,
    mut collision_events: EventWriter<CollisionEvent>,
    mut timer: ResMut<RespawnTimer>,
) {
    let (mut ball_velocity, mut ball_transform, mut ball) = ball_query.single_mut();
    let ball_size = ball_transform.scale.truncate();

    // check collision with walls
    for (_, transform, maybe_paddle) in &collider_query {
        let collision = collide(
            ball_transform.translation,
            ball_size,
            transform.translation,
            transform.scale.truncate(),
        );
        if let Some(collision) = collision {
            // Sends a collision event so that other systems can react to the collision
            collision_events.send_default();

            let mut is_wall = true;

            // Did we collide with a paddle?
            if maybe_paddle.is_some() {
                // If we collided with a paddle, we didn't collide with a wall.
                is_wall = false;
                // Increase the ball velocity by 1.1x
                // This is to apply pressure to the players and prevent drawn out matches.
                // Also clamp it below our max speed, otherwise it can become unplayable.
                ball_velocity.x = (ball_velocity.x*BALL_SPEED_INCREASE).clamp(-MAX_BALL_SPEED,MAX_BALL_SPEED);
                // Set the Y velocity proportionally to how far from the center of the paddle we hit.
                // This is to give the player more control over where the ball goes.
                ball_velocity.y = signum(ball_velocity.y)*(ball_velocity.x * (ball_transform.translation.y - transform.translation.y) / (PADDLE_SIZE.y/3.0)).abs();
            }

            // reflect the ball when it collides
            let mut reflect_x = false;
            let mut reflect_y = false;

            // despawn when we hit the bottom wall
            // doesn't actually despawn, just resets it.
            let mut despawn = false;

            // only reflect if the ball's velocity is going in the opposite direction of the
            // collision
            match (collision, is_wall) {
                (Collision::Left, true) => {
                    scoreboard.scoreleft += 1;
                    ball.lastpointleft = false;
                    despawn = true;
                },
                (Collision::Right, true) => {
                    scoreboard.scoreright += 1;
                    ball.lastpointleft = true;
                    despawn = true;
                },
                (Collision::Left, false) => reflect_x = ball_velocity.x > 0.0,
                (Collision::Right, false) => reflect_x = ball_velocity.x < 0.0,
                (Collision::Top, _) => reflect_y = ball_velocity.y < 0.0,
                (Collision::Bottom, _) => reflect_y = ball_velocity.y > 0.0,
                (Collision::Inside, _) => { /* do nothing */ }
            }

            // If we need to despawn, set our speed to 0 and reset our position.
            if despawn {
                ball_velocity.x = 0.0;
                ball_velocity.y = 0.0;
                ball_transform.translation.x = BALL_STARTING_POSITION.x;
                ball_transform.translation.y = BALL_STARTING_POSITION.x;
                timer.0.reset();
            }

            // reflect velocity on the x-axis if we hit something on the x-axis
            if reflect_x {
                ball_velocity.x = -ball_velocity.x;
            }

            // reflect velocity on the y-axis if we hit something on the y-axis
            if reflect_y {
                ball_velocity.y = -ball_velocity.y;
            }
        }
    }
}

/// Simply checks if the ball should respawn yet.
fn respawn_ball(time: Res<Time>, mut timer: ResMut<RespawnTimer>, mut ball_query: Query<(&mut Velocity, &Ball), With<Ball>>) {
    if timer.0.tick(time.delta()).just_finished() {
        let (mut ball_velocity, ball )= ball_query.single_mut();
        // Choose an angle that is in a 60 degree triangle of whoever was scored on last.
        let init_angle = random::<f32>() * 60.0 - 30.0 + (180 * ball.lastpointleft as i32) as f32;
        // Convert to cartesian coordinates representative of our angle.
        let init_dir = Vec2::from_angle(init_angle * DEG_TO_RAD);
        // Give it the starting speed in the direction we specified previously.
        let ball_velocity_default = Velocity(init_dir * BALL_SPEED);
        // Actually set the velocity now.
        ball_velocity.x = ball_velocity_default.x;
        ball_velocity.y = ball_velocity_default.y;
    }
}

fn play_collision_sound(
    collision_events: EventReader<CollisionEvent>,
    audio: Res<Audio>,
    sound: Res<CollisionSound>,
) {
    // Play a sound once per frame if a collision occurred.
    if !collision_events.is_empty() {
        // This prevents events staying active on the next frame.
        collision_events.clear();
        audio.play(sound.0.clone());
    }
}
