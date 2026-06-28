# fastbogo

A very fast Rust CPU client for [bogo.swapjs.dev](https://bogo.swapjs.dev).

There are two versions of the kernel itself: a scalar version and a vectorized AVX2 version. The AVX2 version is only marginally faster than the scalar version (quite the testament to the efficiency of CPU schedulers!) but can achieve upwards of 180 cycles per permutation (orchestration amortized).

## Installation
Building normally is easy:
```bash
git clone https://github.com/Glitch752/fastbogo
cd fastbogo
cargo build --release
```

Set up a .env file with `BOGO_UUID`, `BOGO_NICK`, and `BOGO_CODE` values, then run:
```bash
cargo run --release
```

### Running offline
The client can be run in "offline mode" to test optimization strategies without connecting to the server.
```bash
cargo run --release -- --offline
```

### Benchmarking
The client can be benchmarked to measure its performance, though it also prints its current speed while running.
```bash
cargo run --release -- --benchmark --count 1000000000 --benchmark-warmup-rounds 1 --benchmark-rounds 3
```

### PGO
PGO (Profile Guided Optimization) can be used to optimize the performance of the client by providing runtime information to the compiler. The benchmarking mode is used by a few helper scripts to make PGO straightforward.

```bash
# Validate base performance
cargo run --release -- --benchmark --count 1000000000 --benchmark-warmup-rounds 1 --benchmark-rounds 3

# Store initial profiling data. Tune count based on your system performance
BENCH_COUNT=100000000 BENCH_WARMUP=1 BENCH_ROUNDS=4 ./scripts/pgo-generate.sh

# Use the generated profiling data to optimize the client
./scripts/pgo-build.sh

# Check optimized performance
cargo run --release -- --benchmark --count 100000000 --benchmark-warmup-rounds 1 --benchmark-rounds 3
```

### Tuning
There are a few important parameters to be tuned to optimize the client's performance:
- threads (`--threads`)
- prune threshold (`--prune-check-start`) (only affects the scalar version)

these can be automatically optimized using the benchmark:
```
cargo run --release -- --benchmark \
  --count 50000000 \
  --benchmark-warmup-rounds 1 \
  --benchmark-rounds 5 \
  --benchmark-thread-sweep 8,12,16 \
  --benchmark-prune-sweep 16,15,14,13,12
```
(of course, with the appropriate thread sweep parameters for your system. somewhere between the number of physical cores and logical cores is typically ideal for thread count)

The prune parameters will be saved to a JSON file on the system and loaded if not explicitly supplied. Make sure to re-run PGO with the tuned parameters!

## Credits

Admittedly, this project is a bit of a mess and partially made with AI assistance (though I wrote the kernel code, which was the interesting part to me). It's mostly an experiment and for my own learning.

This wouldn't be possible without the amazing work of [Mafiosoweb1's bogo-turbo](https://github.com/Mafiosoweb1/bogo-turbo/), a GPU implementation inspiring many of the optimizations used here.
