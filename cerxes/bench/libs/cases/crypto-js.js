import CryptoJS from "crypto-js";
const msg = "The quick brown fox jumps over the lazy dog ".repeat(50);
export default {
  check: {
    hashes: () => [CryptoJS.MD5(msg).toString(), CryptoJS.SHA1(msg).toString(), CryptoJS.SHA256(msg).toString(), CryptoJS.SHA512(msg).toString(), CryptoJS.SHA3(msg, { outputLength: 256 }).toString(), CryptoJS.RIPEMD160(msg).toString()],
    hmac: () => [CryptoJS.HmacSHA256(msg, "key").toString(CryptoJS.enc.Base64), CryptoJS.PBKDF2("pw", CryptoJS.enc.Hex.parse("a1b2c3"), { keySize: 8, iterations: 50, hasher: CryptoJS.algo.SHA256 }).toString()],
    aes: () => { const key = CryptoJS.enc.Hex.parse("000102030405060708090a0b0c0d0e0f"), iv = CryptoJS.enc.Hex.parse("0f0e0d0c0b0a09080706050403020100"); const c = CryptoJS.AES.encrypt(msg, key, { iv }); return [c.ciphertext.toString().slice(0, 64), CryptoJS.AES.decrypt(c, key, { iv }).toString(CryptoJS.enc.Utf8) === msg, CryptoJS.TripleDES.encrypt("hello", CryptoJS.enc.Hex.parse("0123456789abcdef0123456789abcdef0123456789abcdef"), { iv: CryptoJS.enc.Hex.parse("0001020304050607") }).toString(), CryptoJS.RC4.encrypt("hello", CryptoJS.enc.Utf8.parse("k")).ciphertext.toString()]; },
    enc: () => [CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse("äö€😀")), CryptoJS.enc.Utf8.stringify(CryptoJS.enc.Base64.parse("w6TDtuKCrPCfmIA=")), CryptoJS.enc.Utf16.parse("ab").toString()],
  },
  bench: {
    sha256: () => CryptoJS.SHA256(msg).words[0],
    sha512: () => CryptoJS.SHA512(msg).words[0],
    aes: () => CryptoJS.AES.encrypt(msg, CryptoJS.enc.Hex.parse("000102030405060708090a0b0c0d0e0f"), { iv: CryptoJS.enc.Hex.parse("0f0e0d0c0b0a09080706050403020100") }).ciphertext.sigBytes,
    pbkdf2: () => CryptoJS.PBKDF2("pw", "salt", { keySize: 4, iterations: 100 }).words[0],
  },
};
