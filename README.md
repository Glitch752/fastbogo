# fastbogo

A fast Rust CPU client for the [bogo.swapjs.dev](https://bogo.swapjs.dev). Does all the same work as the website but faster :)

Admittedly, this project is a bit of a mess and partially made with AI assistance (though I wrote the majority of kernel code). It's mostly an experiment and for my own learning.

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
PGO (Profile Guided Optimization) can be used to optimize the performance of the client by providing runtime information to the compiler. There is a benchmarking mode and a few scripts to make it easier to use.

```bash
# Validate base performance
cargo run --release -- --benchmark --count 1000000000 --benchmark-warmup-rounds 1 --benchmark-rounds 3

# Store initial profiling data
BENCH_COUNT=100000000 BENCH_WARMUP=1 BENCH_ROUNDS=5 ./scripts/pgo-generate.sh

# Use the generated profiling data to optimize the client
./scripts/pgo-build.sh

# Check optimized performance
cargo run --release -- --benchmark --count 100000000 --benchmark-warmup-rounds 1 --benchmark-rounds 3
```