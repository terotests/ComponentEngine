import nacl from "tweetnacl";
const hex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
const seed = new Uint8Array(32).map((_x, i) => i * 7 + 1);
const kp = nacl.sign.keyPair.fromSeed(seed);
const msg = new Uint8Array(1000).map((_x, i) => i & 255);
const key = new Uint8Array(32).map((_x, i) => 255 - i), nonce = new Uint8Array(24).map((_x, i) => i);
const sig = nacl.sign.detached(msg, kp.secretKey);
export default {
  check: {
    keys: () => hex(kp.publicKey),
    sign: () => [hex(sig), nacl.sign.detached.verify(msg, sig, kp.publicKey), nacl.sign.detached.verify(msg.map((x, i) => (i ? x : 9)), sig, kp.publicKey)],
    box: () => { const c = nacl.secretbox(msg, nonce, key); return [hex(c.subarray(0, 32)), nacl.secretbox.open(c, nonce, key).length]; },
    hash: () => hex(nacl.hash(msg)),
    scalarMult: () => hex(nacl.scalarMult(key, nacl.scalarMult.base(seed))),
  },
  bench: { sign: () => nacl.sign.detached(msg, kp.secretKey).length, verify: () => nacl.sign.detached.verify(msg, sig, kp.publicKey), secretbox: () => nacl.secretbox(msg, nonce, key).length },
};
