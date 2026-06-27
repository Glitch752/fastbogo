// Implementation taken almost directly from the website client for comparison purposes.

const N = 25;

const seed = process.argv[2] ? BigInt(process.argv[2]) : 1234567890123456789n;
const iterations = process.argv[3] ? parseInt(process.argv[3]) : 1000;

function splitmix64Step(zRef) {
  let z = (zRef.z + 0x9e3779b97f4a7c15n) & 0xffffffffffffffffn;
  zRef.z = z;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & 0xffffffffffffffffn;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & 0xffffffffffffffffn;
  return z ^ (z >> 31n);
}

function xseed(seed64) {
  const zRef = { z: seed64 };
  const a = splitmix64Step(zRef);
  const b = splitmix64Step(zRef);
  const s = new Uint32Array(4);
  s[0] = Number(a & 0xffffffffn);
  s[1] = Number((a >> 32n) & 0xffffffffn);
  s[2] = Number(b & 0xffffffffn);
  s[3] = Number((b >> 32n) & 0xffffffffn);
  if (!s[0] && !s[1] && !s[2] && !s[3]) s[0] = 1;
  return s;
}

function rotl32(x, k) {
  return ((x << k) | (x >>> (32 - k))) >>> 0;
}

function xnext(s) {
  const res = (rotl32((s[0] + s[3]) >>> 0, 7) + s[0]) >>> 0;
  const t = (s[1] << 9) >>> 0;
  s[2] = (s[2] ^ s[0]) >>> 0;
  s[3] = (s[3] ^ s[1]) >>> 0;
  s[1] = (s[1] ^ s[2]) >>> 0;
  s[0] = (s[0] ^ s[3]) >>> 0;
  s[2] = (s[2] ^ t) >>> 0;
  s[3] = rotl32(s[3], 11);
  return res;
}

function xint(s, max) {
  const thr = (0x100000000 % max) >>> 0;
  let x;
  do {
    x = xnext(s);
  } while (x < thr);
  return x % max;
}

const arr = new Uint8Array(N);
let best = -1;
let bestArr = null;
let bestIndex = 0;
for (let it = 0; it < iterations; it++) {
  const si = (seed + (BigInt(it) * 0x9e3779b97f4a7c15n)) & 0xffffffffffffffffn;
  const st = xseed(si);
  for (let i = 0; i < N; i++) arr[i] = i + 1;
  for (let i = N - 1; i > 0; i--) {
    const jj = xint(st, i + 1);
    const t = arr[i];
    arr[i] = arr[jj];
    arr[jj] = t;
  }
  let correct = 0;
  for (let i = 0; i < N; i++) {
    if (arr[i] === i + 1) correct++;
  }
  if (correct > best) {
    best = correct;
    bestArr = Array.from(arr);
    bestIndex = it;
  }
}

console.log(JSON.stringify({ best, bestIndex, bestArr }, null, 2));
