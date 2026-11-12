import elliptic from "elliptic";
const ec = new elliptic.ec("secp256k1");
const ed = new elliptic.eddsa("ed25519");
const key = ec.keyFromPrivate("1e99423a4ed27608a15a2616a2b0e9e52ced330ac530edcc32c8ffc6a526aedd", "hex");
const msg = "4b688df40bcedbe641ddb16ff0a1842d9c67ea1c3bf63f3e0471baa664531d1a";
const sig = key.sign(msg);
const edKey = ed.keyFromSecret("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60");
export default {
  check: {
    pub: () => key.getPublic("hex"),
    sign: () => [sig.r.toString(16), sig.s.toString(16), sig.recoveryParam, key.verify(msg, sig), key.verify(msg.replace("4", "5"), sig)],
    der: () => sig.toDER("hex"),
    ecdh: () => key.derive(ec.keyFromPrivate("02", "hex").getPublic()).toString(16),
    ed25519: () => [edKey.getPublic("hex"), edKey.sign("616263").toHex()],
  },
  bench: { sign: () => key.sign(msg).r.bitLength(), verify: () => key.verify(msg, sig) },
};
