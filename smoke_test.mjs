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

// Build a straight path from (1,7) to (5,7), then a coaster at (6,7).
for (let x = 1; x <= 5; x++) {
  const r = ex.click(x, 7, 0);
  if (!r) throw new Error(`path place failed at (${x},7)`);
}
if (!ex.click(6, 7, 1)) throw new Error("coaster place failed");

console.log(`after build: money=${ex.get_money()}`);
// $100 - 5 paths - 1 coaster = 100 - 5 - 50 = 45
if (ex.get_money() !== 45) throw new Error(`expected money=45, got ${ex.get_money()}`);

if (ex.get_tile(6, 7) !== 2) throw new Error("coaster tile not set");
if (ex.get_tile(3, 7) !== 1) throw new Error("path tile not set");

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

// Test bulldoze.
const before = ex.get_tile(3, 7);
if (before !== 1) throw new Error("expected path at (3,7)");
ex.click(3, 7, 2);
if (ex.get_tile(3, 7) !== 0) throw new Error("bulldoze failed");

// Test re-init.
ex.init(1);
if (ex.get_money() !== 100) throw new Error("init did not reset money");
if (ex.get_tile(6, 7) !== 0) throw new Error("init did not reset tiles");

console.log("OK: all smoke tests passed");
