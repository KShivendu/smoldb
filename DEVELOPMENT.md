## Setup:

```sh
# required since we have gRPC APIs
sudo apt update && sudo apt upgrade -y
sudo apt install -y protobuf-compiler libprotobuf-dev

# You can also interact with the p2p (internal) gRPC API like this:
grpcurl -plaintext -import-path src/proto -proto root_api.proto 0.0.0.0:9920 smoldb_p2p_grpc.Service/RootApi
```


## Running benchmarks:

### smolbench

```sh
cargo run -p smolbench # upserts
cargo run -p smolbench -- -n 100k --uri http://localhost:9001 -b 1k

# terminal 1 (upsert):
cargo run -p smolbench --  --skip-query -n 100M --delay 1000 -b 100
# terminal 2 (search):
watch -n1 'cargo run -p smolbench --  --skip-upsert -n 100M --delay 1000 -b 100'
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
