// Breakout: TypeScript + JSX, run by CErXes (compiled to WebAssembly), laid
// out by EVG with the stylesheet in the CSS tab, painted as SVG.
//
// The page calls, once per animation frame:
//   tick(dt, input)   dt in seconds; input.keys["ArrowLeft"] is a held key,
//                     input.pointer.x / .y the pointer over the view
//   view()            the JSX to draw; `width` and `height` are the view's size
// and, when they happen:
//   onKeyDown(key)    a key press ("ArrowLeft", " ", "p", …)
//   onClick={…}       on an element: called when it is clicked
//
// Class names come from the CSS tab; style={{ … }} is inline (numbers are px).

type Brick = { x: number; y: number; row: number; alive: boolean };
type Mode = "ready" | "playing" | "paused" | "won" | "over";

const COLS = 9;
const ROWS = 5;
const GAP = 6;
const TOP = 56;
const BRICK_H = 20;
const PADDLE_W = 92;
const PADDLE_H = 12;
const BALL = 12;

let bricks: Brick[] = [];
let ball = { x: 0, y: 0, vx: 0, vy: 0 };
let paddleX = 0;
let score = 0;
let lives = 3;
let level = 1;
let mode: Mode = "ready";
let lastPointerX = -1;

function brickW(): number {
  return (width - 32 - GAP * (COLS - 1)) / COLS;
}

function buildLevel() {
  bricks = [];
  for (let r = 0; r < ROWS; r++) {
    for (let c = 0; c < COLS; c++) {
      bricks.push({ x: 16 + c * (brickW() + GAP), y: TOP + r * (BRICK_H + GAP), row: r, alive: true });
    }
  }
}

function serve() {
  paddleX = (width - PADDLE_W) / 2;
  ball = { x: width / 2 - BALL / 2, y: height - 60, vx: 0, vy: 0 };
}

function launch() {
  const speed = 300 + level * 40;
  const angle = -Math.PI / 2 + (Math.random() - 0.5) * 0.9;
  ball.vx = Math.cos(angle) * speed;
  ball.vy = Math.sin(angle) * speed;
  mode = "playing";
}

function newGame() {
  score = 0;
  lives = 3;
  level = 1;
  buildLevel();
  serve();
  mode = "ready";
}

function onKeyDown(key: string) {
  if (key === " " || key === "Enter") {
    if (mode === "ready") launch();
    else if (mode === "won" || mode === "over") newGame();
  }
  if (key === "p" || key === "Escape") {
    if (mode === "playing") mode = "paused";
    else if (mode === "paused") mode = "playing";
  }
}

function tick(dt: number, input: any) {
  if (bricks.length === 0) newGame();
  dt = Math.min(dt, 1 / 30);

  // the paddle: arrow keys, or follow the pointer when it moves
  const speed = 520;
  if (input.keys["ArrowLeft"] || input.keys["a"]) paddleX -= speed * dt;
  if (input.keys["ArrowRight"] || input.keys["d"]) paddleX += speed * dt;
  if (input.pointer.inside && input.pointer.x !== lastPointerX) {
    paddleX = input.pointer.x - PADDLE_W / 2;
    lastPointerX = input.pointer.x;
  }
  paddleX = Math.max(8, Math.min(width - PADDLE_W - 8, paddleX));

  if (mode === "ready") {
    ball.x = paddleX + PADDLE_W / 2 - BALL / 2;
    ball.y = height - 40 - PADDLE_H - BALL - 2;
    return;
  }
  if (mode !== "playing") return;

  ball.x += ball.vx * dt;
  ball.y += ball.vy * dt;

  // walls
  if (ball.x < 0) { ball.x = 0; ball.vx = Math.abs(ball.vx); }
  if (ball.x + BALL > width) { ball.x = width - BALL; ball.vx = -Math.abs(ball.vx); }
  if (ball.y < 36) { ball.y = 36; ball.vy = Math.abs(ball.vy); }

  // paddle: the bounce angle follows where the ball hit it
  const py = height - 40 - PADDLE_H;
  if (ball.vy > 0 && ball.y + BALL >= py && ball.y + BALL <= py + PADDLE_H + 8 &&
      ball.x + BALL >= paddleX && ball.x <= paddleX + PADDLE_W) {
    const hit = (ball.x + BALL / 2 - (paddleX + PADDLE_W / 2)) / (PADDLE_W / 2);
    const s = Math.hypot(ball.vx, ball.vy) * 1.01;
    const a = -Math.PI / 2 + hit * 1.05;
    ball.vx = Math.cos(a) * s;
    ball.vy = Math.sin(a) * s;
    ball.y = py - BALL;
  }

  // bricks
  for (const b of bricks) {
    if (!b.alive) continue;
    if (ball.x + BALL > b.x && ball.x < b.x + brickW() && ball.y + BALL > b.y && ball.y < b.y + BRICK_H) {
      b.alive = false;
      score += (ROWS - b.row) * 10;
      const overlapX = Math.min(ball.x + BALL - b.x, b.x + brickW() - ball.x);
      const overlapY = Math.min(ball.y + BALL - b.y, b.y + BRICK_H - ball.y);
      if (overlapX < overlapY) ball.vx = -ball.vx; else ball.vy = -ball.vy;
      break;
    }
  }
  if (bricks.every((b) => !b.alive)) {
    level++;
    if (level > 3) { mode = "won"; return; }
    buildLevel();
    serve();
    mode = "ready";
  }

  // missed
  if (ball.y > height) {
    lives--;
    if (lives <= 0) mode = "over";
    else { serve(); mode = "ready"; }
  }
}

function Hud() {
  return (
    <div className="hud">
      <span className="hud-item">Score {score}</span>
      <span className="hud-item">Level {level}</span>
      <div className="spacer" />
      <span className="hud-item">{"♥".repeat(Math.max(lives, 0))}</span>
      <div className="button" onClick={() => (mode === "playing" ? (mode = "paused") : mode === "paused" ? (mode = "playing") : newGame())}>
        <span className="button-label">{mode === "playing" ? "Pause" : mode === "paused" ? "Resume" : "New game"}</span>
      </div>
    </div>
  );
}

function Banner(props: { title: string; hint: string }) {
  return (
    <div className="banner" style={{ left: width / 2 - 150, top: height / 2 - 40 }}>
      <span className="banner-title">{props.title}</span>
      <span className="banner-hint">{props.hint}</span>
    </div>
  );
}

function view() {
  const bw = brickW();
  return (
    <div className="game" onClick={() => onKeyDown(" ")}>
      <Hud />
      {bricks.filter((b) => b.alive).map((b) => (
        <div className={"brick row" + b.row} style={{ left: b.x, top: b.y, width: bw, height: BRICK_H }} />
      ))}
      <div className="paddle" style={{ left: paddleX, top: height - 40 - PADDLE_H, width: PADDLE_W, height: PADDLE_H }} />
      <div className="ball" style={{ left: ball.x, top: ball.y, width: BALL, height: BALL }} />
      {mode === "ready" && <Banner title={"Level " + level} hint="Space or click to launch" />}
      {mode === "paused" && <Banner title="Paused" hint="P to resume" />}
      {mode === "won" && <Banner title={"You won! " + score} hint="Space or click for a new game" />}
      {mode === "over" && <Banner title={"Game over: " + score} hint="Space or click for a new game" />}
    </div>
  );
}
