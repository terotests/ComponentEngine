import { mat4, vec3, quat } from "gl-matrix";
const r = (a) => Array.from(a, (x) => Math.round(x * 1e6) / 1e6);
function scene(n) {
  const view = mat4.lookAt(mat4.create(), [0, 5, 10], [0, 0, 0], [0, 1, 0]);
  const proj = mat4.perspective(mat4.create(), Math.PI / 4, 16 / 9, 0.1, 100);
  const vp = mat4.multiply(mat4.create(), proj, view);
  const m = mat4.create(), mvp = mat4.create(), q = quat.create(), v = vec3.create(), acc = vec3.create();
  for (let i = 0; i < n; i++) {
    quat.fromEuler(q, i, i * 2, i * 3);
    mat4.fromRotationTranslationScale(m, q, [i % 10, i % 7, -(i % 5)], [1, 1 + (i % 3), 1]);
    mat4.multiply(mvp, vp, m);
    vec3.transformMat4(v, [1, 1, 1], mvp);
    vec3.add(acc, acc, v);
    mat4.invert(m, m);
  }
  return acc;
}
export default {
  check: {
    scene: () => r(scene(100)),
    invert: () => r(mat4.invert(mat4.create(), mat4.fromValues(2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 4, 0, 1, 2, 3, 1))),
    slerp: () => r(quat.slerp(quat.create(), quat.fromEuler(quat.create(), 0, 0, 0), quat.fromEuler(quat.create(), 90, 45, 10), 0.3)),
  },
  bench: { scene: () => scene(2000)[0] },
};
