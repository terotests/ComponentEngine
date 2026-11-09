// SPDX-License-Identifier: AGPL-3.0-or-later
// From terotests/componentengine, playground/src/evg/games/scaffold.tsx.
//
// Scaffold Scramble: a climbing game in TypeScript + JSX, run by CErXes,
// laid out by EVG with the CSS tab's stylesheet, painted as SVG.
//
// Rumbo, a giant purple ape in a hard hat, has climbed the night-shift
// scaffold and a kitten is stuck on the beam above him. He keeps rolling
// barrels down the girders. Help Pip the builder climb up to the kitten.
//
//   ← →  walk        ↑ ↓  climb a ladder        Space  jump a barrel
//   On a phone: the buttons under the game, or tap the game to jump.
//   Pick up a wrench to smash barrels for a few seconds (no climbing then).
//
// The page calls tick(dt, input) and view() every frame; onKeyDown(key) on a
// key press. Everything is boxes: sprites are small trees of divs, placed with
// inline style and dressed by the classes in the CSS tab (gradients, rounded
// corners, rotation, opacity).

type Girder = { x0: number; x1: number; yL: number; yR: number };
type Ladder = { x: number; lo: number; hi: number; broken: boolean };
type Barrel = { g: number; x: number; y: number; dir: number; fall: boolean; vy: number;
  down: number; checked: number; jumped: boolean; spin: number };
type Mode = "title" | "play" | "dying" | "cleared" | "over";

const W = 640;
const H = 480;
const R = 9; // barrel radius (for hits; drawn a little larger)
const THROW_X = 142; // where Rumbo puts a barrel down

// girders from the bottom (0) to Rumbo's (5); the kitten's perch is 6
const G: Girder[] = [
  { x0: 0, x1: 640, yL: 462, yR: 450 },
  { x0: 0, x1: 596, yL: 376, yR: 390 },
  { x0: 44, x1: 640, yL: 306, yR: 292 },
  { x0: 0, x1: 596, yL: 220, yR: 234 },
  { x0: 44, x1: 640, yL: 150, yR: 136 },
  { x0: 0, x1: 430, yL: 86, yR: 86 },
  { x0: 250, x1: 340, yL: 46, yR: 46 },
];
// the direction a barrel rolls on each girder (downhill)
const ROLL = [-1, 1, -1, 1, -1, 1, 0];

const L: Ladder[] = [
  { x: 520, lo: 0, hi: 1, broken: false },
  { x: 250, lo: 0, hi: 1, broken: true },
  { x: 110, lo: 1, hi: 2, broken: false },
  { x: 330, lo: 1, hi: 2, broken: true },
  { x: 470, lo: 2, hi: 3, broken: false },
  { x: 220, lo: 2, hi: 3, broken: false },
  { x: 150, lo: 3, hi: 4, broken: false },
  { x: 380, lo: 3, hi: 4, broken: true },
  { x: 390, lo: 4, hi: 5, broken: false },
  { x: 310, lo: 5, hi: 6, broken: false },
];

function sy(g: number, x: number): number {
  const q = G[g];
  const t = Math.max(0, Math.min(1, (x - q.x0) / (q.x1 - q.x0)));
  return q.yL + (q.yR - q.yL) * t;
}

// ---- state ------------------------------------------------------------------

let mode: Mode = "title";
let score = 0;
let best = 0;
let lives = 3;
let level = 1;
let bonus = 5000;
let bonusClock = 0;
let clock = 0;
let modeClock = 0;

const P = { x: 40, y: 0, g: 0, vx: 0, vy: 0, air: false, climb: -1, face: 1, walk: 0, wrench: 0 };
let barrels: Barrel[] = [];
let throwClock = 0;
let throwAnim = 0;
let wrenches: { g: number; x: number; taken: boolean }[] = [];
let popups: { x: number; y: number; text: string; t: number }[] = [];

function resetLevel() {
  P.x = 40; P.g = 0; P.y = sy(0, P.x + 8); P.vx = 0; P.vy = 0; P.air = false; P.climb = -1; P.face = 1; P.wrench = 0;
  barrels = [];
  throwClock = 1.2;
  bonus = 5000;
  bonusClock = 0;
  wrenches = [{ g: 1, x: 60, taken: false }, { g: 3, x: 540, taken: false }];
  popups = [];
}

function newGame() {
  score = 0; lives = 3; level = 1;
  resetLevel();
  mode = "play";
}

function pop(x: number, y: number, text: string) {
  popups.push({ x, y, text, t: 0.8 });
}

function onKeyDown(key: string) {
  if (mode === "title" || mode === "over") {
    if (key === " " || key === "Enter") newGame();
    return;
  }
  if (mode === "play" && key === " " && !P.air && P.climb < 0) {
    P.air = true;
    P.vy = -230;
    const k = keysNow;
    P.vx = (k["ArrowLeft"] || k["a"] ? -1 : 0) + (k["ArrowRight"] || k["d"] ? 1 : 0);
    P.vx *= 110;
  }
}

let keysNow: any = {};

// ---- the frame --------------------------------------------------------------

function tick(dt: number, input: any) {
  keysNow = input.keys;
  dt = Math.min(dt, 1 / 30);
  clock += dt;
  modeClock += dt;
  for (const p of popups) { p.t -= dt; p.y -= 30 * dt; }
  popups = popups.filter((p) => p.t > 0);
  if (mode === "dying") {
    if (modeClock > 1.4) {
      if (lives <= 0) { mode = "over"; best = Math.max(best, score); }
      else { resetLevel(); mode = "play"; }
      modeClock = 0;
    }
    return;
  }
  if (mode === "cleared") {
    if (modeClock > 2) { level++; resetLevel(); mode = "play"; modeClock = 0; }
    return;
  }
  if (mode !== "play") return;

  const k = input.keys;
  const left = k["ArrowLeft"] || k["a"], right = k["ArrowRight"] || k["d"];
  const up = k["ArrowUp"] || k["w"], down = k["ArrowDown"] || k["s"];

  // bonus runs down
  bonusClock += dt;
  if (bonusClock >= 2) { bonusClock -= 2; bonus = Math.max(0, bonus - 100); if (bonus === 0) die(); }
  if (P.wrench > 0) P.wrench = Math.max(0, P.wrench - dt);

  movePlayer(dt, left, right, up, down);
  moveBarrels(dt);
  rumbo(dt);

  // wrenches
  for (const w of wrenches) {
    if (w.taken) continue;
    const wy = sy(w.g, w.x) - 26;
    if (P.climb < 0 && Math.abs(P.x + 8 - w.x) < 14 && P.y > wy && P.y - 30 < wy + 14) {
      w.taken = true;
      P.wrench = 7;
      pop(w.x, wy, "WRENCH!");
    }
  }

  // the kitten
  if (P.g === 6 && P.climb < 0) {
    score += bonus;
    pop(P.x, P.y - 40, "+" + bonus);
    mode = "cleared";
    modeClock = 0;
  }
}

function die() {
  if (mode !== "play") return;
  pop(P.x - 6, P.y - 44, "OUCH!");
  lives--;
  mode = "dying";
  modeClock = 0;
}

function ladderAt(g: number, goingUp: boolean): Ladder | null {
  for (const l of L) {
    if (l.broken) continue;
    if ((goingUp ? l.lo : l.hi) === g && Math.abs(P.x + 8 - l.x) < 7) return l;
  }
  return null;
}

function movePlayer(dt: number, left: boolean, right: boolean, up: boolean, down: boolean) {
  if (P.climb >= 0) {
    const l = L[P.climb];
    const top = sy(l.hi, l.x), bottom = sy(l.lo, l.x);
    if (up) { P.y -= 70 * dt; P.walk += dt * 8; }
    if (down) { P.y += 70 * dt; P.walk += dt * 8; }
    if (P.y <= top) { P.y = top; P.g = l.hi; P.climb = -1; }
    else if (P.y >= bottom) { P.y = bottom; P.g = l.lo; P.climb = -1; }
    return;
  }
  if (!P.air) {
    // onto a ladder (not while carrying the wrench)
    if (P.wrench <= 0 && (up || down)) {
      const l = ladderAt(P.g, up);
      if (l) { P.climb = L.indexOf(l); P.x = l.x - 8; return; }
    }
    P.vx = 0;
    if (left) { P.vx = -95; P.face = -1; }
    if (right) { P.vx = 95; P.face = 1; }
    if (P.vx !== 0) P.walk += dt * 10;
    P.x += P.vx * dt;
    P.x = Math.max(G[P.g].x0, Math.min(G[P.g].x1 - 16, P.x));
    P.y = sy(P.g, P.x + 8);
  } else {
    P.x += P.vx * dt;
    P.x = Math.max(G[P.g].x0, Math.min(G[P.g].x1 - 16, P.x));
    P.vy += 700 * dt;
    P.y += P.vy * dt;
    const floor = sy(P.g, P.x + 8);
    if (P.vy > 0 && P.y >= floor) { P.y = floor; P.air = false; P.vy = 0; }
  }
}

function moveBarrels(dt: number) {
  const speed = 105 + level * 18;
  for (const s of barrels) {
    s.spin += dt * speed * s.dir / R;
    if (s.down > 0) {
      // going down a ladder
      s.y += 90 * dt;
      const floor = sy(s.down - 1, s.x) - R;
      if (s.y >= floor) { s.g = s.down - 1; s.down = 0; s.dir = ROLL[s.g]; s.y = floor; }
    } else if (s.fall) {
      s.vy += 700 * dt;
      s.y += s.vy * dt;
      if (s.g >= 0) {
        const floor = sy(s.g, s.x) - R;
        if (s.y >= floor) { s.y = floor; s.fall = false; s.vy = 0; s.dir = ROLL[s.g]; }
      }
    } else {
      s.x += s.dir * speed * dt;
      const q = G[s.g];
      if (s.x < q.x0 - 2 || s.x > q.x1 + 2) {
        s.fall = true;
        s.vy = 0;
        s.g = s.g - 1; // the girder below, or -1 off the bottom
      } else {
        s.y = sy(s.g, s.x) - R;
        // a ladder down: decided once per ladder
        for (const l of L) {
          if (l.hi === s.g && !l.broken && Math.abs(s.x - l.x) < 3 && s.checked !== L.indexOf(l)) {
            s.checked = L.indexOf(l);
            if (Math.random() < 0.3 + level * 0.05) { s.down = s.g; s.x = l.x; }
          }
        }
      }
    }
    // the player
    const px = P.x + 8, py = P.y - 12;
    const dx = s.x - px, dy = s.y - py;
    if (mode === "play" && Math.abs(dx) < R + 6 && Math.abs(dy) < R + 11) {
      if (P.wrench > 0 && P.climb < 0) {
        s.y = 9999;
        score += 300;
        pop(s.x, sy(Math.max(s.g, 0), s.x) - 20, "300");
      } else {
        die();
      }
    }
    // jumped over
    if (P.air && !s.jumped && Math.abs(dx) < 10 && s.y > P.y && s.y - P.y < 50 && s.g === P.g) {
      s.jumped = true;
      score += 100;
      pop(px, P.y - 30, "100");
    }
  }
  barrels = barrels.filter((s) => s.y < 600 && s.x > -40);
}

function rumbo(dt: number) {
  throwAnim = Math.max(0, throwAnim - dt);
  throwClock -= dt;
  if (throwClock <= 0) {
    throwClock = Math.max(1.1, 2.6 - level * 0.25) + Math.random() * 1.2;
    throwAnim = 0.4;
    barrels.push({ g: 5, x: THROW_X, y: sy(5, THROW_X) - R, dir: 1, fall: false, vy: 0, down: 0, checked: -1, jumped: false, spin: 0 });
  }
}

// ---- view -------------------------------------------------------------------

function pad(n: number, w: number) {
  let s = String(n);
  while (s.length < w) s = "0" + s;
  return s;
}

// a little pseudo-random sequence, so the stars and windows are the same
// every time the scene is built
let seed = 7;
function rnd() {
  seed = (seed * 16807) % 2147483647;
  return (seed % 10000) / 10000;
}

// The sky, the city, the girders and the ladders never change: built once,
// and the runtime serializes an unchanged element only once.
let scene: any = null;

function Sky() {
  const out: any[] = [<div className="sky" />];
  for (let i = 0; i < 26; i++) {
    const big = rnd() < 0.25;
    out.push(<div className={big ? "star big" : "star"} style={{ left: Math.floor(rnd() * 630), top: Math.floor(30 + rnd() * 260) }} />);
  }
  out.push(<div className="moon-glow" style={{ left: 452, top: 26 }} />);
  out.push(<div className="moon" style={{ left: 466, top: 40 }} />);
  out.push(<div className="crater" style={{ left: 478, top: 50, width: 8, height: 8 }} />);
  out.push(<div className="crater" style={{ left: 492, top: 64, width: 6, height: 6 }} />);
  return out;
}

// the city behind the scaffold: towers with a few lit windows
function City() {
  const out: any[] = [];
  const towers = [[0, 360, 70], [64, 330, 54], [112, 390, 80], [186, 300, 46], [226, 350, 74], [296, 320, 60],
    [352, 372, 84], [430, 286, 52], [478, 340, 70], [544, 310, 50], [590, 360, 50]];
  for (const [x, top, w] of towers) {
    out.push(<div className="tower" style={{ left: x, top, width: w, height: 480 - top }} />);
    for (let y = top + 10; y < 470; y += 16) {
      for (let wx = x + 6; wx < x + w - 8; wx += 12) {
        if (rnd() < 0.18) out.push(<div className={rnd() < 0.3 ? "win warm" : "win"} style={{ left: wx, top: y }} />);
      }
    }
  }
  return out;
}

function Girders() {
  const out: any[] = [];
  G.forEach((q, gi) => {
    for (let x = q.x0; x < q.x1; x += 48) {
      const w = Math.min(48, q.x1 - x);
      out.push(<div className={gi === 6 ? "perch" : "girder"} style={{ left: x, top: sy(gi, x + w / 2), width: w, height: 10 }} />);
    }
  });
  return out;
}

function Ladders() {
  const out: any[] = [];
  for (const l of L) {
    const top = sy(l.hi, l.x) + 10, bottom = sy(l.lo, l.x);
    const segs = l.broken ? [[top, top + 14], [bottom - 14, bottom]] : [[top, bottom]];
    for (const [a, b] of segs) {
      out.push(<div className="rail" style={{ left: l.x - 10, top: a, height: b - a }} />);
      out.push(<div className="rail" style={{ left: l.x + 7, top: a, height: b - a }} />);
      for (let y = a + 4; y < b - 1; y += 9) out.push(<div className="rung" style={{ left: l.x - 8, top: y }} />);
    }
  }
  return out;
}

// three barrels standing on end, Rumbo's supply
function Kegs() {
  const top = sy(5, 10) - 22;
  return [[2, top], [20, top], [11, top - 21]].map(([x, y]) => (
    <div className="keg" style={{ left: x, top: y }}>
      <div className="keg-band" style={{ top: 4 }} />
      <div className="keg-band" style={{ top: 14 }} />
    </div>
  ));
}

// Pip the builder: a hard hat, red overalls, boots. From behind on a ladder.
function Hero() {
  const flash = mode === "dying" && Math.floor(modeClock * 10) % 2 === 0;
  const step = Math.floor(P.walk) % 2;
  const climbing = P.climb >= 0;
  const right = P.face > 0;
  const swing = P.vx !== 0 || climbing ? step : 0;
  const hold = P.wrench > 0;
  const up = Math.floor(clock * 8) % 2;
  return (
    <div className="hero" style={{ left: P.x - 2, top: P.y - 30 }}>
      {hold && <div className="power" style={{ left: -8, top: -6, opacity: 0.25 + 0.2 * up }} />}
      <div className="hero-boot" style={{ left: 3, top: swing ? 25 : 26 }} />
      <div className="hero-boot" style={{ left: 11, top: swing ? 26 : 25 }} />
      <div className="hero-arm" style={{ left: 0, top: climbing ? (step ? 8 : 12) : (swing ? 15 : 14) }} />
      <div className="hero-arm" style={{ left: 16, top: climbing ? (step ? 12 : 8) : (swing ? 14 : 15) }} />
      <div className={flash ? "hero-body hurt" : "hero-body"} />
      <div className="hero-strap" style={{ left: 7 }} />
      <div className="hero-strap" style={{ left: 11 }} />
      <div className={climbing ? "hero-face back" : "hero-face"} />
      {!climbing && <div className="hero-eye" style={{ left: right ? 11 : 6 }} />}
      {!climbing && <div className="hero-cheek" style={{ left: right ? 13 : 5 }} />}
      <div className="hero-hat" />
      <div className="hero-brim" style={{ left: climbing ? 3 : right ? 8 : 2 }} />
      {hold && <div className="wrench-held" style={{ left: right ? 17 : -9, top: up ? 4 : 10, transform: up ? "rotate(-40deg)" : "rotate(20deg)" }} />}
    </div>
  );
}

// Rumbo: a giant purple ape in an orange hard hat. He pounds his chest now
// and then, and roars with an arm up when he puts a barrel down.
function Rumbo() {
  const throwing = throwAnim > 0;
  const pounding = !throwing && clock % 7 > 5.6;
  const beat = Math.floor(clock * 7) % 2;
  const blink = !throwing && Math.floor(clock * 3) % 11 === 0;
  const look = throwing ? 2 : Math.floor(clock / 2) % 3 - 1;
  const roar = throwing || pounding;
  const cheer = mode === "dying";
  const lift = roar || cheer ? 1 : 0;
  return (
    <div className="ape" style={{ left: 30, top: sy(5, 60) - 86 + (pounding && beat ? 1 : 0) }}>
      <div className="ape-leg" style={{ left: 26 }} />
      <div className="ape-leg" style={{ left: 58 }} />
      <div className="ape-foot" style={{ left: 20 }} />
      <div className="ape-foot" style={{ left: 60 }} />
      <div className="ape-body" />
      <div className="ape-belly" />
      {/* arms: hanging, pounding the chest, or one up with a barrel */}
      {pounding ? [
        <div className="ape-arm short" style={{ left: 18, top: beat ? 30 : 36 }} />,
        <div className="ape-arm short" style={{ left: 68, top: beat ? 36 : 30 }} />,
        <div className="ape-fist" style={{ left: 30, top: beat ? 40 : 48 }} />,
        <div className="ape-fist" style={{ left: 56, top: beat ? 48 : 40 }} />,
      ] : [
        <div className="ape-arm" style={{ left: 4, top: 30 - lift * 4 }} />,
        <div className="ape-fist" style={{ left: 0, top: 64 - lift * 4 }} />,
        <div className="ape-arm" style={{ left: 82, top: throwing ? 4 : 30 }} />,
        throwing ? <div className="barrel held" style={{ left: 78, top: -6 }}>
          <div className="barrel-band" style={{ transform: "rotate(90deg)" }} />
        </div> : <div className="ape-fist" style={{ left: 82, top: 64 }} />,
      ]}
      <div className="ape-head" />
      <div className="ape-face" />
      <div className="ape-brow" />
      <div className={blink ? "ape-eye shut" : "ape-eye"} style={{ left: 40 }}>
        {!blink && <div className="ape-pupil" style={{ left: 3 + look }} />}
      </div>
      <div className={blink ? "ape-eye shut" : "ape-eye"} style={{ left: 55 }}>
        {!blink && <div className="ape-pupil" style={{ left: 3 + look }} />}
      </div>
      {roar && <div className="ape-angry" style={{ left: 38, transform: "rotate(18deg)" }} />}
      {roar && <div className="ape-angry" style={{ left: 55, transform: "rotate(-18deg)" }} />}
      <div className="ape-nostril" style={{ left: 49 }} />
      <div className="ape-nostril" style={{ left: 54 }} />
      {roar || cheer ? (
        <div className="ape-mouth open">
          <div className="ape-tooth" style={{ left: 2 }} />
          <div className="ape-tooth" style={{ left: 11 }} />
        </div>
      ) : <div className="ape-mouth" />}
      <div className="ape-hat" />
      <div className="ape-hat-ridge" />
      <div className="ape-hat-brim" />
      {pounding && <span className="roar-text" style={{ left: 96, top: 2 }}>GRRR!</span>}
      {cheer && <span className="roar-text" style={{ left: 96, top: 2 }}>HA HA!</span>}
    </div>
  );
}

function Barrels() {
  return barrels.map((s) => (
    <div className="barrel" style={{ left: s.x - 10, top: s.y - 10 }}>
      <div className="barrel-band" style={{ transform: "rotate(" + Math.round((s.spin * 57.3) % 360) + "deg)" }} />
      <div className="barrel-hub" />
    </div>
  ));
}

function Kitten() {
  const blink = Math.floor(clock * 2) % 6 === 0;
  const wag = Math.floor(clock * 3) % 2;
  return (
    <div className="kitten" style={{ left: 280, top: 20 }}>
      <div className="kitten-tail" style={{ left: wag ? 19 : 20, top: wag ? 9 : 10, transform: wag ? "rotate(20deg)" : "rotate(-10deg)" }} />
      <div className="kitten-body" />
      <div className="kitten-ear" style={{ left: 2 }} />
      <div className="kitten-ear" style={{ left: 11 }} />
      <div className="kitten-head" />
      <div className="kitten-stripe" />
      <div className={blink ? "kitten-eye closed" : "kitten-eye"} style={{ left: 5 }} />
      <div className={blink ? "kitten-eye closed" : "kitten-eye"} style={{ left: 12 }} />
      <div className="kitten-nose" />
      {Math.floor(clock) % 4 < 2 && mode !== "cleared" && (
        <div className="bubble" style={{ left: -44, top: -14 }}><span className="bubble-text">help!</span></div>
      )}
      {mode === "cleared" && <div className="bubble" style={{ left: -44, top: -14 }}><span className="bubble-text">purr!</span></div>}
    </div>
  );
}

function Wrench(props: { x: number; y: number }) {
  const glow = 0.18 + 0.12 * Math.sin(clock * 6);
  return (
    <div className="wrench" style={{ left: props.x - 12, top: props.y }}>
      <div className="wrench-glow" style={{ opacity: glow }} />
      <div className="wrench-handle" style={{ transform: "rotate(35deg)" }} />
      <div className="wrench-head" />
    </div>
  );
}

function Hud() {
  return (
    <div className="hud">
      <div className="hud-col">
        <span className="hud-label">SCORE</span>
        <span className="hud-value">{pad(score, 6)}</span>
      </div>
      <div className="hud-col">
        <span className="hud-label">BEST</span>
        <span className="hud-value">{pad(Math.max(best, score), 6)}</span>
      </div>
      <div className="bonus-box">
        <span className="hud-label">BONUS</span>
        <span className={bonus <= 1000 ? "bonus-value low" : "bonus-value"}>{pad(bonus, 4)}</span>
      </div>
      <div className="lives">
        {[0, 1, 2, 3, 4].filter((i) => i < lives - 1).map(() => <div className="life" />)}
      </div>
      <span className="hud-level">L{level}</span>
    </div>
  );
}

function Banner(props: { title: string; lines: string[]; tall?: boolean }) {
  return (
    <div className="banner" style={{ left: W / 2 - 200, top: props.tall ? 150 : 180 }}>
      <span className="banner-title">{props.title}</span>
      {props.lines.map((l, i) => <span className={i === props.lines.length - 1 ? "banner-line go" : "banner-line"}>{l}</span>)}
    </div>
  );
}

function Scene() {
  seed = 7;
  return (
    <div className="scene">
      <Sky />
      <City />
      <Girders />
      <Ladders />
      <Kegs />
    </div>
  );
}

function view() {
  if (P.y === 0) resetLevel();
  if (!scene) scene = <Scene />;
  return (
    <div className="stage" onClick={() => onKeyDown(" ")}>
      {scene}
      <Rumbo />
      <Kitten />
      {wrenches.filter((w) => !w.taken).map((w) => <Wrench x={w.x} y={sy(w.g, w.x) - 30 + Math.sin(clock * 4) * 2} />)}
      <Barrels />
      {mode !== "title" && <Hero />}
      {popups.map((p) => <span className="popup" style={{ left: p.x - 12, top: p.y }}>{p.text}</span>)}
      <Hud />
      {mode === "title" && <Banner tall title="SCAFFOLD SCRAMBLE" lines={[
        "Rumbo the giant ape is rolling barrels!",
        "Climb up to the kitten and jump the barrels.",
        "← → walk   ↑ ↓ climb   Space jump",
        "Tap, Space or JUMP to start"]} />}
      {mode === "cleared" && <Banner title="KITTEN SAVED!" lines={["Bonus " + bonus, "Next: level " + (level + 1)]} />}
      {mode === "over" && <Banner title="GAME OVER" lines={["Score " + score, "Tap, Space or JUMP to play again"]} />}
    </div>
  );
}
