// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The few WASI calls a wasm32-wasip1 build of an engine makes in a page:
// the clock (Date.now, performance.now), random numbers (Math.random),
// stdout/stderr (collected, not printed) and exit. There are no files, no
// arguments and no environment. Any other call answers ENOSYS.

const ESUCCESS = 0;
const EBADF = 8;
const ENOSYS = 52;

export class WasiExit extends Error {
  constructor(code) {
    super("exit " + code);
    this.code = code;
  }
}

export function makeWasi(onOutput) {
  let memory = null;
  const view = () => new DataView(memory.buffer);
  const bytes = () => new Uint8Array(memory.buffer);
  const decoder = new TextDecoder();
  const pending = { 1: "", 2: "" };

  const flush = (fd, text) => {
    pending[fd] += text;
    let nl;
    while ((nl = pending[fd].indexOf("\n")) >= 0) {
      onOutput(fd, pending[fd].slice(0, nl));
      pending[fd] = pending[fd].slice(nl + 1);
    }
  };

  const imports = {
    fd_write(fd, iovs, iovsLen, nwritten) {
      if (fd !== 1 && fd !== 2) return EBADF;
      const v = view();
      let total = 0;
      let text = "";
      for (let i = 0; i < iovsLen; i++) {
        const ptr = v.getUint32(iovs + i * 8, true);
        const len = v.getUint32(iovs + i * 8 + 4, true);
        text += decoder.decode(bytes().subarray(ptr, ptr + len));
        total += len;
      }
      flush(fd, text);
      v.setUint32(nwritten, total, true);
      return ESUCCESS;
    },
    clock_time_get(id, _precision, out) {
      // 0 realtime, 1 monotonic; nanoseconds as u64
      const ms = id === 0 ? Date.now() : performance.now();
      view().setBigUint64(out, BigInt(Math.round(ms * 1e6)), true);
      return ESUCCESS;
    },
    clock_res_get(_id, out) {
      view().setBigUint64(out, 1000n, true);
      return ESUCCESS;
    },
    random_get(buf, len) {
      crypto.getRandomValues(bytes().subarray(buf, buf + len));
      return ESUCCESS;
    },
    environ_sizes_get(count, size) {
      view().setUint32(count, 0, true);
      view().setUint32(size, 0, true);
      return ESUCCESS;
    },
    environ_get() {
      return ESUCCESS;
    },
    args_sizes_get(count, size) {
      view().setUint32(count, 0, true);
      view().setUint32(size, 0, true);
      return ESUCCESS;
    },
    args_get() {
      return ESUCCESS;
    },
    fd_fdstat_get(fd, out) {
      if (fd > 2) return EBADF;
      const v = view();
      v.setUint8(out, 2); // character device
      v.setUint16(out + 2, 0, true);
      v.setBigUint64(out + 8, 0n, true);
      v.setBigUint64(out + 16, 0n, true);
      return ESUCCESS;
    },
    fd_prestat_get() {
      return EBADF;
    },
    fd_close() {
      return ESUCCESS;
    },
    fd_seek() {
      return ESUCCESS;
    },
    proc_exit(code) {
      throw new WasiExit(code);
    },
    sched_yield() {
      return ESUCCESS;
    },
  };

  const wasi = new Proxy(imports, {
    get: (t, k) => (k in t ? t[k] : () => ENOSYS),
  });

  return {
    imports: { wasi_snapshot_preview1: wasi },
    setMemory(m) {
      memory = m;
    },
    flushAll() {
      for (const fd of [1, 2]) {
        if (pending[fd]) {
          onOutput(fd, pending[fd]);
          pending[fd] = "";
        }
      }
    },
  };
}
