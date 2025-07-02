## Setup:

```sh
# required since we have gRPC APIs
sudo apt update && sudo apt upgrade -y
sudo apt install -y protobuf-compiler libprotobuf-dev

# You can also interact with the p2p (internal) gRPC API like this:
grpcurl -plaintext -import-path src/proto -proto root_api.proto 0.0.0.0:9920 smoldb_p2p_grpc.Service/RootApi
```


## Running benchmarks:

```bash
cargo bench -- --list # List benches
cargo bench # Run all benches
cargo bench upserts # Run all benches in upserts group

# Access the reports at `target/criterion/report/index.html`
python -m http.server .

# For flame graph:
cargo flamegraph --bench upsert -o flamegraph.svg -- --bench
```
