import Matter from "matter-js";
const { Engine, Bodies, Composite, Body } = Matter;
function world(n) {
  const engine = Engine.create();
  const bodies = [Bodies.rectangle(400, 610, 810, 60, { isStatic: true })];
  for (let i = 0; i < n; i++) bodies.push(i % 2 ? Bodies.circle(100 + (i % 20) * 30, 50 - Math.floor(i / 20) * 40, 12) : Bodies.rectangle(100 + (i % 20) * 30, 50 - Math.floor(i / 20) * 40, 20, 20, { angle: i * 0.1 }));
  Composite.add(engine.world, bodies);
  return { engine, bodies };
}
function sim(n, steps) { const w = world(n); for (let i = 0; i < steps; i++) Engine.update(w.engine, 1000 / 60); return w; }
export default {
  check: {
    positions: () => sim(40, 120).bodies.slice(1, 9).map((b) => [b.position.x.toFixed(2), b.position.y.toFixed(2), b.angle.toFixed(3)]),
    sleeping: () => { const w = world(10); w.engine.enableSleeping = true; for (let i = 0; i < 400; i++) Engine.update(w.engine, 1000 / 60); return w.bodies.filter((b) => b.isSleeping).length; },
    force: () => { const w = world(1); Body.applyForce(w.bodies[1], w.bodies[1].position, { x: 0.01, y: -0.02 }); Engine.update(w.engine, 16); return [w.bodies[1].velocity.x.toFixed(4), w.bodies[1].velocity.y.toFixed(4)]; },
  },
  bench: { sim100: () => sim(100, 60).bodies.length },
};
