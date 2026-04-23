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
const TILE_COASTER: u8 = 2;

const TOOL_PATH: u32 = 0;
const TOOL_COASTER: u32 = 1;
const TOOL_BULLDOZE: u32 = 2;

const PATH_COST: i32 = 1;
const COASTER_COST: i32 = 50;
const RIDE_FEE: i32 = 10;
const STARTING_MONEY: i32 = 100;

const MAX_GUESTS: usize = 64;
const SPAWN_INTERVAL_MS: u32 = 2000;
const GUEST_STEP_MS: u32 = 300;
const RIDE_DURATION_MS: u32 = 1500;

const STATE_FREE: u8 = 0;
const STATE_TO_RIDE: u8 = 1;
const STATE_RIDING: u8 = 2;
const STATE_TO_EXIT: u8 = 3;

#[derive(Clone, Copy)]
struct Guest {
    state: u8,
    tile: u16,
    step_timer: u32,
    ride_timer: u32,
}

struct World {
    tiles: [u8; N],
    guests: [Guest; MAX_GUESTS],
    money: i32,
    spawn_timer: u32,
    rng: u32,
    parent: [i16; N],
    coaster_tile: i16,
}

static mut WORLD: World = World {
    tiles: [TILE_GRASS; N],
    guests: [Guest { state: STATE_FREE, tile: 0, step_timer: 0, ride_timer: 0 }; MAX_GUESTS],
    money: STARTING_MONEY,
    spawn_timer: 0,
    rng: 1,
    parent: [-1; N],
    coaster_tile: -1,
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

fn xy_to_idx(x: usize, y: usize) -> usize { y * W + x }
fn idx_to_x(i: usize) -> usize { i % W }
fn idx_to_y(i: usize) -> usize { i / W }

// Is this tile walkable for a guest (path, coaster, or entrance which is grass at (0,7))
fn walkable(tiles: &[u8; N], i: usize) -> bool {
    if i == ENTRANCE_IDX as usize { return true; }
    let t = tiles[i];
    t == TILE_PATH || t == TILE_COASTER
}

// BFS from entrance over walkable tiles, populating parent[] for shortest path back.
// Also locates the nearest coaster tile and stores it in coaster_tile.
fn recompute_paths(w: &mut World) {
    let mut queue: [u16; N] = [0; N];
    let mut visited: [bool; N] = [false; N];
    let mut head = 0usize;
    let mut tail = 0usize;

    for i in 0..N { w.parent[i] = -1; }
    w.coaster_tile = -1;

    let start = ENTRANCE_IDX as usize;
    queue[tail] = start as u16; tail += 1;
    visited[start] = true;

    while head < tail {
        let cur = queue[head] as usize; head += 1;
        if w.tiles[cur] == TILE_COASTER && w.coaster_tile < 0 {
            w.coaster_tile = cur as i16;
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
    if w.coaster_tile < 0 { return false; }
    for g in w.guests.iter_mut() {
        if g.state == STATE_FREE {
            g.state = STATE_TO_RIDE;
            g.tile = ENTRANCE_IDX;
            g.step_timer = 0;
            g.ride_timer = 0;
            return true;
        }
    }
    false
}

// Find next step from `from` toward `target` using parent tree rooted at entrance.
// The tree points children → parents (toward entrance). To walk FROM entrance TO target,
// we need to invert: from current node, pick the neighbor whose parent chain to entrance is
// one shorter than ours. Simpler approach: BFS from current to target on the fly.
fn step_toward(w: &World, from: u16, target: u16) -> Option<u16> {
    if from == target { return None; }
    let mut queue: [u16; N] = [0; N];
    let mut parent: [i16; N] = [-1; N];
    let mut visited: [bool; N] = [false; N];
    let mut head = 0usize;
    let mut tail = 0usize;
    queue[tail] = from; tail += 1;
    visited[from as usize] = true;

    let mut found = false;
    while head < tail {
        let cur = queue[head] as usize; head += 1;
        if cur as u16 == target { found = true; break; }
        let cx = idx_to_x(cur); let cy = idx_to_y(cur);
        let neighbors: [(i32, i32); 4] = [(1,0),(-1,0),(0,1),(0,-1)];
        for (dx, dy) in neighbors.iter() {
            let nx = cx as i32 + dx; let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 || nx >= W as i32 || ny >= H as i32 { continue; }
            let ni = xy_to_idx(nx as usize, ny as usize);
            if visited[ni] { continue; }
            if !walkable(&w.tiles, ni) { continue; }
            visited[ni] = true;
            parent[ni] = cur as i16;
            queue[tail] = ni as u16; tail += 1;
        }
    }
    if !found { return None; }
    // Walk parent chain from target back to the step after `from`.
    let mut cur = target as i16;
    loop {
        let p = parent[cur as usize];
        if p < 0 { return None; }
        if p as u16 == from { return Some(cur as u16); }
        cur = p;
    }
}

fn guest_tick(w: &mut World, gi: usize, dt: u32) {
    let state = w.guests[gi].state;
    match state {
        STATE_TO_RIDE => {
            if w.coaster_tile < 0 {
                w.guests[gi].state = STATE_TO_EXIT;
                return;
            }
            let target = w.coaster_tile as u16;
            let mut timer = w.guests[gi].step_timer + dt;
            while timer >= GUEST_STEP_MS {
                timer -= GUEST_STEP_MS;
                let cur = w.guests[gi].tile;
                if cur == target {
                    w.guests[gi].state = STATE_RIDING;
                    w.guests[gi].ride_timer = 0;
                    w.guests[gi].step_timer = 0;
                    return;
                }
                match step_toward(w, cur, target) {
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
            let t = w.guests[gi].ride_timer + dt;
            if t >= RIDE_DURATION_MS {
                w.money += RIDE_FEE;
                w.guests[gi].state = STATE_TO_EXIT;
                w.guests[gi].ride_timer = 0;
                w.guests[gi].step_timer = 0;
            } else {
                w.guests[gi].ride_timer = t;
            }
        }
        STATE_TO_EXIT => {
            let mut timer = w.guests[gi].step_timer + dt;
            while timer >= GUEST_STEP_MS {
                timer -= GUEST_STEP_MS;
                let cur = w.guests[gi].tile;
                if cur == ENTRANCE_IDX {
                    w.guests[gi].state = STATE_FREE;
                    return;
                }
                match step_toward(w, cur, ENTRANCE_IDX) {
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
    w.guests = [Guest { state: STATE_FREE, tile: 0, step_timer: 0, ride_timer: 0 }; MAX_GUESTS];
    w.money = STARTING_MONEY;
    w.spawn_timer = 0;
    w.rng = if seed == 0 { 1 } else { seed };
    w.parent = [-1; N];
    w.coaster_tile = -1;
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
    // Consume rng so it's not unused.
    let _ = xorshift(&mut w.rng);
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
        TOOL_COASTER => {
            // One coaster per park.
            for i in 0..N { if w.tiles[i] == TILE_COASTER { return 0; } }
            if w.tiles[idx] != TILE_GRASS { return 0; }
            if w.money < COASTER_COST { return 0; }
            w.money -= COASTER_COST;
            w.tiles[idx] = TILE_COASTER;
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
