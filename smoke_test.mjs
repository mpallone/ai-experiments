import fs from "node:fs";

const wasm = fs.readFileSync("target/wasm32-unknown-unknown/release/rct_mvp.wasm");
const mod = await WebAssembly.instantiate(wasm, {});
const ex = mod.instance.exports;

console.log("exports:", Object.keys(ex).sort().join(","));

ex.init(42);

const W = ex.width();
const H = ex.height();
const ENT_X = ex.entrance_x();
const ENT_Y = ex.entrance_y();
console.log(`grid ${W}x${H}, entrance (${ENT_X},${ENT_Y}), money=${ex.get_money()}`);

// Build a straight path from (1,7) to (5,7), then a 4-tile track from (6,7) to (9,7).
for (let x = 1; x <= 5; x++) {
  const r = ex.click(x, 7, 0);
  if (!r) throw new Error(`path place failed at (${x},7)`);
}
for (let x = 6; x <= 9; x++) {
  const r = ex.click(x, 7, 1);
  if (!r) throw new Error(`track place failed at (${x},7)`);
}

console.log(`after build: money=${ex.get_money()}`);
// $100 - 5 paths * $1 - 4 track * $10 = 100 - 5 - 40 = 55
if (ex.get_money() !== 55) throw new Error(`expected money=55, got ${ex.get_money()}`);

if (ex.get_tile(6, 7) !== 2) throw new Error("boarding track tile not set");
if (ex.get_tile(9, 7) !== 2) throw new Error("far-end track tile not set");
if (ex.get_tile(3, 7) !== 1) throw new Error("path tile not set");

// Multi-placement is allowed (no one-per-park cap).
if (!ex.click(6, 8, 1)) throw new Error("second track tile should place");
if (ex.get_tile(6, 8) !== 2) throw new Error("second track tile not set");

// ---- Elevation ----
// Freshly placed track starts at height 0.
if (ex.get_height(7, 7) !== 0) throw new Error("new track should start at height 0");

// Raise tool is $2 per level.
const moneyBeforeRaise = ex.get_money();
if (!ex.click(7, 7, 3)) throw new Error("raise should succeed on track");
if (ex.get_height(7, 7) !== 1) throw new Error("height should be 1 after one raise");
if (ex.get_money() !== moneyBeforeRaise - 2) throw new Error("raise should cost $2");

// Raising clamps at MAX_HEIGHT (=5).
for (let i = 0; i < 4; i++) {
  if (!ex.click(7, 7, 3)) throw new Error(`raise ${i+2} should succeed`);
}
if (ex.get_height(7, 7) !== 5) throw new Error("height should cap at 5");
const moneyAtCap = ex.get_money();
if (ex.click(7, 7, 3)) throw new Error("raise past MAX_HEIGHT should fail");
if (ex.get_height(7, 7) !== 5) throw new Error("height should stay at 5");
if (ex.get_money() !== moneyAtCap) throw new Error("failed raise should not charge money");

// Lower is free and decrements.
if (!ex.click(7, 7, 4)) throw new Error("lower should succeed");
if (ex.get_height(7, 7) !== 4) throw new Error("height should drop to 4");
if (ex.get_money() !== moneyAtCap) throw new Error("lower should be free");

// Lower clamps at 0.
for (let i = 0; i < 4; i++) ex.click(7, 7, 4);
if (ex.get_height(7, 7) !== 0) throw new Error("height should reach 0");
if (ex.click(7, 7, 4)) throw new Error("lower below 0 should fail");
if (ex.get_height(7, 7) !== 0) throw new Error("height should stay at 0");

// Raise/lower only work on track tiles.
if (ex.click(2, 7, 3)) throw new Error("raise on path should fail");
if (ex.click(2, 7, 4)) throw new Error("lower on path should fail");
if (ex.click(15, 10, 3)) throw new Error("raise on grass should fail");

// Bulldozing a raised track resets its height.
ex.click(8, 7, 3);
ex.click(8, 7, 3);
if (ex.get_height(8, 7) !== 2) throw new Error("setup: expected height 2");
ex.click(8, 7, 2);
if (ex.get_tile(8, 7) !== 0) throw new Error("bulldoze should clear tile");
if (ex.get_height(8, 7) !== 0) throw new Error("bulldoze should reset height");

// ---- Loop tile ----
const TOOL_LOOP = 5;
const TILE_LOOP = 3;
const LOOP_COST = 25;

const m0 = ex.get_money();
if (!ex.click(10, 7, TOOL_LOOP)) throw new Error("loop place failed at (10,7)");
if (ex.get_tile(10, 7) !== TILE_LOOP) throw new Error("loop tile not set");
if (ex.get_money() !== m0 - LOOP_COST) throw new Error("loop should cost $25");

// Raise/lower work on loops.
if (!ex.click(10, 7, 3)) throw new Error("raise on loop should succeed");
if (ex.get_height(10, 7) !== 1) throw new Error("loop raise did not increment");
if (!ex.click(10, 7, 4)) throw new Error("lower on loop should succeed");
if (ex.get_height(10, 7) !== 0) throw new Error("loop lower did not decrement");

// Placement guards.
if (ex.click(10, 7, TOOL_LOOP)) throw new Error("loop should not place on existing loop");
if (ex.click(2, 7, TOOL_LOOP)) throw new Error("loop should not place on path");

// Bulldoze clears the loop and resets height.
if (!ex.click(10, 7, 2)) throw new Error("bulldoze on loop failed");
if (ex.get_tile(10, 7) !== 0) throw new Error("loop bulldoze did not clear");
if (ex.get_height(10, 7) !== 0) throw new Error("loop bulldoze did not reset height");

// Tick for 10 simulated seconds in 100ms steps.
let maxGuests = 0;
const startMoney = ex.get_money();
for (let i = 0; i < 100; i++) {
  ex.tick(100);
  const n = ex.get_guest_count();
  if (n > maxGuests) maxGuests = n;
}
const endMoney = ex.get_money();
console.log(`after 10s: money=${endMoney}, max guests seen=${maxGuests}`);

if (maxGuests === 0) throw new Error("no guests spawned");
if (endMoney <= startMoney) throw new Error(`money did not increase: ${startMoney} -> ${endMoney}`);

// Verify get_guest unpacking works.
let anyGuest = false;
for (let i = 0; i < ex.max_guests(); i++) {
  const g = ex.get_guest(i);
  if (g !== 0) {
    anyGuest = true;
    const state = g & 0xff;
    const gx = (g >> 8) & 0xff;
    const gy = (g >> 16) & 0xff;
    if (gx >= W || gy >= H) throw new Error(`guest ${i} out of bounds: ${gx},${gy}`);
    if (state < 1 || state > 3) throw new Error(`guest ${i} bad state: ${state}`);
  }
}
// It's fine if no guests are alive right at the end of the sim; the maxGuests check above is the real test.

// Test bulldoze on path.
const before = ex.get_tile(3, 7);
if (before !== 1) throw new Error("expected path at (3,7)");
ex.click(3, 7, 2);
if (ex.get_tile(3, 7) !== 0) throw new Error("bulldoze failed");

// Test bulldoze on track.
ex.click(9, 7, 2);
if (ex.get_tile(9, 7) !== 0) throw new Error("track bulldoze failed");

// Test re-init.
// Raise one more tile before reset to confirm init clears heights.
ex.click(6, 8, 3);
if (ex.get_height(6, 8) !== 1) throw new Error("setup: pre-init raise");
ex.init(1);
if (ex.get_money() !== 100) throw new Error("init did not reset money");
if (ex.get_tile(6, 7) !== 0) throw new Error("init did not reset tiles");
if (ex.get_height(6, 8) !== 0) throw new Error("init did not reset heights");

console.log("OK: all smoke tests passed");
