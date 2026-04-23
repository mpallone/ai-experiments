#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

const W: usize = 20;
const H: usize = 15;
const N: usize = W * H;
const ENTRANCE_X: usize = 0;
const ENTRANCE_Y: usize = 7;
const ENTRANCE_IDX: u16 = (ENTRANCE_Y * W + ENTRANCE_X) as u16;

const TILE_GRASS: u8 = 0;
const TILE_PATH: u8 = 1;
const TILE_TRACK: u8 = 2;

const TOOL_PATH: u32 = 0;
const TOOL_TRACK: u32 = 1;
const TOOL_BULLDOZE: u32 = 2;

const PATH_COST: i32 = 1;
const TRACK_COST: i32 = 10;
const RIDE_FEE: i32 = 15;
const STARTING_MONEY: i32 = 100;

const MAX_GUESTS: usize = 64;
const SPAWN_INTERVAL_MS: u32 = 2000;
const GUEST_STEP_MS: u32 = 300;
const RIDE_STEP_MS: u32 = 120;
const MAX_RIDE_TILES: u32 = 32;

const STATE_FREE: u8 = 0;
const STATE_TO_RIDE: u8 = 1;
const STATE_RIDING: u8 = 2;
const STATE_TO_EXIT: u8 = 3;

#[derive(Clone, Copy)]
struct Guest {
    state: u8,
    tile: u16,
    step_timer: u32,
    ride_tiles_visited: u8,
    ride_phase: u8,
    ride_origin: u16,
    // Per-guest seed used to permute BFS neighbor order so guests disperse
    // across equal-length paths at forks instead of all taking the same branch.
    path_seed: u32,
}

struct World {
    tiles: [u8; N],
    guests: [Guest; MAX_GUESTS],
    money: i32,
    spawn_timer: u32,
    rng: u32,
    parent: [i16; N],
    boarding_tile: i16,
}

static mut WORLD: World = World {
    tiles: [TILE_GRASS; N],
    guests: [Guest {
        state: STATE_FREE,
        tile: 0,
        step_timer: 0,
        ride_tiles_visited: 0,
        ride_phase: 0,
        ride_origin: 0,
        path_seed: 0,
    }; MAX_GUESTS],
    money: STARTING_MONEY,
    spawn_timer: 0,
    rng: 1,
    parent: [-1; N],
    boarding_tile: -1,
};

fn world() -> &'static mut World {
    unsafe { &mut *core::ptr::addr_of_mut!(WORLD) }
}

fn xorshift(s: &mut u32) -> u32 {
    let mut x = *s;
    if x == 0 { x = 0x9E3779B9; }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *s = x;
    x
}

// Deterministic Fisher-Yates shuffle of the 4 grid neighbors, driven by a
// per-guest seed so each guest breaks ties consistently but differently.
fn shuffled_neighbors(seed: u32) -> [(i32, i32); 4] {
    let mut a: [(i32, i32); 4] = [(1,0),(-1,0),(0,1),(0,-1)];
    let mut s = if seed == 0 { 0x9E3779B9 } else { seed };
    let mut i = 3usize;
    while i > 0 {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let j = (s as usize) % (i + 1);
        a.swap(i, j);
        i -= 1;
    }
    a
}

fn xy_to_idx(x: usize, y: usize) -> usize { y * W + x }
fn idx_to_x(i: usize) -> usize { i % W }
fn idx_to_y(i: usize) -> usize { i / W }

// Is this tile walkable for a guest (path, track, or the entrance at (0,7))
fn walkable(tiles: &[u8; N], i: usize) -> bool {
    if i == ENTRANCE_IDX as usize { return true; }
    let t = tiles[i];
    t == TILE_PATH || t == TILE_TRACK
}

fn is_track(tiles: &[u8; N], i: usize) -> bool {
    tiles[i] == TILE_TRACK
}

// BFS from entrance over walkable tiles, populating parent[] for shortest path back.
// Also locates the nearest track tile (the boarding point) in boarding_tile.
fn recompute_paths(w: &mut World) {
    let mut queue: [u16; N] = [0; N];
    let mut visited: [bool; N] = [false; N];
    let mut head = 0usize;
    let mut tail = 0usize;

    for i in 0..N { w.parent[i] = -1; }
    w.boarding_tile = -1;

    let start = ENTRANCE_IDX as usize;
    queue[tail] = start as u16; tail += 1;
    visited[start] = true;

    while head < tail {
        let cur = queue[head] as usize; head += 1;
        if w.tiles[cur] == TILE_TRACK && w.boarding_tile < 0 {
            w.boarding_tile = cur as i16;
        }
        let cx = idx_to_x(cur); let cy = idx_to_y(cur);
        let neighbors: [(i32, i32); 4] = [(1,0),(-1,0),(0,1),(0,-1)];
        for (dx, dy) in neighbors.iter() {
            let nx = cx as i32 + dx; let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 { continue; }
            let ni = xy_to_idx(nx as usize, ny as usize);
            if visited[ni] { continue; }
            if !walkable(&w.tiles, ni) { continue; }
            visited[ni] = true;
            w.parent[ni] = cur as i16;
            queue[tail] = ni as u16; tail += 1;
        }
    }
}

fn try_spawn_guest(w: &mut World) -> bool {
    if w.boarding_tile < 0 { return false; }
    let seed = xorshift(&mut w.rng);
    for g in w.guests.iter_mut() {
        if g.state == STATE_FREE {
            g.state = STATE_TO_RIDE;
            g.tile = ENTRANCE_IDX;
            g.step_timer = 0;
            g.ride_tiles_visited = 0;
            g.ride_phase = 0;
            g.ride_origin = 0;
            g.path_seed = seed;
            return true;
        }
    }
    false
}

// BFS from `from` toward `target` across tiles matching `filter`. Returns the next
// step along the shortest path, or None if unreachable.
fn step_bfs(
    tiles: &[u8; N],
    from: u16,
    target: u16,
    filter: fn(&[u8; N], usize) -> bool,
    seed: u32,
) -> Option<u16> {
    if from == target { return None; }
    let mut queue: [u16; N] = [0; N];
    let mut parent: [i16; N] = [-1; N];
    let mut visited: [bool; N] = [false; N];
    let mut head = 0usize;
    let mut tail = 0usize;
    queue[tail] = from; tail += 1;
    visited[from as usize] = true;

    let neighbors = shuffled_neighbors(seed);
    let mut found = false;
    while head < tail {
        let cur = queue[head] as usize; head += 1;
        if cur as u16 == target { found = true; break; }
        let cx = idx_to_x(cur); let cy = idx_to_y(cur);
        for (dx, dy) in neighbors.iter() {
            let nx = cx as i32 + dx; let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 { continue; }
            let ni = xy_to_idx(nx as usize, ny as usize);
            if visited[ni] { continue; }
            if !filter(tiles, ni) { continue; }
            visited[ni] = true;
            parent[ni] = cur as i16;
            queue[tail] = ni as u16; tail += 1;
        }
    }
    if !found { return None; }
    let mut cur = target as i16;
    loop {
        let p = parent[cur as usize];
        if p < 0 { return None; }
        if p as u16 == from { return Some(cur as u16); }
        cur = p;
    }
}

fn step_toward(w: &World, from: u16, target: u16, seed: u32) -> Option<u16> {
    step_bfs(&w.tiles, from, target, walkable, seed)
}

fn step_along_track(w: &World, from: u16, target: u16, seed: u32) -> Option<u16> {
    step_bfs(&w.tiles, from, target, is_track, seed)
}

// Pick the farthest reachable track tile from `boarding` using BFS over track-only
// tiles. Returns `boarding` itself if there are no adjacent track tiles.
fn pick_ride_target(w: &World, boarding: u16) -> u16 {
    let mut queue: [u16; N] = [0; N];
    let mut visited: [bool; N] = [false; N];
    let mut head = 0usize;
    let mut tail = 0usize;
    queue[tail] = boarding; tail += 1;
    visited[boarding as usize] = true;
    let mut last = boarding;

    while head < tail {
        let cur = queue[head] as usize; head += 1;
        last = cur as u16;
        let cx = idx_to_x(cur); let cy = idx_to_y(cur);
        let neighbors: [(i32, i32); 4] = [(1,0),(-1,0),(0,1),(0,-1)];
        for (dx, dy) in neighbors.iter() {
            let nx = cx as i32 + dx; let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 { continue; }
            let ni = xy_to_idx(nx as usize, ny as usize);
            if visited[ni] { continue; }
            if !is_track(&w.tiles, ni) { continue; }
            visited[ni] = true;
            queue[tail] = ni as u16; tail += 1;
        }
    }
    last
}

fn guest_tick(w: &mut World, gi: usize, dt: u32) {
    let state = w.guests[gi].state;
    match state {
        STATE_TO_RIDE => {
            if w.boarding_tile < 0 {
                w.guests[gi].state = STATE_TO_EXIT;
                return;
            }
            let target = w.boarding_tile as u16;
            let mut timer = w.guests[gi].step_timer + dt;
            while timer >= GUEST_STEP_MS {
                timer -= GUEST_STEP_MS;
                let cur = w.guests[gi].tile;
                if cur == target {
                    w.guests[gi].state = STATE_RIDING;
                    w.guests[gi].ride_origin = cur;
                    w.guests[gi].ride_tiles_visited = 0;
                    w.guests[gi].ride_phase = 0;
                    w.guests[gi].step_timer = 0;
                    return;
                }
                let seed = w.guests[gi].path_seed;
                match step_toward(w, cur, target, seed) {
                    Some(next) => { w.guests[gi].tile = next; }
                    None => {
                        w.guests[gi].state = STATE_TO_EXIT;
                        w.guests[gi].step_timer = 0;
                        return;
                    }
                }
            }
            w.guests[gi].step_timer = timer;
        }
        STATE_RIDING => {
            let boarding = w.guests[gi].ride_origin;
            let far = pick_ride_target(w, boarding);
            let mut timer = w.guests[gi].step_timer + dt;
            while timer >= RIDE_STEP_MS {
                timer -= RIDE_STEP_MS;
                let cur = w.guests[gi].tile;
                let target = if w.guests[gi].ride_phase == 0 { far } else { boarding };
                let seed = w.guests[gi].path_seed;
                match step_along_track(w, cur, target, seed) {
                    Some(next) => {
                        w.guests[gi].tile = next;
                        w.guests[gi].ride_tiles_visited =
                            w.guests[gi].ride_tiles_visited.saturating_add(1);
                        if next == far && w.guests[gi].ride_phase == 0 {
                            w.guests[gi].ride_phase = 1;
                        }
                        if w.guests[gi].ride_tiles_visited as u32 >= MAX_RIDE_TILES
                            && w.guests[gi].ride_phase == 0
                        {
                            w.guests[gi].ride_phase = 1;
                        }
                        if next == boarding && w.guests[gi].ride_phase == 1 {
                            w.money += RIDE_FEE;
                            w.guests[gi].state = STATE_TO_EXIT;
                            w.guests[gi].step_timer = 0;
                            return;
                        }
                    }
                    None => {
                        // Single-tile track or track bulldozed mid-ride: pay and leave.
                        w.money += RIDE_FEE;
                        w.guests[gi].state = STATE_TO_EXIT;
                        w.guests[gi].step_timer = 0;
                        return;
                    }
                }
            }
            w.guests[gi].step_timer = timer;
        }
        STATE_TO_EXIT => {
            // If a guest's tile was bulldozed out from under them, despawn cleanly.
            let cur_idx = w.guests[gi].tile as usize;
            if cur_idx != ENTRANCE_IDX as usize && !walkable(&w.tiles, cur_idx) {
                w.guests[gi].state = STATE_FREE;
                return;
            }
            let mut timer = w.guests[gi].step_timer + dt;
            while timer >= GUEST_STEP_MS {
                timer -= GUEST_STEP_MS;
                let cur = w.guests[gi].tile;
                if cur == ENTRANCE_IDX {
                    w.guests[gi].state = STATE_FREE;
                    return;
                }
                let seed = w.guests[gi].path_seed;
                match step_toward(w, cur, ENTRANCE_IDX, seed) {
                    Some(next) => { w.guests[gi].tile = next; }
                    None => {
                        w.guests[gi].state = STATE_FREE;
                        return;
                    }
                }
            }
            w.guests[gi].step_timer = timer;
        }
        _ => {}
    }
}

// ---- Exports ----

#[no_mangle]
pub extern "C" fn init(seed: u32) {
    let w = world();
    w.tiles = [TILE_GRASS; N];
    w.guests = [Guest {
        state: STATE_FREE,
        tile: 0,
        step_timer: 0,
        ride_tiles_visited: 0,
        ride_phase: 0,
        ride_origin: 0,
        path_seed: 0,
    }; MAX_GUESTS];
    w.money = STARTING_MONEY;
    w.spawn_timer = 0;
    w.rng = if seed == 0 { 1 } else { seed };
    w.parent = [-1; N];
    w.boarding_tile = -1;
}

#[no_mangle]
pub extern "C" fn tick(dt_ms: u32) {
    let w = world();
    let dt = if dt_ms > 500 { 500 } else { dt_ms }; // clamp to avoid huge jumps
    w.spawn_timer = w.spawn_timer.saturating_add(dt);
    while w.spawn_timer >= SPAWN_INTERVAL_MS {
        w.spawn_timer -= SPAWN_INTERVAL_MS;
        try_spawn_guest(w);
    }
    for i in 0..MAX_GUESTS {
        if w.guests[i].state != STATE_FREE {
            guest_tick(w, i, dt);
        }
    }
}

#[no_mangle]
pub extern "C" fn click(tile_x: u32, tile_y: u32, tool: u32) -> u32 {
    if tile_x >= W as u32 || tile_y >= H as u32 { return 0; }
    let w = world();
    let idx = xy_to_idx(tile_x as usize, tile_y as usize);

    // Entrance tile is untouchable.
    if idx == ENTRANCE_IDX as usize { return 0; }

    match tool {
        TOOL_PATH => {
            if w.tiles[idx] != TILE_GRASS { return 0; }
            if w.money < PATH_COST { return 0; }
            w.money -= PATH_COST;
            w.tiles[idx] = TILE_PATH;
        }
        TOOL_TRACK => {
            if w.tiles[idx] != TILE_GRASS { return 0; }
            if w.money < TRACK_COST { return 0; }
            w.money -= TRACK_COST;
            w.tiles[idx] = TILE_TRACK;
        }
        TOOL_BULLDOZE => {
            if w.tiles[idx] == TILE_GRASS { return 0; }
            w.tiles[idx] = TILE_GRASS;
        }
        _ => return 0,
    }
    recompute_paths(w);
    1
}

#[no_mangle]
pub extern "C" fn get_money() -> i32 { world().money }

#[no_mangle]
pub extern "C" fn get_guest_count() -> u32 {
    let w = world();
    let mut n = 0u32;
    for g in w.guests.iter() { if g.state != STATE_FREE { n += 1; } }
    n
}

#[no_mangle]
pub extern "C" fn get_tile(x: u32, y: u32) -> u32 {
    if x >= W as u32 || y >= H as u32 { return 0; }
    world().tiles[xy_to_idx(x as usize, y as usize)] as u32
}

// Returns packed: state(8) | x(8) | y(8) | reserved(8). 0 = free slot.
#[no_mangle]
pub extern "C" fn get_guest(i: u32) -> u32 {
    if i as usize >= MAX_GUESTS { return 0; }
    let g = world().guests[i as usize];
    if g.state == STATE_FREE { return 0; }
    let x = idx_to_x(g.tile as usize) as u32;
    let y = idx_to_y(g.tile as usize) as u32;
    (g.state as u32) | (x << 8) | (y << 16)
}

#[no_mangle]
pub extern "C" fn width() -> u32 { W as u32 }

#[no_mangle]
pub extern "C" fn height() -> u32 { H as u32 }

#[no_mangle]
pub extern "C" fn max_guests() -> u32 { MAX_GUESTS as u32 }

#[no_mangle]
pub extern "C" fn entrance_x() -> u32 { ENTRANCE_X as u32 }

#[no_mangle]
pub extern "C" fn entrance_y() -> u32 { ENTRANCE_Y as u32 }
