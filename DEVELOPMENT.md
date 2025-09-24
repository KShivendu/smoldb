## Setup:

```sh
# required since we have gRPC APIs
sudo apt update && sudo apt upgrade -y
sudo apt install -y protobuf-compiler libprotobuf-dev

# You can also interact with the p2p (internal) gRPC API like this:
grpcurl -plaintext -import-path src/api/grpc/proto/ -proto smoldb.proto 0.0.0.0:5000 smoldb.Service/RootApi
```


## Running benchmarks:

### smolbench

```sh
cargo run -p smolbench # upserts
cargo run -p smolbench -- -n 100k --uri http://localhost:9001 -b 1k

# terminal 1 (upsert):
cargo run -p smolbench --  --skip-read -n 100M --delay 1000 -b 100
# terminal 2 (search):
watch -n1 'cargo run -p smolbench --  --skip-write -n 100M --delay 1000 -b 100'
```

### Criterion / flamegraph branch

```bash
cargo bench -- --list # List benches
cargo bench # Run all benches
cargo bench upserts # Run all benches in upserts group

# Access the reports at `target/criterion/report/index.html`
python -m http.server .

# For flame graph:
cargo flamegraph --bench upsert -o flamegraph.svg -- --bench
```

## Perf investigation:

```sh
# Terminal 1:
cargo build --profile perf
cargo build -r -p smolbench

# Terminal 1:
sudo perf record --call-graph dwarf -F 4000 -g ./target/perf/smoldb

# Terminal 2:
./target/release/smolbench --skip-create --skip-upsert --skip-read

# Terminal 1:
# Stop smoldb once smolbench runs
sudo chown $USER:$USER perf.data
hotspot perf.data # Install https://github.com/KDAB/hotspot
```

```sh
perf annotate --tui
```

### Errors:

By default, we use `color-backtrace` crate to show snippets.

## citation

if you find this work useful in your research, please consider citing:
```bibtex
@software{smoldb2025,
  author = {kshivendu},
  title = {kshivendu: a small distributed database built in Rust},
  year = {2025},
  publisher = {github},
  journal = {github repository},
  url = {https://github.com/kshivendu/smoldb}
}
```
