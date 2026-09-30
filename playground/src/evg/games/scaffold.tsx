// Scaffold Scramble: a climbing game in TypeScript + JSX, run by CErXes,
// laid out by EVG with the CSS tab's stylesheet, painted as SVG.
//
// A kitten is stuck at the top of the night-shift scaffold, and Grumble the
// crane-bot keeps rolling cable spools down the girders. Climb up to it.
//
//   ← →  walk        ↑ ↓  climb a ladder        Space  jump a spool
//   Pick up a wrench to smash spools for a few seconds (no climbing then).
//
// The page calls tick(dt, input) and view() every frame; onKeyDown(key) on a
// key press. Everything is boxes: sprites are small trees of divs, placed with
// inline style and dressed by the classes in the CSS tab.

type Girder = { x0: number; x1: number; yL: number; yR: number };
type Ladder = { x: number; lo: number; hi: number; broken: boolean };
type Spool = { g: number; x: number; y: number; dir: number; fall: boolean; vy: number;
  down: number; checked: number; jumped: boolean; spin: number };
type Mode = "title" | "play" | "dying" | "cleared" | "over";

const W = 640;
const H = 480;
const R = 9; // spool radius

// girders from the bottom (0) to Grumble's (5); the kitten's perch is 6
const G: Girder[] = [
  { x0: 0, x1: 640, yL: 462, yR: 450 },
  { x0: 0, x1: 596, yL: 376, yR: 390 },
  { x0: 44, x1: 640, yL: 306, yR: 292 },
  { x0: 0, x1: 596, yL: 220, yR: 234 },
  { x0: 44, x1: 640, yL: 150, yR: 136 },
  { x0: 0, x1: 430, yL: 86, yR: 86 },
  { x0: 250, x1: 340, yL: 46, yR: 46 },
];
// the direction a spool rolls on each girder (downhill)
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
let spools: Spool[] = [];
let throwClock = 0;
let throwAnim = 0;
let wrenches: { g: number; x: number; taken: boolean }[] = [];
let popups: { x: number; y: number; text: string; t: number }[] = [];

function resetLevel() {
  P.x = 40; P.g = 0; P.y = sy(0, P.x + 8); P.vx = 0; P.vy = 0; P.air = false; P.climb = -1; P.face = 1; P.wrench = 0;
  spools = [];
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
  moveSpools(dt);
  grumble(dt);

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

function moveSpools(dt: number) {
  const speed = 105 + level * 18;
  for (const s of spools) {
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
  spools = spools.filter((s) => s.y < 600 && s.x > -40);
}

function grumble(dt: number) {
  throwAnim = Math.max(0, throwAnim - dt);
  throwClock -= dt;
  if (throwClock <= 0) {
    throwClock = Math.max(1.1, 2.6 - level * 0.25) + Math.random() * 1.2;
    throwAnim = 0.4;
    spools.push({ g: 5, x: 120, y: sy(5, 120) - R, dir: 1, fall: false, vy: 0, down: 0, checked: -1, jumped: false, spin: 0 });
  }
}

// ---- view -------------------------------------------------------------------

function pad(n: number, w: number) {
  let s = String(n);
  while (s.length < w) s = "0" + s;
  return s;
}

// The girders and ladders never change: built once, and the runtime
// serializes an unchanged element only once.
let scene: any = null;

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
      out.push(<div className="rail" style={{ left: l.x - 9, top: a, height: b - a }} />);
      out.push(<div className="rail" style={{ left: l.x + 7, top: a, height: b - a }} />);
      for (let y = a + 4; y < b - 1; y += 10) out.push(<div className="rung" style={{ left: l.x - 8, top: y }} />);
    }
  }
  return out;
}

function Bot() {
  const flash = mode === "dying" && Math.floor(modeClock * 10) % 2 === 0;
  const step = Math.floor(P.walk) % 2;
  const cls = "bot" + (P.face < 0 ? " left" : "") + (P.climb >= 0 ? " climbing" : "");
  return (
    <div className={cls} style={{ left: P.x, top: P.y - 26 }}>
      <div className="bot-helmet" />
      <div className="bot-visor" style={{ left: P.face < 0 ? 2 : 7 }} />
      <div className={flash ? "bot-body hurt" : "bot-body"} />
      <div className="bot-leg" style={{ left: 3, top: step ? 19 : 20 }} />
      <div className="bot-leg" style={{ left: 9, top: step ? 20 : 19 }} />
      {P.wrench > 0 && <div className={"wrench-held" + (Math.floor(clock * 8) % 2 ? " up" : "")}
        style={{ left: P.face < 0 ? -9 : 16, top: Math.floor(clock * 8) % 2 ? 2 : 8 }} />}
    </div>
  );
}

// Grumble: a crane-bot on treads, with a hook arm that swings up to throw.
function Grumble() {
  const throwing = throwAnim > 0;
  const blink = Math.floor(clock * 3) % 9 === 0;
  return (
    <div className="grumble" style={{ left: 34, top: sy(5, 60) - 58 }}>
      <div className={throwing ? "grumble-lamp on" : "grumble-lamp"} />
      <div className="grumble-antenna" />
      <div className="grumble-head" />
      <div className={blink ? "grumble-eye shut" : "grumble-eye"} style={{ left: throwing ? 20 : 17 }} />
      <div className={blink ? "grumble-eye shut" : "grumble-eye"} style={{ left: throwing ? 35 : 32 }} />
      <div className="grumble-grille" style={{ left: 22 }} />
      <div className="grumble-grille" style={{ left: 27 }} />
      <div className="grumble-grille" style={{ left: 32 }} />
      <div className="grumble-body" />
      <div className="grumble-bolt" style={{ left: 12, top: 28 }} />
      <div className="grumble-bolt" style={{ left: 46, top: 28 }} />
      <div className="grumble-panel" />
      <div className="grumble-arm" style={{ left: 0, top: 24 }} />
      <div className="grumble-boom" style={{ left: 55, top: throwing ? 8 : 26 }} />
      <div className="grumble-hook" style={{ left: 66, top: throwing ? 4 : 30 }} />
      <div className="grumble-tread" />
      <div className="grumble-wheel" style={{ left: 8 }} />
      <div className="grumble-wheel" style={{ left: 24 }} />
      <div className="grumble-wheel" style={{ left: 40 }} />
    </div>
  );
}

function Spools() {
  return spools.map((s) => (
    <div className="spool" style={{ left: s.x - R, top: s.y - R }}>
      <div className={Math.floor(s.spin * 2) % 2 ? "spool-hub turned" : "spool-hub"} />
    </div>
  ));
}

function Kitten() {
  const blink = Math.floor(clock * 2) % 6 === 0;
  return (
    <div className="kitten" style={{ left: 282, top: 22 }}>
      <div className="kitten-ear" style={{ left: 1 }} />
      <div className="kitten-ear" style={{ left: 9 }} />
      <div className="kitten-head" />
      <div className={blink ? "kitten-eyes closed" : "kitten-eyes"} />
      <div className="kitten-body" />
      <div className="kitten-tail" />
      {Math.floor(clock) % 4 === 0 && <span className="help" style={{ left: 22, top: -6 }}>mew!</span>}
    </div>
  );
}

function Hud() {
  return (
    <div className="hud">
      <div className="hud-col">
        <span className="hud-label">1UP</span>
        <span className="hud-value">{pad(score, 6)}</span>
      </div>
      <div className="hud-col">
        <span className="hud-label">BEST</span>
        <span className="hud-value">{pad(Math.max(best, score), 6)}</span>
      </div>
      <div className="spacer" />
      <div className="lives">
        {[0, 1, 2, 3, 4].filter((i) => i < lives - 1).map(() => <div className="life" />)}
      </div>
      <div className="bonus-box">
        <span className="hud-label">BONUS</span>
        <span className={bonus <= 1000 ? "bonus-value low" : "bonus-value"}>{pad(bonus, 4)}</span>
      </div>
      <span className="hud-level">L{pad(level, 2)}</span>
    </div>
  );
}

function Banner(props: { title: string; lines: string[] }) {
  return (
    <div className="banner" style={{ left: W / 2 - 170, top: 190 }}>
      <span className="banner-title">{props.title}</span>
      {props.lines.map((l) => <span className="banner-line">{l}</span>)}
    </div>
  );
}

function Scene() {
  return (
    <div className="scene">
      <div className="moon" />
      <Girders />
      <Ladders />
      <div className="spool-pile" style={{ left: 2, top: sy(5, 10) - 40 }}>
        <div className="pile-spool" style={{ left: 0, top: 20 }} />
        <div className="pile-spool" style={{ left: 16, top: 20 }} />
        <div className="pile-spool" style={{ left: 8, top: 2 }} />
      </div>
    </div>
  );
}

function view() {
  if (P.y === 0) resetLevel();
  if (!scene) scene = <Scene />;
  return (
    <div className="stage" onClick={() => onKeyDown(" ")}>
      {scene}
      <Grumble />
      <Kitten />
      {wrenches.filter((w) => !w.taken).map((w) => (
        <div className="wrench" style={{ left: w.x - 5, top: sy(w.g, w.x) - 28 + Math.sin(clock * 4) * 2 }} />
      ))}
      <Spools />
      {mode !== "title" && <Bot />}
      {popups.map((p) => <span className="popup" style={{ left: p.x - 12, top: p.y }}>{p.text}</span>)}
      <Hud />
      {mode === "title" && <Banner title="SCAFFOLD SCRAMBLE" lines={["Climb to the kitten. Jump the spools.", "← → walk   ↑ ↓ climb   Space jump", "Space or click to start"]} />}
      {mode === "cleared" && <Banner title="KITTEN SAVED!" lines={["Bonus " + bonus, "Next: level " + (level + 1)]} />}
      {mode === "over" && <Banner title="GAME OVER" lines={["Score " + score, "Space or click to play again"]} />}
    </div>
  );
}
