use std::f64::consts::TAU;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use log::LevelFilter;
use valence::client::GameMode;
use valence::config::{Config, ServerListPing};
use valence::text::Color;
use valence::util::to_yaw_and_pitch;
use valence::{
    async_trait, DimensionId, EntityId, EntityType, Server, ShutdownResult, Text,
    TextFormat, WorldId, Worlds, Client,
};
use vek::{Mat3, Vec3};

pub fn main() -> ShutdownResult {
    env_logger::Builder::new()
        .filter_module("valence", LevelFilter::Trace)
        .parse_default_env()
        .init();

    valence::start_server(Game {
        player_count: AtomicUsize::new(0),
        cows: Mutex::new(Vec::new()),
    })
}

struct Game {
    player_count: AtomicUsize,
    cows: Mutex<Vec<EntityId>>,
}

const MAX_PLAYERS: usize = 10;

#[async_trait]
impl Config for Game {
    fn max_connections(&self) -> usize {
        // We want status pings to be successful even if the server is full.
        MAX_PLAYERS + 64
    }

    fn online_mode(&self) -> bool {
        // You'll want this to be true on real servers.
        false
    }

    async fn server_list_ping(&self, _server: &Server, _remote_addr: SocketAddr) -> ServerListPing {
        ServerListPing::Respond {
            online_players: self.player_count.load(Ordering::SeqCst) as i32,
            max_players: MAX_PLAYERS as i32,
            description: "Hello Valence!".color(Color::AQUA),
            favicon_png: Some(include_bytes!("favicon.png")),
        }
    }

    fn join(
        &self,
        _server: &Server,
        _client: &mut Client,
        worlds: &mut Worlds,
    ) -> Result<WorldId, Text> {
        if let Ok(_) = self
            .player_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                (count < MAX_PLAYERS).then(|| count + 1)
            })
        {
            Ok(worlds.iter().next().unwrap().0)
        } else {
            Err("The server is full!".into())
        }
    }

    fn init(&self, _server: &Server, worlds: &mut Worlds) {
        let world = worlds.create(DimensionId::default()).1;
        world.meta.set_flat(true);

        let size = 5;
        for z in -size..size {
            for x in -size..size {
                world.chunks.create([x, z]);
            }
        }

        self.cows.lock().unwrap().extend((0..200).map(|_| {
            let (id, e) = world.entities.create();
            e.set_type(EntityType::Cow);
            id
        }));
    }

    fn update(&self, server: &Server, worlds: &mut Worlds) {
        let world = worlds.iter_mut().next().unwrap().1;

        world.clients.retain(|_, client| {
            if client.created_tick() == server.current_tick() {
                client.set_game_mode(GameMode::Creative);
                client.teleport([0.0, 200.0, 0.0], 0.0, 0.0);

                world.meta.player_list_mut().insert(
                    client.uuid(),
                    client.username().to_string(),
                    client.textures().cloned(),
                    client.game_mode(),
                    0,
                    None,
                );
            }

            if client.is_disconnected() {
                self.player_count.fetch_sub(1, Ordering::SeqCst);
                false
            } else {
                true
            }
        });

        let time = server.current_tick() as f64 / server.tick_rate() as f64;

        let rot = Mat3::rotation_x(time * TAU * 0.1)
            .rotated_y(time * TAU * 0.2)
            .rotated_z(time * TAU * 0.3);

        let cows = self.cows.lock().unwrap();
        let cow_count = cows.len();

        let radius = 6.0 + ((time * TAU / 2.5).sin() + 1.0) / 2.0 * 10.0;

        // TODO: use eye position.
        let player_pos = world
            .clients
            .iter()
            .next()
            .map(|c| c.1.position())
            .unwrap_or_default();

        for (cow_id, p) in cows.iter().cloned().zip(fibonacci_spiral(cow_count)) {
            let cow = world.entities.get_mut(cow_id).expect("missing cow");
            let rotated = p * rot;
            let transformed = rotated * radius + [0.0, 100.0, 0.0];

            let yaw = f32::atan2(rotated.z as f32, rotated.x as f32).to_degrees() - 90.0;
            let (looking_yaw, looking_pitch) =
                to_yaw_and_pitch((player_pos - transformed).normalized());

            cow.set_position(transformed);
            cow.set_yaw(yaw);
            cow.set_pitch(looking_pitch);
            cow.set_head_yaw(looking_yaw);
        }
    }
}

/// Distributes N points on the surface of a unit sphere.
fn fibonacci_spiral(n: usize) -> impl Iterator<Item = Vec3<f64>> {
    (0..n).map(move |i| {
        let golden_ratio = (1.0 + 5_f64.sqrt()) / 2.0;

        // Map to unit square
        let x = i as f64 / golden_ratio % 1.0;
        let y = i as f64 / n as f64;

        // Map from unit square to unit sphere.
        let theta = x * TAU;
        let phi = (1.0 - 2.0 * y).acos();
        Vec3::new(theta.cos() * phi.sin(), theta.sin() * phi.sin(), phi.cos())
    })
}
